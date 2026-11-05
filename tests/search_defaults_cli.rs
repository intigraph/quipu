//! A fresh installation can ingest and search without model configuration.
#![cfg(feature = "shacl")]

use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::{Command, Output};

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quipu"))
        .current_dir(dir)
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .args(args)
        .output()
        .unwrap()
}

fn success(out: &Output) -> serde_json::Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn fresh_ingest_search_and_doctor_need_no_model() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("sample.ttl"),
        r#"<https://example.org/library> <http://www.w3.org/2000/01/rdf-schema#label> "Clockwork Library" ."#,
    ).unwrap();
    let digest = format!(
        "{:x}",
        Sha256::digest(std::fs::read(dir.path().join("sample.ttl")).unwrap())
    );
    let loaded = run(
        dir.path(),
        &[
            "ingest",
            "sample.ttl",
            "--db",
            "fresh.db",
            "--graph",
            "https://example.org/sample",
            "--timestamp",
            "2026-01-01T00:00:00Z",
            "--declare-count",
            "1",
            "--declare-sha256",
            &digest,
        ],
    );
    assert!(
        loaded.status.success(),
        "{}",
        String::from_utf8_lossy(&loaded.stderr)
    );
    let result = success(&run(
        dir.path(),
        &["search", "Clockwork", "--db", "fresh.db"],
    ));
    assert_eq!(result["count"], 1);
    assert_eq!(result["ranking"], "keyword");
    assert_eq!(result["mode_reason"], "no_embedding_provider");
    assert!(result["results"][0].get("similarity").is_none());
    let doctor = success(&run(dir.path(), &["doctor", "search", "--db", "fresh.db"]));
    assert_eq!(doctor["keyword_ready"], true);
    assert_eq!(doctor["semantic_ready"], false);
    let explicit = run(
        dir.path(),
        &[
            "search",
            "Clockwork",
            "--mode",
            "semantic",
            "--graph",
            "urn:quipu:graph:root",
            "--db",
            "fresh.db",
        ],
    );
    assert!(!explicit.status.success());
    assert!(
        String::from_utf8_lossy(&explicit.stderr).contains("no embedding provider"),
        "{}",
        String::from_utf8_lossy(&explicit.stderr)
    );
}

#[test]
fn old_unindexed_store_requires_bounded_backfill() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("old.db");
    let mut store = quipu::Store::open(db.to_str().unwrap()).unwrap();
    quipu::ingest_rdf(&mut store,
        r#"<https://example.org/legacy> <http://www.w3.org/2000/01/rdf-schema#label> "Clockwork" ."#.as_bytes(),
        oxrdfio::RdfFormat::Turtle, None, "2026-01-01T00:00:00Z", None, None,
    ).unwrap();
    drop(store);
    let out = run(dir.path(), &["search", "Clockwork", "--db", "old.db"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("backfill"));
    let doctor = success(&run(dir.path(), &["doctor", "search", "--db", "old.db"]));
    assert_eq!(doctor["keyword_ready"], false);
}
