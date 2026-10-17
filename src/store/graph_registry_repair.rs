//! Once-only repair for legacy writers that omitted graph registry rows.
use super::Store;
use crate::error::Result;
use rusqlite::Connection;

impl Store {
    /// Repair only local current data-bearing graphs, atomically with a marker.
    /// Read-only opens never call this migration. Recovered creation time is an
    /// explicit unknown epoch, not a claim about the original creator or date.
    pub(super) fn repair_data_graph_registry_once(conn: &Connection) -> Result<()> {
        conn.execute_batch("SAVEPOINT quipu_graph_registry_repair")?;
        let result = (|| -> Result<()> {
            conn.execute_batch("CREATE TABLE IF NOT EXISTS main.quipu_schema_repairs (name TEXT PRIMARY KEY NOT NULL)")?;
            let done = conn
                .prepare(
                    "SELECT 1 FROM main.quipu_schema_repairs WHERE name='data_graph_registry_v1'",
                )?
                .exists([])?;
            if !done {
                conn.execute_batch(
                    "INSERT INTO main.graphs (g,class,parent_branch,created_at)
                     SELECT current.g,'committed',NULL,'1970-01-01T00:00:00Z'
                     FROM (SELECT DISTINCT g FROM main.facts
                           WHERE op=1 AND valid_to IS NULL AND g!=0) AS current
                     LEFT JOIN main.graphs existing ON existing.g=current.g
                     WHERE existing.g IS NULL;
                     INSERT INTO main.quipu_schema_repairs(name) VALUES ('data_graph_registry_v1');"
                )?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            conn.execute_batch(
                "ROLLBACK TO quipu_graph_registry_repair; RELEASE quipu_graph_registry_repair",
            )?;
            return Err(error);
        }
        conn.execute_batch("RELEASE quipu_graph_registry_repair")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Op, Value, store::Datum};
    fn datum(store: &Store, value: &str, op: Op) -> Datum {
        Datum {
            entity: store.intern("urn:repair:entity").unwrap(),
            attribute: store.intern("urn:repair:attribute").unwrap(),
            value: Value::Str(value.into()),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
            op,
        }
    }
    fn reset_marker(store: &Store) {
        store
            .conn
            .execute("DELETE FROM quipu_schema_repairs", [])
            .unwrap();
    }
    #[test]
    fn repair_is_once_only_current_and_preserves_existing_graphs() {
        let mut store = Store::open_in_memory().unwrap();
        reset_marker(&store);
        let current = store.intern("urn:repair:current").unwrap();
        let historical = store.intern("urn:repair:historical").unwrap();
        store.intern("urn:repair:empty").unwrap();
        let parent = store.graph_create("urn:repair:parent").unwrap();
        let overlay = store.overlay_create("urn:repair:overlay", parent).unwrap();
        store
            .conn
            .execute(
                "UPDATE graphs SET created_at='fixture-original' WHERE g=?1",
                [overlay],
            )
            .unwrap();
        let d = datum(&store, "current", Op::Assert);
        store
            .transact_to_graph(&[d], "2026-01-01T00:00:00Z", None, None, current)
            .unwrap();
        let d = datum(&store, "old", Op::Assert);
        store
            .transact_to_graph(&[d], "2026-01-01T00:00:00Z", None, None, historical)
            .unwrap();
        let d = datum(&store, "old", Op::Retract);
        store
            .transact_to_graph(&[d], "2026-01-02T00:00:00Z", None, None, historical)
            .unwrap();
        let entity = store.lookup("urn:repair:entity").unwrap().unwrap();
        let before = format!(
            "{:?}{:?}",
            store.entity_history_in_graph(entity, current).unwrap(),
            store.entity_history_in_graph(entity, historical).unwrap()
        );
        Store::repair_data_graph_registry_once(&store.conn).unwrap();
        assert_eq!(
            store.registered_graph_id("urn:repair:current").unwrap(),
            Some(current)
        );
        assert!(
            store
                .registered_graph_id("urn:repair:historical")
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .registered_graph_id("urn:repair:empty")
                .unwrap()
                .is_none()
        );
        let row: (String, Option<i64>, String) = store
            .conn
            .query_row(
                "SELECT class,parent_branch,created_at FROM graphs WHERE g=?1",
                [overlay],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            row,
            ("overlay".into(), Some(parent), "fixture-original".into())
        );
        assert_eq!(
            before,
            format!(
                "{:?}{:?}",
                store.entity_history_in_graph(entity, current).unwrap(),
                store.entity_history_in_graph(entity, historical).unwrap()
            )
        );
        let root_count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM graphs WHERE g=0", [], |r| r.get(0))
            .unwrap();
        assert_eq!(root_count, 1);
        // A sentinel prevents a second scan; this is deliberate fixture damage.
        store
            .conn
            .execute("DELETE FROM graphs WHERE g=?1", [current])
            .unwrap();
        Store::repair_data_graph_registry_once(&store.conn).unwrap();
        assert!(
            store
                .registered_graph_id("urn:repair:current")
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn marker_failure_rolls_back_recovered_registry_rows() {
        let mut store = Store::open_in_memory().unwrap();
        reset_marker(&store);
        let g = store.intern("urn:repair:rollback").unwrap();
        let d = datum(&store, "kept", Op::Assert);
        store
            .transact_to_graph(&[d], "2026-01-01T00:00:00Z", None, None, g)
            .unwrap();
        store.conn.execute_batch("CREATE TEMP TRIGGER fixture_marker_refusal BEFORE INSERT ON quipu_schema_repairs BEGIN SELECT RAISE(ABORT,'fixture marker refusal'); END;").unwrap();
        assert!(Store::repair_data_graph_registry_once(&store.conn).is_err());
        assert!(
            store
                .registered_graph_id("urn:repair:rollback")
                .unwrap()
                .is_none()
        );
        let markers: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM quipu_schema_repairs", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(markers, 0);
        store
            .conn
            .execute_batch("DROP TRIGGER fixture_marker_refusal")
            .unwrap();
        Store::repair_data_graph_registry_once(&store.conn).unwrap();
        assert_eq!(
            store.registered_graph_id("urn:repair:rollback").unwrap(),
            Some(g)
        );
    }
    #[test]
    fn batch_registry_failure_rolls_back_earlier_graphs_and_transactions() {
        let mut store = Store::open_in_memory().unwrap();
        let first = store.intern("urn:batch:first").unwrap();
        let second = store.intern("urn:batch:second").unwrap();
        store.conn.execute_batch(&format!("CREATE TEMP TRIGGER fixture_second_registry BEFORE INSERT ON graphs WHEN NEW.g={second} BEGIN SELECT RAISE(ABORT,'fixture second registry refusal'); END;")).unwrap();
        let batches = vec![
            (first, vec![datum(&store, "first", Op::Assert)]),
            (second, vec![datum(&store, "second", Op::Assert)]),
        ];
        let before: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert!(
            store
                .transact_graph_batches(&batches, "2026-01-01T00:00:00Z", None, None)
                .is_err()
        );
        for (g, _) in batches {
            assert!(store.current_facts_in_graph(g).unwrap().is_empty());
            assert!(store.graph_class(g).unwrap().is_none());
        }
        let after: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(before, after);
    }
}
