//! Durable caller declarations beside credential evidence, never authority.
//!
//! Rows commit or roll back with the transaction that owns them. Historical
//! transactions and non-request/internal writes have no row: unknown, not an
//! invented agent or a reconstruction from free text. Share excludes these local
//! declarations, including in reconstruction packs; a raw SQLite backup retains them.

use crate::write_provenance::Declared;
use rusqlite::{Connection, OptionalExtension, params};

/// Persist within the caller's open savepoint, using the metric population.
pub(crate) fn record(
    conn: &Connection,
    tx: i64,
    actor: Option<&str>,
    source: Option<&str>,
) -> crate::Result<()> {
    use crate::write_kind::WriteKind;
    if matches!(
        crate::write_kind::classify(actor, source),
        WriteKind::Verdict | WriteKind::Reasoner | WriteKind::Migration
    ) {
        return Ok(());
    }
    let Some(p) = crate::write_provenance::current() else {
        return Ok(());
    };
    let d = &p.declared;
    conn.execute(
        "INSERT INTO transaction_provenance \
        (tx, client, endpoint, completeness, agent, harness, host, session, model, task, client_version) \
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![tx, p.client, p.endpoint, p.completeness.as_str(), d.agent, d.harness,
            d.host, d.session, d.model, d.task, d.client_version],
    )?;
    Ok(())
}

/// Values declared by one request. Authentication remains a separate object.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TransactionProvenance {
    pub tx: i64,
    pub client: String,
    pub endpoint: String,
    pub completeness: String,
    pub declared: Declared,
}

fn exists(conn: &Connection) -> crate::Result<bool> {
    Ok(conn.query_row(
        "SELECT count(*)=2 FROM sqlite_master WHERE type='table' AND name IN ('transaction_provenance','transaction_provenance_state')",
        [], |row| row.get(0))?)
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<TransactionProvenance> {
    Ok(TransactionProvenance {
        tx: r.get(0)?,
        client: r.get(1)?,
        endpoint: r.get(2)?,
        completeness: r.get(3)?,
        declared: Declared {
            agent: r.get(4)?,
            harness: r.get(5)?,
            host: r.get(6)?,
            session: r.get(7)?,
            model: r.get(8)?,
            task: r.get(9)?,
            client_version: r.get(10)?,
        },
    })
}

/// Scope and schema floor of this local provenance provider.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProvenanceState {
    pub store_id: String,
    pub incarnation: String,
    pub start_tx: i64,
    pub provider: &'static str,
}

impl crate::Store {
    /// Missing schema is unavailable, never proof of empty/complete history.
    pub fn provenance_state(&self) -> crate::Result<Option<ProvenanceState>> {
        if !exists(&self.conn)? {
            return Ok(None);
        }
        let state = self
            .conn
            .query_row(
                "SELECT start_tx, incarnation FROM transaction_provenance_state WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((start_tx, incarnation)) = state else {
            return Ok(None);
        };
        Ok(Some(ProvenanceState {
            store_id: self.store_id()?,
            incarnation,
            start_tx,
            provider: "local-store",
        }))
    }

    /// Local declaration for an exact transaction, or unknown on old/read-only DBs.
    pub fn transaction_provenance(&self, tx: i64) -> crate::Result<Option<TransactionProvenance>> {
        if !exists(&self.conn)? {
            return Ok(None);
        }
        Ok(self
            .conn
            .query_row(
                "SELECT * FROM transaction_provenance WHERE tx=?1",
                [tx],
                row,
            )
            .optional()?)
    }

    /// Bounded transaction-order page for coverage; never a full-history count.
    pub fn provenance_window(
        &self,
        since: i64,
        limit: usize,
    ) -> crate::Result<Vec<TransactionProvenance>> {
        if !exists(&self.conn)? {
            return Ok(Vec::new());
        }
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM transaction_provenance WHERE tx>?1 ORDER BY tx LIMIT ?2")?;
        Ok(stmt
            .query_map(params![since, limit.clamp(1, 10_000) as i64], row)?
            .collect::<Result<Vec<_>, _>>()?)
    }

    /// A keyset page for one session or agent. At least one selector is required.
    /// Session carriers are not global identities: callers must compare the
    /// returned agent/harness/host, or supply an agent as an additional filter.
    /// Historical transactions without declarations cannot match this query.
    pub fn provenance_transactions(
        &self,
        since: i64,
        limit: usize,
        session: Option<&str>,
        agent: Option<&str>,
    ) -> crate::Result<Vec<TransactionProvenance>> {
        if session.is_none() && agent.is_none() {
            return Err(crate::Error::InvalidValue(
                "session or agent is required".into(),
            ));
        }
        if !exists(&self.conn)? {
            return Ok(Vec::new());
        }
        // Choose the indexable leading selector explicitly, not an OR/NULL
        // parameter predicate that makes SQLite scan the whole audit table.
        let mut stmt = if session.is_some() && agent.is_some() {
            self.conn.prepare("SELECT * FROM transaction_provenance WHERE session=?1 AND agent=?4 AND tx>?2 ORDER BY tx LIMIT ?3")?
        } else if session.is_some() {
            self.conn.prepare("SELECT * FROM transaction_provenance WHERE session=?1 AND tx>?2 ORDER BY tx LIMIT ?3")?
        } else {
            self.conn.prepare("SELECT * FROM transaction_provenance WHERE agent=?1 AND tx>?2 ORDER BY tx LIMIT ?3")?
        };
        let selector = session.or(agent).unwrap();
        let limit = limit.clamp(1, 10_000) as i64;
        let rows = if session.is_some() && agent.is_some() {
            stmt.query_map(params![selector, since, limit, agent], row)?
        } else {
            stmt.query_map(params![selector, since, limit], row)?
        };
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

#[cfg(test)]
#[path = "transaction_provenance_tests.rs"]
mod tests;
