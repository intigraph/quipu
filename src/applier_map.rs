//! Which source transaction each local transaction of a replica belongs to
//! (`shared-applier-api.md` §6). A replica commits a source transaction as one
//! local transaction per graph, so its own log is numbered locally. A relay
//! serves its children in SOURCE numbering through this map.

use rusqlite::{OptionalExtension, params};

use super::Source;
use crate::error::Result;
use crate::store::Store;

pub(super) const MAP_DDL: &str = "CREATE TABLE IF NOT EXISTS replica_tx_map (\
     local_tx INTEGER PRIMARY KEY, store_id TEXT NOT NULL, log_epoch TEXT NOT NULL, \
     source_tx INTEGER NOT NULL)";

/// Record that `local_tx` holds part of `source_tx` (inside the apply savepoint).
pub(super) fn record(store: &Store, source: &Source, local_tx: i64, source_tx: i64) -> Result<()> {
    store.conn.execute(
        "INSERT INTO replica_tx_map (local_tx, store_id, log_epoch, source_tx) \
         VALUES (?1, ?2, ?3, ?4)",
        params![local_tx, source.store_id, source.log_epoch, source_tx],
    )?;
    Ok(())
}

/// The source transaction `local_tx` belongs to, if the applier wrote it.
///
/// # Errors
/// [`crate::Error::Sqlite`] if the map cannot be read.
pub fn source_tx_of(store: &Store, source: &Source, local_tx: i64) -> Result<Option<i64>> {
    if !exists(store)? {
        return Ok(None);
    }
    Ok(store
        .conn
        .query_row(
            "SELECT source_tx FROM replica_tx_map \
             WHERE local_tx = ?1 AND store_id = ?2 AND log_epoch = ?3",
            params![local_tx, source.store_id, source.log_epoch],
            |r| r.get(0),
        )
        .optional()?)
}

/// The last local transaction holding source transactions at or below
/// `source_tx`: a relay's local cursor for a child at that source cursor.
/// `None` when nothing that old is mapped (the child is behind the relay's
/// own bootstrap).
///
/// # Errors
/// [`crate::Error::Sqlite`] if the map cannot be read.
pub fn local_cursor(store: &Store, source: &Source, source_tx: i64) -> Result<Option<i64>> {
    if !exists(store)? {
        return Ok(None);
    }
    let floor: Option<i64> = store.conn.query_row(
        "SELECT MIN(source_tx) FROM replica_tx_map WHERE store_id = ?1 AND log_epoch = ?2",
        params![source.store_id, source.log_epoch],
        |r| r.get(0),
    )?;
    if floor.is_none_or(|f| source_tx < f) {
        return Ok(None);
    }
    Ok(store.conn.query_row(
        "SELECT MAX(local_tx) FROM replica_tx_map \
         WHERE store_id = ?1 AND log_epoch = ?2 AND source_tx <= ?3",
        params![source.store_id, source.log_epoch, source_tx],
        |r| r.get(0),
    )?)
}

fn exists(store: &Store) -> Result<bool> {
    Ok(store.conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'replica_tx_map')",
        [],
        |r| r.get(0),
    )?)
}

/// Where a replica stands, read without writing: the source it follows, the
/// last applied source transaction and the verified coverage. `None` when the
/// store is not (yet) a replica.
///
/// # Errors
/// [`crate::Error::Sqlite`] if the replica tables cannot be read.
pub fn position(store: &Store) -> Result<Option<(Source, i64, Option<i64>)>> {
    let has = |table: &str| -> Result<bool> {
        Ok(store.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table],
            |r| r.get(0),
        )?)
    };
    if !has("replica_progress")? {
        return Ok(None);
    }
    let Some((source, source_tx)) = store
        .conn
        .query_row(
            "SELECT store_id, log_epoch, source_tx FROM replica_progress WHERE id = 1",
            [],
            |r| {
                Ok((
                    Source {
                        store_id: r.get(0)?,
                        log_epoch: r.get(1)?,
                    },
                    r.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(None);
    };
    let coverage = if has("replica_coverage")? {
        store
            .conn
            .query_row(
                "SELECT watermark FROM replica_coverage \
                 WHERE id = 1 AND store_id = ?1 AND log_epoch = ?2",
                params![source.store_id, source.log_epoch],
                |r| r.get(0),
            )
            .optional()?
    } else {
        None
    };
    Ok(Some((source, source_tx, coverage)))
}
