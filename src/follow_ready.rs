//! Whether a follower database may serve reads in place of its primary
//! (aegis-tnh). A failover router asks this of the replica, so the answer
//! cannot depend on the primary being reachable: that is the case it exists
//! for.
//!
//! A follower is ready only when all of these hold:
//! - it was built by the applier ([`crate::follow`]), so it has a source
//!   lineage, and that lineage is the store the operator expects;
//! - its bootstrap reached the primary's watermark at least once on that
//!   lineage. Until then a missing item reads as absent, which looks
//!   authoritative and is not;
//! - the applier has not stopped it, and its lineage has not ended (an epoch
//!   change at the source). Either one means rebuild, never serve.
//!
//! Staleness is not part of readiness. Every replica response states it in
//! the `X-Quipu-Replica-*` headers.

use rusqlite::OptionalExtension as _;

use crate::error::Result;
use crate::store::Store;

const CAUGHT_UP_DDL: &str = "CREATE TABLE IF NOT EXISTS follow_caught_up (\
     id INTEGER PRIMARY KEY CHECK (id = 1), \
     store_id TEXT NOT NULL, log_epoch TEXT NOT NULL, source_tx INTEGER NOT NULL, \
     at TEXT NOT NULL)";

/// Record that the follower's bootstrap reached the primary's watermark on its
/// current lineage. The first record stands: a later catch-up on the same
/// lineage changes nothing, and a follower on another lineage is a rebuild.
///
/// # Errors
/// A store error reading the progress record or writing the marker.
pub fn mark_caught_up(store: &Store) -> Result<()> {
    store.conn.execute_batch(CAUGHT_UP_DDL)?;
    store.conn.execute(
        "INSERT OR IGNORE INTO follow_caught_up (id, store_id, log_epoch, source_tx, at) \
         SELECT 1, store_id, log_epoch, source_tx, strftime('%Y-%m-%dT%H:%M:%SZ', 'now') \
         FROM replica_progress WHERE id = 1",
        [],
    )?;
    Ok(())
}

/// A follower's readiness to serve reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readiness {
    /// It may serve reads for `lineage`.
    Ready {
        /// The source store it follows.
        lineage: String,
        /// The last source transaction it holds in full.
        source_tx: i64,
        /// When its bootstrap first reached the primary's watermark.
        caught_up_at: String,
    },
    /// It must not serve reads in place of the primary.
    NotReady {
        /// Why, in words an operator can act on.
        reason: String,
    },
}

fn has_table(store: &Store, name: &str) -> Result<bool> {
    Ok(store.conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [name],
        |r| r.get(0),
    )?)
}

fn not_ready(reason: impl Into<String>) -> Result<Readiness> {
    Ok(Readiness::NotReady {
        reason: reason.into(),
    })
}

/// The readiness of `store` as a follower of `expected_store_id`. Reads only,
/// so it works on the replica's read-only connection.
///
/// # Errors
/// A store error reading the follower's records.
pub fn readiness(store: &Store, expected_store_id: Option<&str>) -> Result<Readiness> {
    let Some(expected) = expected_store_id else {
        return not_ready("server.replica.expected_store_id is not configured");
    };
    if !has_table(store, "replica_progress")? {
        return not_ready("not a follower built by quipu follow");
    }
    let Some((lineage, epoch, source_tx)) = store
        .conn
        .query_row(
            "SELECT store_id, log_epoch, source_tx FROM replica_progress WHERE id = 1",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get(2)?)),
        )
        .optional()?
    else {
        return not_ready("the follower holds no source transaction yet");
    };
    if lineage != expected {
        return not_ready(format!(
            "the follower holds {lineage}, not the expected {expected}"
        ));
    }
    if has_table(store, "replica_stop")? {
        let stop: Option<String> = store
            .conn
            .query_row("SELECT reason FROM replica_stop WHERE id = 1", [], |r| {
                r.get(0)
            })
            .optional()?;
        if let Some(reason) = stop {
            return not_ready(format!("the applier stopped this follower: {reason}"));
        }
    }
    if has_table(store, "replica_source_end")? {
        let ended: Option<String> = store
            .conn
            .query_row(
                "SELECT reason FROM replica_source_end WHERE store_id = ?1 AND log_epoch = ?2",
                [&lineage, &epoch],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(reason) = ended {
            return not_ready(format!("the source lineage ended: {reason}"));
        }
    }
    let caught_up: Option<String> = if has_table(store, "follow_caught_up")? {
        store
            .conn
            .query_row(
                "SELECT at FROM follow_caught_up \
                 WHERE id = 1 AND store_id = ?1 AND log_epoch = ?2",
                [&lineage, &epoch],
                |r| r.get(0),
            )
            .optional()?
    } else {
        None
    };
    let Some(caught_up_at) = caught_up else {
        return not_ready("the bootstrap has not reached the primary's watermark");
    };
    Ok(Readiness::Ready {
        lineage,
        source_tx,
        caught_up_at,
    })
}

#[cfg(test)]
#[path = "follow_ready_tests.rs"]
mod tests;
