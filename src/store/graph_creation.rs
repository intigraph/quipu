//! Creation provenance for explicitly registered committed graphs.
use super::Store;
use crate::error::Result;
use rusqlite::{OptionalExtension, params};
use serde_json::{Value as JsonValue, json};

impl Store {
    /// Register and record creation atomically. Repeats retain the original
    /// timestamp and declared actor; an interned term is not a registered graph.
    pub fn graph_create_recorded(
        &self,
        graph: &str,
        timestamp: &str,
        actor: Option<&str>,
    ) -> Result<JsonValue> {
        self.ensure_ownership_writable()?;
        self.conn
            .execute_batch("SAVEPOINT quipu_graph_create_recorded")?;
        let result = (|| {
            let existed = self.registered_graph_id(graph)?.is_some();
            let g = self.graph_create(graph)?;
            if !existed {
                self.conn.execute(
                    "UPDATE graphs SET created_at = ?2 WHERE g = ?1",
                    params![g, timestamp],
                )?;
                self.conn.execute(
                    "INSERT INTO events (type, ts, subject, group_id, tx_id, payload) VALUES ('graph.created', ?1, ?2, NULL, ?3, ?4)",
                    params![timestamp, graph, self.latest_tx_id()?, json!({"graph":graph,"actor":actor,"timestamp":timestamp}).to_string()],
                )?;
            }
            let provenance = self.graph_creation_provenance(g)?;
            Ok(json!({"graph":graph,"g":g,"created":!existed,"creation":provenance}))
        })();
        match result {
            Ok(value) => {
                crate::store::release::release(&self.conn, "quipu_graph_create_recorded")?;
                Ok(value)
            }
            Err(error) => {
                let _ = crate::store::release::rollback_release(
                    &self.conn,
                    "quipu_graph_create_recorded",
                );
                Err(error)
            }
        }
    }

    /// Old/imported graphs have a registry timestamp but no declared actor.
    /// Never attribute a repeat caller as the creator of an existing graph.
    pub fn graph_creation_provenance(&self, g: i64) -> Result<JsonValue> {
        let timestamp: String =
            self.conn
                .query_row("SELECT created_at FROM graphs WHERE g = ?1", [g], |r| {
                    r.get(0)
                })?;
        let graph = self.graph_display_name(g);
        let payload: Option<String> = self.conn.query_row(
            "SELECT payload FROM events WHERE type = 'graph.created' AND subject = ?1 ORDER BY offset DESC LIMIT 1",
            [&graph], |r| r.get(0),
        ).optional()?;
        let actor = payload
            .and_then(|p| serde_json::from_str::<JsonValue>(&p).ok())
            .and_then(|p| p.get("actor").cloned())
            .unwrap_or(JsonValue::Null);
        Ok(json!({"timestamp":timestamp,"actor":actor}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registration_is_atomic_idempotent_and_distinct_from_interning() {
        let s = Store::open_in_memory().unwrap();
        s.intern("urn:test:interned").unwrap();
        let first = s
            .graph_create_recorded(
                "urn:test:interned",
                "2026-10-14T00:00:00Z",
                Some("urn:actor:first"),
            )
            .unwrap();
        assert_eq!(first["created"], true);
        let repeat = s
            .graph_create_recorded(
                "urn:test:interned",
                "2026-10-15T00:00:00Z",
                Some("urn:actor:second"),
            )
            .unwrap();
        assert_eq!(repeat["created"], false);
        assert_eq!(repeat["creation"], first["creation"]);
        let count: i64 = s
            .conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE type='graph.created'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn a_failed_creation_record_rolls_back_registration_and_interning() {
        let s = Store::open_in_memory().unwrap();
        s.conn.execute_batch("CREATE TEMP TRIGGER refuse_creation BEFORE INSERT ON events WHEN NEW.type='graph.created' BEGIN SELECT RAISE(ABORT, 'fixture refuses event'); END;").unwrap();
        assert!(
            s.graph_create_recorded("urn:test:failed", "2026-10-14T00:00:00Z", None)
                .is_err()
        );
        assert_eq!(s.registered_graph_id("urn:test:failed").unwrap(), None);
        assert_eq!(s.lookup("urn:test:failed").unwrap(), None);
        s.conn
            .execute_batch("DROP TRIGGER refuse_creation")
            .unwrap();
        assert_eq!(
            s.graph_create_recorded("urn:test:control", "2026-10-14T00:00:00Z", None)
                .unwrap()["created"],
            true
        );
    }

    #[test]
    fn historical_creation_is_not_attributed_to_a_repeat_caller() {
        let s = Store::open_in_memory().unwrap();
        let g = s.graph_create("urn:test:legacy").unwrap();
        let before = s.graph_creation_provenance(g).unwrap();
        let result = s
            .graph_create_recorded(
                "urn:test:legacy",
                "2026-10-14T00:00:00Z",
                Some("urn:actor:later"),
            )
            .unwrap();
        assert_eq!(result["created"], false);
        assert_eq!(result["creation"], before);
        assert!(result["creation"]["actor"].is_null());
    }
}
