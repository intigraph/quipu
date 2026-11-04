//! Reconstruct the base independently in the master's writer snapshot.

use rusqlite::params;

use super::*;
use crate::client_changeset::{BaseJournal, BaseRow};
use crate::types::{Fact, Op, Value};

pub(super) fn graph_id(store: &Store, graph: &str) -> Result<i64> {
    let graphs = crate::asof_share::resolve_graphs(store, &[graph.to_owned()], &[])
        .map_err(|e| refuse(&format!("scope: {e:?}")))?;
    crate::asof_share::refuse_frozen(store, &graphs)
        .map_err(|e| refuse(&format!("base-unavailable: {e:?}")))?;
    graphs
        .first()
        .map(|(_, id)| *id)
        .ok_or_else(|| refuse("unknown graph"))
}

pub(super) fn rows(store: &Store, graph: i64, tx: i64) -> Result<Vec<BaseRow>> {
    let mut statement = store.conn.prepare(
        "SELECT e,a,v,tx,valid_from,valid_to,retracted_tx FROM facts \
         WHERE g=?1 AND op=1 AND tx<=?2 AND (retracted_tx IS NULL OR retracted_tx>?2) \
         ORDER BY e,a,v,tx",
    )?;
    let mut cursor = statement.query(params![graph, tx])?;
    let mut out = Vec::new();
    while let Some(row) = cursor.next()? {
        let from: String = row.get(4)?;
        let mut to: Option<String> = row.get(5)?;
        let closed: Option<i64> = row.get(6)?;
        // A later retraction was not part of the frozen base. In particular,
        // it must not change the digest of an outstanding overlay's journal.
        if closed.is_some_and(|closed| closed > tx) {
            to = None;
        }
        if to.as_deref() == Some(from.as_str()) {
            continue;
        } // zero-duration receipt
        if to.is_some() {
            return Err(refuse(
                "UNKNOWN bounded-valid-time context; base-unavailable",
            ));
        }
        let fact = Fact {
            entity: row.get(0)?,
            attribute: row.get(1)?,
            value: Value::from_bytes(&row.get::<_, Vec<u8>>(2)?)?,
            tx: row.get(3)?,
            valid_from: from.clone(),
            valid_to: to.clone(),
            op: Op::Assert,
        };
        let s = crate::changeset_store::fact_statement(store, &fact, None)?;
        out.push(BaseRow {
            triple: s.triple,
            valid_from: from,
            valid_to: to,
        });
    }
    Ok(out)
}

pub(super) fn journal(store: &Store, graph: &str, tx: i64) -> Result<BaseJournal> {
    if tx < 1 || tx > store.latest_tx_id()? || store.get_transaction(tx)?.is_none() {
        return Err(refuse("base-unavailable"));
    }
    let graph_id = graph_id(store, graph)?;
    let shapes = store
        .get_combined_shapes_as_of(&super::super::AsOf {
            tx: Some(tx),
            valid_at: None,
        })?
        .ok_or_else(|| refuse("UNKNOWN base shapes"))?;
    BaseJournal::new(
        store.store_id()?,
        store.log_epoch()?,
        tx,
        graph.to_owned(),
        digest(shapes.as_bytes()),
        rows(store, graph_id, tx)?,
    )
}

impl Store {
    /// Freeze a source base in one read snapshot. A native caller already
    /// holding the store file is privileged; this method arms no read route.
    pub fn client_base_journal(&self, graph: &str, tx: i64) -> Result<BaseJournal> {
        let read = self
            .conn
            .is_autocommit()
            .then(|| self.conn.unchecked_transaction())
            .transpose()?;
        let result = journal(self, graph, tx);
        if let Some(read) = read {
            read.commit()?;
        }
        result
    }
}
