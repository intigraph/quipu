//! Protected local write grants and an authority/data commit barrier.
use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use super::Store;
use crate::writer_scope::{Capability, GraphInstance, WriteGrant, WriteOperation, denied};
use crate::{Error, Result};

impl Store {
    pub(super) fn migrate_writer_scope(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE IF NOT EXISTS writer_graph_instances (
                 g INTEGER PRIMARY KEY, generation TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS writer_grants (
                 id TEXT PRIMARY KEY, revision INTEGER NOT NULL, payload TEXT NOT NULL);
             INSERT OR IGNORE INTO writer_graph_instances(g,generation)
                 SELECT g,lower(hex(randomblob(32))) FROM graphs;
             CREATE TRIGGER IF NOT EXISTS writer_graph_insert AFTER INSERT ON graphs BEGIN
                 INSERT OR REPLACE INTO writer_graph_instances VALUES(NEW.g,lower(hex(randomblob(32))));
             END;
             CREATE TRIGGER IF NOT EXISTS writer_graph_delete AFTER DELETE ON graphs BEGIN
                 DELETE FROM writer_graph_instances WHERE g=OLD.g;
             END;
             CREATE TRIGGER IF NOT EXISTS writer_graph_update AFTER UPDATE ON graphs BEGIN
                 DELETE FROM writer_graph_instances WHERE g=OLD.g;
                 INSERT OR REPLACE INTO writer_graph_instances VALUES(NEW.g,lower(hex(randomblob(32))));
             END;
             COMMIT;",
        )?;
        Ok(())
    }

    /// Actual-target backstop for adapters before term interning.
    pub fn check_writer_scope(&self, graph: i64) -> Result<()> {
        crate::writer_scope::check(self, graph)
    }

    /// Owner-local observation for an enrollment packet. Does not mint a pin
    /// on lookup, accept ROOT, or treat an attached/overlay graph as local.
    pub fn writer_graph_instance(&self, iri: &str) -> Result<GraphInstance> {
        if iri == crate::schema::ROOT_GRAPH_IRI {
            return Err(denied());
        }
        let g = self
            .committed_graph_id(iri)?
            .filter(|g| *g != 0)
            .ok_or_else(denied)?;
        let pin: String = self.conn.query_row(
            "SELECT i.generation FROM main.writer_graph_instances i JOIN main.graphs r ON r.g=i.g WHERE i.g=?1 AND r.source IS NULL AND COALESCE(r.lifecycle,'') != 'frozen'",
            [g], |r| r.get(0),
        ).optional()?.ok_or_else(denied)?;
        let store_id = self.store_id()?;
        // Store ids name lineage; explicit adoption changes the local epoch.
        // A copied/adopted file must not silently reuse its old machine grant.
        let generation = format!(
            "{:x}",
            Sha256::digest(format!(
                "writer-graph-instance-v1\0{store_id}\0{}\0{pin}",
                self.log_epoch()?
            ))
        );
        Ok(GraphInstance {
            store_id,
            graph: iri.into(),
            generation,
        })
    }

