//! Event-log retention (quipu-9z9, aegis-jio5.5).
//!
//! Deletes events whose `ts` is older than a cutoff AND that every registered
//! consumer has already committed past. With rows in `consumers`, only ROOT
//! offsets at or below `MIN(committed_offset)` are eligible, so an event a
//! consumer has not consumed is kept however old it is, a committed cursor
//! never points at a hole, and a lagging consumer's backlog stays on disk.
//! Named-graph events wait only on that graph's consumers. With NO registered
//! consumers there is no cursor to honour and age alone decides.
//!
//! A consumer is registered only by committing a cursor (`POST
//! /events/commit`, `/events/graph/commit`). A reader that only pulls
//! `/events?since=` or streams SSE is not protected.
//!
//! `ts` is compared as an instant through `julianday`, because the log holds
//! `Z`, numeric-offset and other forms: a text comparison against a `Z`
//! cutoff deletes offset-form rows that are still inside the window. A `ts`
//! that `julianday` cannot parse (for example `reactive`) is kept rather
//! than guessed at.
//!
//! Offsets are `AUTOINCREMENT`, so pruning never causes offset reuse. What
//! retention gives up is genesis replay: a consumer registering after a
//! prune replays from the oldest retained event.

use rusqlite::params;

use crate::{Result, Store};

impl Store {
    /// Delete up to `limit` eligible events (ROOT first, then named graphs)
    /// older than `older_than` (an ISO-8601 instant) and return how many
    /// were deleted. Lowest offsets go first, so repeated calls drain a
    /// backlog in log order; a call returning less than `limit` found nothing
    /// more to delete. Each call is one bounded statement per table, so a
    /// caller can release the store lock between calls.
    ///
    /// # Errors
    /// [`crate::Error::Sqlite`] if a read or delete fails.
    pub fn prune_events_batch(&self, older_than: &str, limit: usize) -> Result<usize> {
        if limit == 0 {
            return Ok(0);
        }
        let cap = i64::try_from(limit).unwrap_or(i64::MAX);
        let safe: i64 = self.conn.query_row(
            "SELECT COALESCE(MIN(committed_offset), ?1) FROM consumers",
            params![i64::MAX],
            |r| r.get(0),
        )?;
        let root = self.conn.execute(
            "DELETE FROM events WHERE \"offset\" IN (\
                 SELECT \"offset\" FROM events \
                 WHERE \"offset\" <= ?2 AND julianday(ts) < julianday(?1) \
                 ORDER BY \"offset\" LIMIT ?3)",
            params![older_than, safe, cap],
        )?;
        let left = cap - i64::try_from(root).unwrap_or(cap);
        if left <= 0 || !self.has_graph_events()? {
            return Ok(root);
        }
        let graph = self.conn.execute(
            "DELETE FROM graph_events WHERE \"offset\" IN (\
                 SELECT e.\"offset\" FROM graph_events e \
                 WHERE e.\"offset\" <= COALESCE((SELECT MIN(c.committed_offset) \
                     FROM graph_consumers c WHERE c.graph = e.graph), ?2) \
                   AND julianday(e.ts) < julianday(?1) \
                 ORDER BY e.\"offset\" LIMIT ?3)",
            params![older_than, i64::MAX, left],
        )?;
        Ok(root + graph)
    }

    /// `ts` of the lowest-offset ROOT event still retained, or `None` for an
    /// empty log. Counts over the log (for example `quipu events refusals`)
    /// cover only events from here on once retention has pruned.
    ///
    /// # Errors
    /// [`crate::Error::Sqlite`] if the read fails.
    pub fn oldest_retained_event_ts(&self) -> Result<Option<String>> {
        use rusqlite::OptionalExtension as _;
        Ok(self
            .conn
            .query_row(
                "SELECT ts FROM events ORDER BY \"offset\" LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Prune every eligible event older than `older_than`, in batches, and
    /// return how many were deleted. For callers that hold the store for the
    /// whole prune (tests, the CLI); the server uses
    /// [`Self::prune_events_batch`] so it can release the lock between
    /// batches.
    ///
    /// # Errors
    /// [`crate::Error::Sqlite`] if a read or delete fails.
    pub fn prune_events(&self, older_than: &str) -> Result<u64> {
        const BATCH: usize = 10_000;
        let mut total = 0u64;
        loop {
            let n = self.prune_events_batch(older_than, BATCH)?;
            total += n as u64;
            if n < BATCH {
                return Ok(total);
            }
        }
    }
}

#[cfg(test)]
#[path = "events_retention_tests.rs"]
mod tests;
