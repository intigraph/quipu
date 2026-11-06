//! Logical full-store application stays inside the ONE shared Applier and
//! its existing progress row. No other projector or per-graph cursor.
use rusqlite::{OptionalExtension, params};
use sha2::{Digest, Sha256};

use super::{Applier, Outcome, Progress, Source};
use crate::store_replica::{
    FullStoreManifest, SnapshotHeader, SnapshotRow, SnapshotTrust, catalog, invalid, rows,
};
use crate::{Result, Store};

pub(super) struct Cursor {
    pub generation: String,
    pub revision: i64,
    pub catalog_digest: String,
    pub digest: String,
}

type StoredCursor = (Option<String>, Option<i64>, Option<String>, Option<String>);

pub(super) fn prepare(store: &Store) -> Result<Option<Cursor>> {
    let columns = store
        .conn
        .prepare("PRAGMA table_info(replica_progress)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (name, kind) in [
        ("store_generation", "TEXT"),
        ("store_revision", "INTEGER"),
        ("store_catalog", "TEXT"),
        ("store_digest", "TEXT"),
    ] {
        if !columns.iter().any(|c| c == name) {
            store.conn.execute_batch(&format!(
                "ALTER TABLE replica_progress ADD COLUMN {name} {kind}"
            ))?;
        }
    }
    let fields: Option<StoredCursor> = store.conn.query_row(
        "SELECT store_generation,store_revision,store_catalog,store_digest FROM replica_progress WHERE id=1",
        [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
    ).optional()?;
    match fields {
        None | Some((None, None, None, None)) => Ok(None),
        Some((Some(generation), Some(revision), Some(catalog_digest), Some(digest)))
            if revision >= 0 =>
        {
            Ok(Some(Cursor {
                generation,
                revision,
                catalog_digest,
                digest,
            }))
        }
        _ => Err(invalid(
            "incomplete logical composite cursor; admission refused",
        )),
    }
}

fn trust_digest(trust: &SnapshotTrust) -> String {
    let value = serde_json::json!({"key":trust.public_key_hex,"source":trust.source,
        "generation":trust.journal_generation,"catalog":trust.catalog_digest,"cut":trust.manifest_sha256});
    hex::encode(Sha256::digest(crate::replicate_wire::canonical(&value)))
}

fn invalidate(store: &Store) {
    store.term_cache.borrow_mut().clear_persistent();
    store.invalidate_read_model();
    store.projected_graph.borrow_mut().take();
    store.affordability_counts.borrow_mut().clear();
}

fn persist(store: &Store, source: &Source, tx: i64, cursor: &Cursor) -> Result<()> {
    store.conn.execute("INSERT INTO replica_progress \
        (id,store_id,log_epoch,source_tx,store_generation,store_revision,store_catalog,store_digest) \
        VALUES(1,?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET \
        store_id=?1,log_epoch=?2,source_tx=?3,store_generation=?4,store_revision=?5,store_catalog=?6,store_digest=?7",
        params![source.store_id,source.log_epoch,tx,cursor.generation,cursor.revision,cursor.catalog_digest,cursor.digest])?;
    store.conn.execute(
        "INSERT OR IGNORE INTO replica_tx_map(local_tx,store_id,log_epoch,source_tx) \
        SELECT id,?1,?2,id FROM transactions",
        params![source.store_id, source.log_epoch],
    )?;
    Ok(())
}

impl Applier {
    /// Journal position from the same persisted progress row as `progress()`.
    /// Metadata-only application can advance this without a new fact tx.
    #[must_use]
    pub fn logical_revision(&self) -> Option<i64> {
        self.logical.as_ref().map(|cursor| cursor.revision)
    }

    /// Pin an independently admitted logical-store descriptor. Reopening a
    /// replica requires the same descriptor; legacy admission cannot upgrade.
    ///
    /// # Errors
    /// Different existing descriptor, active legacy progress, or open share.
    pub fn require_logical_store(&mut self, trust: SnapshotTrust) -> Result<()> {
        if self.share.is_some()
            || self.chunk.is_some()
            || (self.progress.is_some() && self.logical.is_none())
            || self.bootstrap_trust.is_some()
        {
            return Err(invalid(
                "cannot replace legacy or active replication with logical admission",
            ));
        }
        let digest = trust_digest(&trust);
        if self
            .bootstrap_admission
            .as_ref()
            .is_some_and(|old| old != &digest)
        {
            return Err(invalid(
                "logical bootstrap descriptor differs from persisted admission",
            ));
        }
        self.logical_trust = Some(trust);
        self.end_session();
        Ok(())
    }

    /// Consume one privately staged, bounded-line logical spool. Only a fully
    /// authenticated manifest and exact complete spool publish data + the
    /// composite cursor together. Lines include their trailing newline.
    ///
    /// # Errors
    /// Unadmitted cut, dirty receiving store, invalid schema/cells, partial or
    /// altered spool, source/cursor mismatch, ownership/SQL or writer refusal.
    pub fn bootstrap_logical_store(
        &mut self,
        store: &mut Store,
        manifest: &FullStoreManifest,
        signature: &str,
        lines: impl IntoIterator<Item = Result<Vec<u8>>>,
    ) -> Result<Outcome> {
        self.end_session();
        let trust = self
            .logical_trust
            .as_ref()
            .ok_or_else(|| invalid("logical bootstrap trust is not configured"))?;
        if !trust.accepts(manifest, signature)
            || manifest.header.revision < 0
            || manifest.header.source_tx < 0
        {
            return Err(invalid("logical bootstrap signature/descriptor refused"));
        }
        if self.progress.is_some() || self.stopped.is_some() || !store.conn.is_autocommit() {
            return Err(invalid(
                "logical bootstrap requires an empty unblocked replica",
            ));
        }
        let local = catalog::inspect(store)?;
        if local != manifest.header.catalog || catalog::digest(&local)? != trust.catalog_digest {
            return Err(invalid(
                "logical bootstrap differs from locally recognized schema",
            ));
        }
        rows::require_fresh_target(store, &local)?;
        store.ensure_ownership_writable()?;
        store
            .conn
            .execute_batch("SAVEPOINT replica_logical; PRAGMA defer_foreign_keys=ON")?;
        let source = Source {
            store_id: trust.source.store_id.clone(),
            log_epoch: trust.source.log_epoch.clone(),
        };
        let cursor = Cursor {
            generation: trust.journal_generation.clone(),
            revision: manifest.header.revision,
            catalog_digest: trust.catalog_digest.clone(),
            digest: trust.manifest_sha256.clone(),
        };
        let admission = trust_digest(trust);
        let result = (|| -> Result<()> {
            for table in local.iter().rev() {
                store
                    .conn
                    .execute(&format!("DELETE FROM {}", catalog::quote(&table.name)), [])?;
            }
            let mut hash = Sha256::new();
            let mut bytes = 0_u64;
            let mut count = 0_u64;
            let mut first = true;
            for line in lines {
                let line = line?;
                if line.len() > 8 * 1024 * 1024 || !line.ends_with(b"\n") {
                    return Err(invalid("logical snapshot line is oversized or incomplete"));
                }
                bytes = bytes
                    .checked_add(line.len() as u64)
                    .ok_or_else(|| invalid("logical byte overflow"))?;
                if bytes > manifest.bytes {
                    return Err(invalid("logical snapshot exceeds signed byte count"));
                }
                hash.update(&line);
                if first {
                    first = false;
                    let header: SnapshotHeader =
                        serde_json::from_slice(&line).map_err(|e| invalid(e.to_string()))?;
                    if serde_json::to_value(header).map_err(|e| invalid(e.to_string()))?
                        != serde_json::to_value(&manifest.header)
                            .map_err(|e| invalid(e.to_string()))?
                    {
                        return Err(invalid(
                            "logical snapshot header differs from signed manifest",
                        ));
                    }
                    continue;
                }
                let row: SnapshotRow =
                    serde_json::from_slice(&line).map_err(|e| invalid(e.to_string()))?;
                let table = local
                    .iter()
                    .find(|t| t.name == row.table)
                    .ok_or_else(|| invalid("unadmitted logical table"))?;
                rows::insert(store, table, &row.cells)?;
                count = count
                    .checked_add(1)
                    .ok_or_else(|| invalid("logical row overflow"))?;
                if count > manifest.rows {
                    return Err(invalid("logical snapshot exceeds signed row count"));
                }
            }
            if first
                || bytes != manifest.bytes
                || count != manifest.rows
                || hex::encode(hash.finalize()) != manifest.spool_sha256
            {
                return Err(invalid("logical snapshot is incomplete or altered"));
            }
            if store.latest_tx_id()? != manifest.header.source_tx {
                return Err(invalid(
                    "logical snapshot transaction history differs from signed head",
                ));
            }
            persist(store, &source, manifest.header.source_tx, &cursor)?;
            store.conn.execute(
                "INSERT INTO replica_bootstrap_admission(id,descriptor_digest) VALUES(1,?1)",
                [&admission],
            )?;
            crate::store::release::release(&store.conn, "replica_logical")?;
            Ok(())
        })();
        invalidate(store);
        if let Err(error) = result {
            if !store.conn.is_autocommit() {
                crate::store::release::rollback_release(&store.conn, "replica_logical")?;
            }
            return Err(error);
        }
        self.progress = Some(Progress {
            source,
            source_tx: manifest.header.source_tx,
        });
        self.logical = Some(cursor);
        self.bootstrap_admission = Some(admission);
        Ok(Outcome::Bootstrapped {
            source_tx: manifest.header.source_tx,
        })
    }

    /// Apply one complete signed logical revision atomically with the existing
    /// composite progress row. The signature is checked under the externally
    /// pinned bootstrap key; metadata-only updates require no invented fact tx.
    /// This does not establish freshness without a separate admitted proof.
    ///
    /// # Errors
    /// Missing admission, bad signature/source/epoch/catalog, gap/conflicting
    /// redelivery, malformed rows, incomplete transaction history, SQL or
    /// ownership/commit refusal. Failure preserves prior data and cursor.
    pub fn apply_logical_revision(
        &mut self,
        store: &mut Store,
        revision: &crate::store_replica::Revision,
        signature: &str,
    ) -> Result<Outcome> {
        self.end_session();
        let trust = self
            .logical_trust
            .as_ref()
            .ok_or_else(|| invalid("logical admission missing"))?;
        let current = self
            .logical
            .as_ref()
            .ok_or_else(|| invalid("logical bootstrap required"))?;
        let progress = self
            .progress
            .as_ref()
            .ok_or_else(|| invalid("logical progress missing"))?;
        if !store.conn.is_autocommit() || self.stopped.is_some() {
            return Err(invalid(
                "logical revision requires an unblocked idle writer",
            ));
        }
        if revision.source != trust.source
            || revision.source.store_id != progress.source.store_id
            || revision.source.log_epoch != progress.source.log_epoch
            || revision.journal.generation != current.generation
            || revision.journal.catalog_digest != current.catalog_digest
            || revision.source_tx < progress.source_tx
            || !crate::store_replica::verify_revision(revision, &trust.public_key_hex, signature)
        {
            return Err(invalid(
                "logical signature/source/epoch/catalog admission refused",
            ));
        }
        let digest = crate::store_replica::revision_hash(revision)?;
        if revision.through == current.revision
            && revision.since == current.revision
            && revision.changes.is_empty()
            && revision.source_tx == progress.source_tx
        {
            return Ok(Outcome::Idle);
        }
        if revision.through == current.revision
            && digest == current.digest
            && revision.source_tx == progress.source_tx
        {
            return Ok(Outcome::Duplicate);
        }
        let count = revision
            .through
            .checked_sub(revision.since)
            .and_then(|n| usize::try_from(n).ok());
        if revision.since != current.revision
            || revision.through <= current.revision
            || count != Some(revision.changes.len())
        {
            return Err(invalid(
                "logical gap or conflicting redelivery; resume/bootstrap required",
            ));
        }
        let catalog = catalog::inspect(store)?;
        if catalog::digest(&catalog)? != current.catalog_digest {
            return Err(invalid("logical receiver schema changed"));
        }
        let cursor = Cursor {
            generation: current.generation.clone(),
            catalog_digest: current.catalog_digest.clone(),
            revision: revision.through,
            digest,
        };
        let source = progress.source.clone();
        let previous_tx = progress.source_tx;
        store.ensure_ownership_writable()?;
        store
            .conn
            .execute_batch("SAVEPOINT replica_logical; PRAGMA defer_foreign_keys=ON")?;
        let result = (|| -> Result<()> {
            for (index, change) in revision.changes.iter().enumerate() {
                let expected = revision
                    .since
                    .checked_add(index as i64)
                    .and_then(|n| n.checked_add(1));
                if Some(change.revision) != expected {
                    return Err(invalid("logical revision contains a gap/reordered row"));
                }
                let table = catalog
                    .iter()
                    .find(|t| t.name == change.table)
                    .ok_or_else(|| invalid("logical mutation names an unadmitted table"))?;
                match (change.operation.as_str(), change.row.as_ref()) {
                    ("put", Some(row)) => {
                        if row.len() != table.columns.len() {
                            return Err(invalid("logical mutation row width differs"));
                        }
                        let keys = table
                            .key_indices()
                            .iter()
                            .map(|&i| row[i].to_sql())
                            .collect::<Result<Vec<_>>>()?;
                        let declared = change
                            .key
                            .iter()
                            .map(crate::store_replica::Cell::to_sql)
                            .collect::<Result<Vec<_>>>()?;
                        if keys != declared {
                            return Err(invalid("logical mutation key differs from row"));
                        }
                        rows::put(store, table, row)?;
                    }
                    ("delete", None) => rows::delete(store, table, &change.key)?,
                    _ => return Err(invalid("logical mutation operation/row shape invalid")),
                }
            }
            if store.latest_tx_id()? != revision.source_tx {
                return Err(invalid(
                    "logical transaction history differs from signed head",
                ));
            }
            persist(store, &source, revision.source_tx, &cursor)?;
            crate::store::release::release(&store.conn, "replica_logical")?;
            Ok(())
        })();
        invalidate(store);
        if let Err(error) = result {
            if !store.conn.is_autocommit() {
                crate::store::release::rollback_release(&store.conn, "replica_logical")?;
            }
            return Err(error);
        }
        self.logical = Some(cursor);
        self.progress = Some(Progress {
            source,
            source_tx: revision.source_tx,
        });
        Ok(Outcome::Applied {
            source_txs: if revision.source_tx > previous_tx {
                vec![revision.source_tx]
            } else {
                vec![]
            },
        })
    }
}
