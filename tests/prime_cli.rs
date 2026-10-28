//! `quipu prime`: static primer always, live summary read-only and bounded.
#![cfg(feature = "shacl")]

use std::path::Path;
use std::process::{Command, Output};

fn prime(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quipu"))
        .current_dir(dir)
        .arg("prime")
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn static_primer_needs_no_store() {
    let dir = tempfile::tempdir().unwrap();
    let out = prime(dir.path(), &["--static"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with("# Quipu agent primer"));
    assert!(!text.contains("## Live store"));
    assert!(
        !dir.path().join(".bobbin").exists(),
        "--static touched the store path"
    );
}

#[test]
fn missing_store_is_named_and_never_created() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("nested").join("q.db");
    let out = prime(dir.path(), &["--db", db.to_str().unwrap()]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("No store at"));
    assert!(!db.parent().unwrap().exists());
}

#[test]
fn live_summary_is_read_only_and_fast() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("live.db");
    {
        let store = quipu::Store::open(db.to_str().unwrap()).unwrap();
        store
            .graph_create_recorded("urn:fixture:ops", "2026-10-27T00:00:00Z", Some("fixture"))
            .unwrap();
    }
    let before = std::fs::read(&db).unwrap();
    let out = prime(dir.path(), &["--db", db.to_str().unwrap(), "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["store"]["status"], "ok");
    // A fresh store also registers its own metadata graph.
    let names = v["store"]["graphs"]["names"].as_array().unwrap();
    assert!(names.iter().any(|n| n == "urn:fixture:ops"), "{names:?}");
    assert!(v["primer"].as_str().unwrap().contains("## MCP tools"));
    assert_eq!(
        std::fs::read(&db).unwrap(),
        before,
        "prime modified the store"
    );
    // The summary's own clock, against its 400 ms budget. Process start-up
    // is not prime's to bound and varies by host (over 1 s on some Macs).
    let elapsed_ms = v["store"]["elapsed_ms"].as_u64().unwrap();
    assert!(elapsed_ms <= 400, "summary took {elapsed_ms} ms");
}

#[test]
fn section_and_brief_select_text() {
    let dir = tempfile::tempdir().unwrap();
    let out = prime(dir.path(), &["--static", "--section", "writing"]);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with("## Writing safely"));
    let bad = prime(dir.path(), &["--static", "--section", "nope"]);
    assert_eq!(bad.status.code(), Some(2));
    let brief = prime(dir.path(), &["--static", "--brief"]);
    let text = String::from_utf8(brief.stdout).unwrap();
    assert!(text.contains("## Query first") && !text.contains("## Core commands"));
}

#[test]
fn a_garbage_db_file_is_reported_unreadable() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("garbage.db");
    std::fs::write(&db, b"not a database\n".repeat(500)).unwrap();
    let out = prime(dir.path(), &["--db", db.to_str().unwrap(), "--json"]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["store"]["status"], "unreadable", "{v}");
    let text = prime(dir.path(), &["--db", db.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&text.stdout).contains("could not be read"));
}