    /// Trusted local enrollment API, deliberately absent from RDF/HTTP/MCP.
    /// The same BEGIN IMMEDIATE barrier is held by scoped data commits: once
    /// this call returns, an older capability cannot commit afterward.
    pub fn writer_grant_apply(&self, grant: &WriteGrant) -> Result<()> {
        self.ensure_ownership_writable()?;
        if crate::writer_scope::is_scoped_request() {
            return Err(denied());
        }
        grant.validate()?;
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| {
            if grant.instance.store_id != self.store_id()? {
                return Err(denied());
            }
            // Withdrawal must still work after graph recreation or deletion.
            if !grant.revoked
                && self.writer_graph_instance(&grant.instance.graph)? != grant.instance
            {
                return Err(denied());
            }
            let previous: Option<i64> = self
                .conn
                .query_row(
                    "SELECT revision FROM writer_grants WHERE id=?1",
                    [&grant.id],
                    |r| r.get(0),
                )
                .optional()?;
            let previous = previous
                .map(u64::try_from)
                .transpose()
                .map_err(|_| denied())?;
            if previous.is_some_and(|r| grant.revision <= r) {
                return Err(denied());
            }
            self.conn.execute(
                "INSERT INTO writer_grants VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,payload=excluded.payload",
                params![grant.id, grant.revision as i64, serde_json::to_string(grant).map_err(|_| denied())?],
            )?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                let committed =
                    crate::store::release::settle(&self.conn, self.conn.execute_batch("COMMIT"));
                if committed.is_err() {
                    let _ = self.conn.execute_batch("ROLLBACK");
                }
                committed.map_err(Error::from)
            }
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    /// Authorize the ENTIRE statically declared target set before work. The
    /// outer SQLite transaction serializes grants, generation and data across
    /// processes, not merely across this listener's mutex. Every inner graph
    /// transaction also checks the owned capability. No lease/cache fallback.
    pub fn with_writer_scope<T, E: From<Error>>(
        &mut self,
        operation: WriteOperation,
        graphs: &BTreeSet<String>,
        work: impl FnOnce(&mut Self) -> std::result::Result<T, E>,
    ) -> std::result::Result<T, E> {
        if !crate::writer_scope::is_scoped_request() {
            return work(self);
        }
        self.ensure_ownership_writable()?;
        if !self.conn.is_autocommit() {
            return Err(denied().into());
        }
        self.conn
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(Error::from)?;
        let mut audit = None;
        let mut work_succeeded = false;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let capability = self.writer_capability(operation, graphs)?;
            audit = Some(capability.audit.clone());
            let ids: Vec<_> = capability.instances.keys().copied().collect();
            crate::writer_scope::scoped(capability, || {
                let result = work(self)?;
                work_succeeded = true;
                // Expiry, identity and registry are rechecked before the ONE
                // outer commit, including no-op conditional updates.
                for graph in ids {
                    crate::writer_scope::check(self, graph)?;
                }
                crate::store::release::settle(&self.conn, self.conn.execute_batch("COMMIT"))
                    .map_err(Error::from)?;
                Ok(result)
            })
        }));
        let deferred =
            audit.map(|audit| std::mem::take(&mut *audit.lock().expect("scope audit poisoned")));
        match result {
            Ok(Ok(value)) => {
                if let Some(audit) = deferred {
                    audit.flush(self);
                }
                Ok(value)
            }
            failed => {
                let _ = self.conn.execute_batch("ROLLBACK");
                self.term_cache.borrow_mut().clear_persistent();
                self.read_model.borrow_mut().clear();
                self.policy_registry = None;
                self.clear_pending_write_events();
                let _ = self.take_deferred_embed();
                // As on a failed ordinary COMMIT, do not publish a satisfied
                // verdict for a write whose final authorization/commit failed.
                if work_succeeded || failed.is_err() {
                    self.pending_verdicts.clear();
                    self.pending_requests.clear();
                    self.pending_quarantine = None;
                }
                if let Some(audit) = deferred {
                    audit.flush(self);
                }
                match failed {
                    Ok(Err(e)) => Err(e),
                    Err(p) => std::panic::resume_unwind(p),
                    _ => unreachable!(),
                }
            }
        }
    }

    fn writer_capability(
        &self,
        op: WriteOperation,
        graphs: &BTreeSet<String>,
    ) -> Result<Capability> {
        let identity = crate::transaction_auth::current_identity().ok_or_else(denied)?;
        let credential_id = identity.credential_id.ok_or_else(denied)?;
        let now = crate::time::epoch_secs();
        let mut eligible = BTreeMap::new();
        let mut statement = self.conn.prepare("SELECT payload FROM writer_grants")?;
        for row in statement.query_map([], |r| r.get::<_, String>(0))? {
            let grant: WriteGrant = serde_json::from_str(&row?).map_err(|_| denied())?;
            grant.validate()?;
            if grant.permits(&identity.principal, graphs, now)
                && grant.credential_id == credential_id
                && grant.operations.contains(&op)
                && grant.instance.store_id == self.store_id()?
                && self
                    .writer_graph_instance(&grant.instance.graph)
                    .ok()
                    .as_ref()
                    == Some(&grant.instance)
            {
                eligible
                    .entry(grant.instance.graph.clone())
                    .or_insert((grant.instance, grant.valid_until));
            }
        }
        if graphs.is_empty() || graphs.len() > 64 {
            return Err(denied());
        }
        let mut instances = BTreeMap::new();
        let mut expires = u64::MAX;
        for graph in graphs {
            let (instance, expiry) = eligible.get(graph).ok_or_else(denied)?;
            let g = self.committed_graph_id(graph)?.ok_or_else(denied)?;
            instances.insert(g, instance.clone());
            expires = expires.min(*expiry);
        }
        Ok(Capability {
            principal: identity.principal,
            credential_id,
            instances,
            expires,
            authorized_at: now,
            audit: Default::default(),
        })
    }
}

#[cfg(test)]
mod tests;
