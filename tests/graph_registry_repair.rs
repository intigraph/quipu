//! Legacy graph registry and batch atomicity controls.
use quipu::{Op, Store, Value, store::Datum};

#[test]
fn registry_followup_open_never_repairs_and_explicit_maintenance_does() {
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
    assert!(
        reopened
            .registered_graph_id("urn:legacy:graph")
            .unwrap()
            .is_none()
    );
    assert_eq!(reopened.unregistered_data_graph_count().unwrap(), 1);
    assert_eq!(
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='quipu_schema_repairs'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    drop(reopened);
    for flags in [vec![], vec!["--dry-run"]] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_quipu"))
            .arg("graph-registry-repair")
            .args(flags)
            .arg("--db")
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["unregistered_graphs"], 1);
        assert_eq!(value["dry_run"], true);
    }
    let reopened = Store::open(path.to_str().unwrap()).unwrap();
    assert!(
        reopened
            .registered_graph_id("urn:legacy:graph")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='quipu_schema_repairs'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    drop(reopened);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_quipu"))
        .args(["graph-registry-repair", "--execute", "--db"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reopened = Store::open(path.to_str().unwrap()).unwrap();
    assert_eq!(reopened.unregistered_data_graph_count().unwrap(), 0);
    reopened.repair_data_graph_registry().unwrap();
    let registered = reopened.registered_graph_id("urn:legacy:graph").unwrap();
    println!("LEGACY_EXPLICIT_REPAIR registered={}", registered.is_some());
    assert_eq!(registered, Some(g));
    assert_eq!(reopened.entity_facts_in_graph(e, g).unwrap().len(), 1);
}
