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
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env_remove("ORT_DYLIB_PATH")
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
    assert_eq!(doctor["onnx_runtime"]["available"], false);
    assert!(doctor["onnx_runtime"]["linkage"].is_null());
    assert!(doctor["onnx_runtime"]["path"].is_null());
    assert_eq!(
        doctor["onnx_runtime"]["state"],
        if cfg!(feature = "onnx") {
            "not_initialized"
        } else {
            "not_compiled"
        }
    );
    assert!(
        !dir.path().join("cache").exists(),
        "doctor must not stage a runtime without a model"
    );
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
fn old_unindexed_store_requires_explicit_opt_in_and_bounded_backfill() {
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
    let doctor = success(&run(dir.path(), &["doctor", "search", "--db", "old.db"]));
    assert_eq!(doctor["keyword_ready"], false);
    let conn = rusqlite::Connection::open(&db).unwrap();
    let before: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name LIKE 'lexical_%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        before, 0,
        "opening an existing store must not install FTS tables/triggers"
    );
    std::fs::create_dir_all(dir.path().join(".bobbin")).unwrap();
    std::fs::write(
        dir.path().join(".bobbin/config.toml"),
        "[quipu.search]\nkeyword = true\n",
    )
    .unwrap();
    let opted_in = run(dir.path(), &["search", "Clockwork", "--db", "old.db"]);
    assert!(!opted_in.status.success());
    assert!(String::from_utf8_lossy(&opted_in.stderr).contains("backfill"));
    let after: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name LIKE 'lexical_%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        after > 0,
        "explicit opt-in installs the bounded-backfill index"
    );
    let backfill = success(&run(
        dir.path(),
        &[
            "search-index",
            "backfill",
            "--batch-size",
            "500",
            "--db",
            "old.db",
        ],
    ));
    assert!(backfill.is_object());
    let result = success(&run(dir.path(), &["search", "Clockwork", "--db", "old.db"]));
    assert_eq!(result["count"], 1);
    assert_eq!(result["ranking"], "keyword");
}

#[test]
fn explicit_false_preserves_fresh_store_opt_out() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".bobbin")).unwrap();
    std::fs::write(
        dir.path().join(".bobbin/config.toml"),
        "[quipu.search]\nkeyword = false\n",
    )
    .unwrap();
    let doctor = success(&run(dir.path(), &["doctor", "search", "--db", "fresh.db"]));
    assert_eq!(doctor["keyword_ready"], false);
    let conn = rusqlite::Connection::open(dir.path().join("fresh.db")).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name LIKE 'lexical_%'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}
