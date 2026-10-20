//! Point metadata CLI: existing files only, without writer initialization.
#![cfg(feature = "shacl")]

use std::path::Path;
use std::process::{Command, Output};

fn run(dir: &Path, db: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quipu"))
        .current_dir(dir)
        .args(["graph", "list", "--db"])
        .arg(db)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn selector_reads_existing_store_and_never_creates_or_migrates_one() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("private.db");
    {
        let store = quipu::Store::open(db.to_str().unwrap()).unwrap();
        store
            .graph_create_recorded(
                "urn:fixture:target",
                "2026-10-20T00:00:00Z",
                Some("fixture"),
            )
            .unwrap();
        store.graph_create("urn:fixture:other").unwrap();
    }
    let bytes = std::fs::read(&db).unwrap();
    let result = run(
        dir.path(),
        &db,
        &["--graph", "urn:fixture:target", "--json"],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["count"], 1);
    assert_eq!(value["graphs"][0]["creation"]["actor"], "fixture");
    assert!(
        !run(
            dir.path(),
            &db,
            &["--graph", "urn:fixture:target", "--frozen"]
        )
        .status
        .success()
    );
    assert!(!run(dir.path(), &db, &["--graph"]).status.success());
    assert_eq!(std::fs::read(&db).unwrap(), bytes);

    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch("DROP INDEX idx_events_type_subject_offset")
        .unwrap();
    drop(conn);
    let old_bytes = std::fs::read(&db).unwrap();
    assert!(
        !run(dir.path(), &db, &["--graph", "urn:fixture:target"])
            .status
            .success()
    );
    assert_eq!(std::fs::read(&db).unwrap(), old_bytes);

    let absent = dir.path().join("absent.db");
    assert!(
        !run(dir.path(), &absent, &["--graph", "urn:fixture:target"])
            .status
            .success()
    );
    assert!(!absent.exists());
}
