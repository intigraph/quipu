//! Legacy graph registry and batch atomicity controls.
use quipu::{Op, Store, Value, store::Datum};

#[test]
fn registry_followup_writable_open_repairs_legacy_current_graph() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    let mut store = Store::open(path.to_str().unwrap()).unwrap();
    let g = store.graph_create("urn:legacy:graph").unwrap();
    let e = store.intern("urn:legacy:entity").unwrap();
    let a = store.intern("urn:legacy:attribute").unwrap();
    store
        .transact_to_graph(
            &[Datum {
                entity: e,
                attribute: a,
                value: Value::Str("kept".into()),
                valid_from: "2026-01-01T00:00:00Z".into(),
                valid_to: None,
                op: Op::Assert,
            }],
            "2026-01-01T00:00:00Z",
            None,
            None,
            g,
        )
        .unwrap();
    drop(store);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("DROP TABLE IF EXISTS quipu_schema_repairs")
        .unwrap();
    conn.execute("DELETE FROM graphs WHERE g=?1", [g]).unwrap();
    drop(conn);
    let reader = Store::open_read_only(path.to_str().unwrap()).unwrap();
    assert!(
        reader
            .registered_graph_id("urn:legacy:graph")
            .unwrap()
            .is_none()
    );
    drop(reader);
    let reopened = Store::open(path.to_str().unwrap()).unwrap();
    let registered = reopened.registered_graph_id("urn:legacy:graph").unwrap();
    println!("LEGACY_REOPEN registered={}", registered.is_some());
    assert_eq!(registered, Some(g));
    assert_eq!(reopened.entity_facts_in_graph(e, g).unwrap().len(), 1);
}
