use super::*;
use crate::types::{Op, Value};

fn datum(store: &Store, subject: &str, value: i64, timestamp: &str) -> Datum {
    Datum {
        entity: store.intern(subject).unwrap(),
        attribute: store.intern("urn:fixture:recorded-tx").unwrap(),
        value: Value::Int(value),
        valid_from: timestamp.into(),
        valid_to: None,
        op: Op::Assert,
    }
}

#[test]
fn mapping_atomic_two_graphs_have_one_tx_and_replay_has_no_new_tx() {
    let mut store = Store::open_in_memory().unwrap();
    let mapping = store.graph_create("urn:fixture:mapping").unwrap();
    let operations = store.graph_create("urn:fixture:operations").unwrap();
    let tx = store
        .mapping_atomic(
            "urn:fixture:actor",
            |_| Ok(Preparation::Write(())),
            |store, (), timestamp, tx| {
                Ok((
                    tx,
                    vec![
                        (
                            mapping,
                            vec![datum(store, "urn:fixture:definition", tx, timestamp)],
                        ),
                        (
                            operations,
                            vec![datum(store, "urn:fixture:record", tx, timestamp)],
                        ),
                    ],
                ))
            },
        )
        .unwrap();
    for graph in [mapping, operations] {
        let facts = store.current_facts_in_graph(graph).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].tx, tx);
        assert_eq!(facts[0].value, Value::Int(tx));
    }
    assert_eq!(store.latest_tx_id().unwrap(), tx);
    let replay = store
        .mapping_atomic::<(), i64>(
            "urn:fixture:actor",
            |_| Ok(Preparation::Replay(tx)),
            |_, (), _, _| panic!("replay must not enter writer"),
        )
        .unwrap();
    assert_eq!(replay, tx);
    assert_eq!(store.latest_tx_id().unwrap(), tx);
}

#[test]
fn mapping_atomic_late_graph_failure_restores_registry_facts_and_events() {
    let mut store = Store::open_in_memory().unwrap();
    let mapping = store.graph_create("urn:fixture:mapping").unwrap();
    let operations = store.graph_create("urn:fixture:operations").unwrap();
    store.conn.execute_batch("CREATE TABLE mapping_atomic_fixture(value TEXT); INSERT INTO mapping_atomic_fixture VALUES('before')").unwrap();
    store.conn.execute_batch(&format!("CREATE TRIGGER mapping_atomic_failure BEFORE INSERT ON facts WHEN NEW.g={operations} BEGIN SELECT RAISE(ABORT, 'late graph fixture'); END")).unwrap();
    let before = store.latest_tx_id().unwrap();
    let result = store.mapping_atomic(
        "urn:fixture:actor",
        |_| Ok(Preparation::Write(())),
        |store, (), timestamp, tx| {
            store
                .conn
                .execute("UPDATE mapping_atomic_fixture SET value='after'", [])?;
            Ok((
                (),
                vec![
                    (
                        mapping,
                        vec![datum(store, "urn:fixture:definition", tx, timestamp)],
                    ),
                    (
                        operations,
                        vec![datum(store, "urn:fixture:record", tx, timestamp)],
                    ),
                ],
            ))
        },
    );
    assert!(result.is_err());
    assert!(store.conn.is_autocommit());
    assert!(!store.write_in_progress());
    assert_eq!(store.latest_tx_id().unwrap(), before);
    assert!(store.current_facts_in_graph(mapping).unwrap().is_empty());
    assert!(store.current_facts_in_graph(operations).unwrap().is_empty());
    let value: String = store
        .conn
        .query_row("SELECT value FROM mapping_atomic_fixture", [], |r| r.get(0))
        .unwrap();
    assert_eq!(value, "before");
    let events: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM graph_events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(events, 0);
}

#[test]
fn mapping_atomic_panic_restores_writer_and_nested_use_refuses() {
    let mut store = Store::open_in_memory().unwrap();
    let before = store.latest_tx_id().unwrap();
    let panic = catch_unwind(AssertUnwindSafe(|| {
        let _ = store.mapping_atomic::<(), ()>(
            "urn:fixture:actor",
            |_| Ok(Preparation::Write(())),
            |store, (), _, _| {
                store.intern("urn:fixture:abandoned")?;
                panic!("fixture panic")
            },
        );
    }));
    assert!(panic.is_err());
    assert!(store.conn.is_autocommit());
    assert!(!store.write_in_progress());
    assert!(store.lookup("urn:fixture:abandoned").unwrap().is_none());
    assert_eq!(store.latest_tx_id().unwrap(), before);
    store.conn.execute_batch("SAVEPOINT unrelated").unwrap();
    let nested = store.mapping_atomic::<(), ()>(
        "urn:fixture:actor",
        |_| Ok(Preparation::Replay(())),
        |_, (), _, _| unreachable!(),
    );
    assert!(nested.is_err());
    store
        .conn
        .execute_batch("ROLLBACK TO unrelated; RELEASE unrelated")
        .unwrap();
}

#[test]
fn mapping_atomic_cross_graph_claim_alias_refuses_before_any_commit() {
    let mut store = Store::open_in_memory().unwrap();
    let mapping = store.graph_create("urn:fixture:mapping").unwrap();
    let operations = store.graph_create("urn:fixture:operations").unwrap();
    let before = store.latest_tx_id().unwrap();
    let result = store.mapping_atomic(
        "urn:fixture:actor",
        |_| Ok(Preparation::Write(())),
        |store, (), timestamp, tx| {
            let same = datum(store, "urn:fixture:shared", tx, timestamp);
            Ok((
                (),
                vec![(mapping, vec![same.clone()]), (operations, vec![same])],
            ))
        },
    );
    assert!(result.is_err());
    assert_eq!(store.latest_tx_id().unwrap(), before);
    assert!(store.current_facts_in_graph(mapping).unwrap().is_empty());
    assert!(store.current_facts_in_graph(operations).unwrap().is_empty());
}

#[test]
fn mapping_atomic_retirement_closes_validity_without_negative_history_rows() {
    let mut store = Store::open_in_memory().unwrap();
    let graph = store.graph_create("urn:fixture:mapping").unwrap();
    let old = datum(&store, "urn:fixture:definition", 17, "2026-01-01T00:00:00Z");
    let old_tx = store
        .transact_to_graph(
            std::slice::from_ref(&old),
            &old.valid_from,
            None,
            None,
            graph,
        )
        .unwrap();
    let close_tx = store
        .mapping_atomic(
            "urn:fixture:actor",
            |_| Ok(Preparation::Write(())),
            |_, (), timestamp, tx| {
                let mut closed = old.clone();
                closed.op = Op::Retract;
                closed.valid_from = timestamp.into();
                Ok((tx, vec![(graph, vec![closed])]))
            },
        )
        .unwrap();
    assert!(store.current_facts_in_graph(graph).unwrap().is_empty());
    let history: (i64, String, i64, i64) = store
        .conn
        .query_row(
            "SELECT tx,valid_to,op,retracted_tx FROM facts WHERE g=?1",
            [graph],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(history.0, old_tx);
    assert!(!history.1.is_empty());
    assert_eq!(history.2, Op::Assert as i64);
    assert_eq!(history.3, close_tx);
    let count: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM facts WHERE g=?1", [graph], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
}
