//! Named-graph semantic events (event-push design §4): their own log,
//! `graph_events`, and their readers' cursors, `graph_consumers`. Kept apart
//! from `events` so that a store written here reads the same to any older
//! binary (see `migrate_graph_events`).

use std::collections::HashSet;

use rusqlite::{OptionalExtension, params};
use serde_json::json;

use super::{Datum, Store};
use crate::error::Result;

impl Store {
    /// Entity-level events for a commit to a registered COMMITTED named graph;
    /// a write to an overlay or an unregistered graph emits nothing. Same savepoint,
    /// same atomicity as the ROOT events.
    pub(super) fn emit_named_graph_events(
        &self,
        asserts: &[&Datum],
        retracts: &[&Datum],
        tx_id: i64,
        timestamp: &str,
        source: Option<&str>,
        graph: i64,
    ) -> Result<()> {
        let graph_iri = self.resolve(graph)?;
        if self.committed_graph_id(&graph_iri)? != Some(graph) {
            return Ok(());
        }
        let insert_event =
            |event_type: &str, subject: Option<&str>, payload: &serde_json::Value| {
                self.conn.execute(
                    "INSERT INTO graph_events (type, ts, subject, group_id, tx_id, payload, graph) \
                 VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6)",
                    params![
                        event_type,
                        timestamp,
                        subject,
                        tx_id,
                        payload.to_string(),
                        graph_iri
                    ],
                )
            };
        if let Some(name) = source.and_then(|s| s.strip_prefix("episode:")) {
            let payload = json!({ "name": name, "facts": asserts.len() + retracts.len() });
            insert_event("episode.ingested", None, &payload)?;
        }
        // "Added" is judged within THIS graph: an item's first facts in the
        // board are an addition to the board even if ROOT already knew the IRI.
        let mut seen: HashSet<i64> = HashSet::new();
        for d in asserts.iter().chain(retracts.iter()) {
            if !seen.insert(d.entity) {
                continue;
            }
            let preexisting = self
                .conn
                .query_row(
                    "SELECT 1 FROM facts WHERE g = ?1 AND e = ?2 AND tx < ?3 LIMIT 1",
                    params![graph, d.entity, tx_id],
                    |_| Ok(true),
                )
                .optional()?
                .is_some();
            let event_type = if preexisting {
                "entity.updated"
            } else {
                "entity.added"
            };
            insert_event(event_type, Some(&self.resolve(d.entity)?), &json!({}))?;
        }
        Ok(())
    }

    /// Highest offset in ONE graph's log (`None` = ROOT's `events`), 0 when
    /// empty. A reader's lag is measured against its own log, so a ROOT
    /// consumer's lag never moves with board writes it does not receive.
    ///
    /// # Errors
    /// [`crate::Error::Sqlite`] if the read fails.
    pub fn latest_event_offset_in(&self, graph: Option<&str>) -> Result<i64> {
        let Some(g) = graph else {
            return self.latest_event_offset();
        };
        if !self.has_graph_events()? {
            return Ok(0);
        }
        Ok(self.conn.query_row(
            "SELECT COALESCE(MAX(\"offset\"), 0) FROM graph_events WHERE graph = ?1",
            params![g],
            |r| r.get(0),
        )?)
    }

    /// Whether this store has the named-graph event log. A read-only handle
    /// on a store no writer has migrated yet does not.
    pub(super) fn has_graph_events(&self) -> Result<bool> {
        Ok(self
            .conn
            .prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'graph_events'")?
            .exists([])?)
    }

    /// [`Self::consumer_committed`] in one graph's log (`None` = ROOT). A
    /// graph consumer's cursor is an offset in `graph_events`, so it lives in
    /// `graph_consumers`, never in `consumers` beside ROOT offsets.
    ///
    /// # Errors
    /// [`crate::Error::Sqlite`] if the read fails.
    pub fn consumer_committed_in(&self, consumer_id: &str, graph: Option<&str>) -> Result<i64> {
        let Some(g) = graph else {
            return self.consumer_committed(consumer_id);
        };
        if !self.has_graph_events()? {
            return Ok(0);
        }
        Ok(self
            .conn
            .query_row(
                "SELECT committed_offset FROM graph_consumers WHERE consumer_id = ?1 AND graph = ?2",
                params![consumer_id, g],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0))
    }

    /// [`Self::commit_consumer`] in one graph's log (`None` = ROOT).
    ///
    /// # Errors
    /// [`crate::Error::Sqlite`] if the write fails.
    pub fn commit_consumer_in(
        &self,
        consumer_id: &str,
        graph: Option<&str>,
        offset: i64,
        now: &str,
    ) -> Result<()> {
        let Some(g) = graph else {
            return self.commit_consumer(consumer_id, offset, now);
        };
        self.conn.execute(
            "INSERT INTO graph_consumers (consumer_id, graph, committed_offset, updated_at) \
             VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT(consumer_id, graph) DO UPDATE SET \
                 committed_offset = excluded.committed_offset, \
                 updated_at = excluded.updated_at",
            params![consumer_id, g, offset, now],
        )?;
        Ok(())
    }
}
