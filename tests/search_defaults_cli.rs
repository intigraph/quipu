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
    // New library stores now index by default. Model an actual pre-index
    // store explicitly; otherwise this fixture tests a fresh indexed store.
    store.search_config_mut().keyword = false;
    quipu::ingest_rdf(&mut store,
        r#"<https://example.org/legacy> <http://www.w3.org/2000/01/rdf-schema#label> "Clockwork" ."#.as_bytes(),
        oxrdfio::RdfFormat::Turtle, None, "2026-01-01T00:00:00Z", None, None,
    ).unwrap();
    drop(store);
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        let installed: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name LIKE 'lexical_%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(installed, 0, "legacy fixture must start without an index");
    }
    // The default unindexed path now provides bounded lexical evidence;
    // explicit keyword mode still requires opt-in and a completed index.
    let fallback = success(&run(dir.path(), &["search", "Clockwork", "--db", "old.db"]));
    assert_eq!(fallback["count"], 1);
    assert_eq!(fallback["ranking"], "lexical");
    assert_eq!(
        fallback["retrieval"]["contract"],
        "quipu-lexical-results/v1"
    );
    assert_eq!(fallback["retrieval"]["status"], "degraded");
    assert_eq!(
        fallback["results"][0]["entity"],
        "https://example.org/legacy"
    );
    assert!(fallback["results"][0].get("similarity").is_none());
    let out = run(
        dir.path(),
        &["search", "Clockwork", "--mode", "keyword", "--db", "old.db"],
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("disabled"));
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
    let opted_in = run(
        dir.path(),
        &["search", "Clockwork", "--mode", "keyword", "--db", "old.db"],
    );
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
    // Populate this fixture through the bounded store API: the maintenance
    // CLI intentionally refuses during UTC minutes 10..20. Its pause policy
    // has explicit inside/outside clock controls in cli_search's unit tests.
    let fixture = quipu::Store::open(db.to_str().unwrap()).unwrap();
    let backfill = fixture.backfill_lexical_batch(500).unwrap();
    assert!(backfill.complete);
    assert_eq!(backfill.documents, 1);
    drop(fixture);
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

/// Uses a verified local fixture: download and bundled-runtime proofs are separate.
#[test]
#[ignore = "requires pinned QUIPU_TEST_MODEL_DIR and an ONNX-capable CLI runtime"]
fn one_model_command_prepares_existing_root_facts() {
    let fixture = std::path::PathBuf::from(
        std::env::var_os("QUIPU_TEST_MODEL_DIR").expect("set QUIPU_TEST_MODEL_DIR"),
    );
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("sample.ttl"),
        r#"<https://example.org/cats> <http://www.w3.org/2000/01/rdf-schema#label> "Cats are feline pets that hunt mice and other rodents" .
<https://example.org/railway> <http://www.w3.org/2000/01/rdf-schema#label> "Railway locomotives pull passenger trains along steel tracks" ."#,
    ).unwrap();
    let loaded = run(dir.path(), &["knot", "sample.ttl", "--db", "fresh.db"]);
    assert!(
        loaded.status.success(),
        "{}",
        String::from_utf8_lossy(&loaded.stderr)
    );
    let before = success(&run(dir.path(), &["doctor", "search", "--db", "fresh.db"]));
    assert_eq!(before["semantic_ready"], false);
    assert_eq!(before["vector_rows"], 0);
    let keyword = success(&run(dir.path(), &["search", "Cats", "--db", "fresh.db"]));
    assert_eq!(keyword["ranking"], "keyword");
    assert_eq!(keyword["count"], 1);

    // Cache only after ingest: these facts must be prepared by the one command.
    let cache = dir.path().join("data/quipu/models").join(format!(
        "all-MiniLM-L6-v2-{}",
        quipu::model_assets::REVISION
    ));
    std::fs::create_dir_all(&cache).unwrap();
    for file in ["model.onnx", "tokenizer.json"] {
        std::fs::copy(fixture.join(file), cache.join(file)).unwrap();
    }
    let installed = success(&run(dir.path(), &["model", "install", "--db", "fresh.db"]));
    assert_eq!(installed["preparation"]["embedded"], 2);
    assert_eq!(installed["preparation"]["complete"], true);
    let after = success(&run(dir.path(), &["doctor", "search", "--db", "fresh.db"]));
    assert_eq!(after["semantic_ready"], true);
    assert_eq!(after["vector_rows"], 2);
    let semantic = success(&run(
        dir.path(),
        &[
            "search",
            "feline pets hunt rodents",
            "--mode",
            "semantic",
            "--db",
            "fresh.db",
        ],
    ));
    assert_eq!(semantic["ranking"], "semantic");
    assert_eq!(semantic["results"][0]["entity"], "https://example.org/cats");
    assert!(semantic["results"][0]["similarity"].as_f64().is_some());
}
