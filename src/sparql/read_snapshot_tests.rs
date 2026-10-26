//! Deterministic concurrent-writer and transaction-ownership controls.

use super::*;
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn insert(store: &mut Store, subject: &str, status: &str) {
    crate::tool_knot(
        store,
        &json!({
            "turtle": format!("<{subject}> <urn:snapshot:status> \"{status}\" ."),
            "timestamp": "2026-10-20T00:00:00Z"
        }),
    )
    .unwrap();
}

#[test]
fn union_aggregates_keep_one_snapshot_while_another_connection_commits() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("snapshot.db");
    let path = db.to_str().unwrap();
    let mut reader = Store::open(path).unwrap();
    insert(&mut reader, "urn:snapshot:a", "open");
    insert(&mut reader, "urn:snapshot:b", "closed");
    let mut writer = Store::open(path).unwrap();
    let fired = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&fired);
    read_snapshot::set_after_union_left(Some(Box::new(move || {
        insert(&mut writer, "urn:snapshot:late", "open");
        signal.store(true, Ordering::SeqCst);
    })));
    let result = query(
        &reader,
        r#"
        SELECT ?arm ?status ?n WHERE {
            { { SELECT (COUNT(?s) AS ?n) WHERE {
                ?s <urn:snapshot:status> ?value
            } } BIND("total" AS ?arm) }
            UNION
            { { SELECT ?status (COUNT(?s) AS ?n) WHERE {
                ?s <urn:snapshot:status> ?status
            } GROUP BY ?status } BIND("group" AS ?arm) }
        }
    "#,
    );
    read_snapshot::set_after_union_left(None);
    let result = result.unwrap();
    assert!(fired.load(Ordering::SeqCst), "the writer control must run");
    let mut total = None;
    let mut groups = 0;
    for row in result.rows() {
        let Some(Value::Int(n)) = row.get("n") else {
            panic!("integer count required")
        };
        match row.get("arm") {
            Some(Value::Str(arm)) if arm == "total" => {
                assert!(total.replace(*n).is_none());
            }
            Some(Value::Str(arm)) if arm == "group" => {
                groups += n;
            }
            _ => panic!("unknown aggregate arm"),
        }
    }
    assert_eq!(total, Some(2));
    assert_eq!(
        groups, 2,
        "the later group must not see the committed writer"
    );
    assert!(reader.conn.is_autocommit(), "the owned snapshot must end");
    let fresh = query(
        &reader,
        "SELECT (COUNT(?s) AS ?n) WHERE { ?s <urn:snapshot:status> ?v }",
    )
    .unwrap();
    assert_eq!(fresh.rows()[0].get("n"), Some(&Value::Int(3)));
}

#[test]
fn a_query_never_commits_or_rolls_back_its_callers_transaction() {
    let store = Store::open_in_memory().unwrap();
    store.conn.execute_batch("BEGIN").unwrap();
    query(&store, "SELECT ?x WHERE { VALUES ?x { 1 } }").unwrap();
    assert!(!store.conn.is_autocommit());
    assert!(query(&store, "not valid SPARQL").is_err());
    assert!(
        !store.conn.is_autocommit(),
        "a failed nested query leaves ownership alone"
    );
    store.conn.execute_batch("ROLLBACK").unwrap();
    assert!(store.conn.is_autocommit());
}

#[test]
fn a_failed_owned_query_releases_its_read_transaction() {
    let store = Store::open_in_memory().unwrap();
    assert!(query(&store, "not valid SPARQL").is_err());
    assert!(store.conn.is_autocommit());
    let result = query(&store, "SELECT ?x WHERE { VALUES ?x { 1 } }").unwrap();
    assert_eq!(result.rows()[0].get("x"), Some(&Value::Int(1)));
}

#[test]
fn a_timeout_releases_the_owned_snapshot_and_keeps_its_error() {
    let mut store = Store::open_in_memory().unwrap();
    insert(&mut store, "urn:snapshot:a", "open");
    let ctx = TemporalContext {
        deadline: Some(crate::time::Deadline::after_millis(0)),
        ..Default::default()
    };
    let error = query_temporal(
        &store,
        "SELECT ?s WHERE { ?s <urn:snapshot:status> ?v }",
        &ctx,
    )
    .unwrap_err();
    assert!(matches!(error, crate::error::Error::QueryTimeout { .. }));
    assert!(store.conn.is_autocommit());
    assert_eq!(
        query(&store, "SELECT ?s WHERE { ?s <urn:snapshot:status> ?v }")
            .unwrap()
            .rows()
            .len(),
        1
    );
}
