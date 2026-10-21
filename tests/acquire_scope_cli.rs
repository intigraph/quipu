//! `quipu acquire scope`: read-only, explicit budget, no store change.
#![cfg(feature = "shacl")]

use std::path::Path;
use std::process::{Command, Output};

const GRAPH: &str = "https://example.org/project/board";

fn run(dir: &Path, db: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quipu"))
        .current_dir(dir)
        .env("HOME", dir)
        .args(["acquire", "scope", "--db"])
        .arg(db)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn scope_measures_a_file_store_without_changing_it() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("artifact.db");
    let store_id = {
        let mut store = quipu::Store::open(db.to_str().unwrap()).unwrap();
        let graph = store.graph_create(GRAPH).unwrap();
        quipu::rdf::ingest_rdf_to_graph(
            &mut store,
            b"<https://example.org/i/1> <https://example.org/t> \"one\" .\n<https://example.org/i/2> <https://example.org/t> \"two\" ."
                .as_slice(),
            oxrdfio::RdfFormat::NTriples,
            None,
            "2026-01-01T00:00:00Z",
            None,
            None,
            graph,
        )
        .unwrap();
        store.store_id().unwrap()
    };
    let before = std::fs::read(&db).unwrap();
    let ok = run(
        dir.path(),
        &db,
        &[
            "--source-store-id",
            &store_id,
            "--graph",
            GRAPH,
            "--max-vm-steps",
            "100000000",
            "--deadline-ms",
            "60000",
        ],
    );
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    let out: serde_json::Value = serde_json::from_slice(&ok.stdout).unwrap();
    assert_eq!(out["schema"], "quipu-acquisition-scope/v1");
    assert_eq!(out["graphs"][GRAPH]["status"], "measured");
    assert_eq!(out["graphs"][GRAPH]["rows"], 2);
    assert_eq!(
        std::fs::read(&db).unwrap(),
        before,
        "the store file changed"
    );

    // The budget is explicit: omitting it refuses before opening the store.
    let unbudgeted = run(
        dir.path(),
        &db,
        &[
            "--source-store-id",
            &store_id,
            "--graph",
            GRAPH,
            "--deadline-ms",
            "60000",
        ],
    );
    assert!(!unbudgeted.status.success());
    assert!(String::from_utf8_lossy(&unbudgeted.stderr).contains("--max-vm-steps is required"));

    // A missing store is not created.
    let absent = dir.path().join("absent.db");
    let missing = run(
        dir.path(),
        &absent,
        &[
            "--source-store-id",
            &store_id,
            "--graph",
            GRAPH,
            "--max-vm-steps",
            "1000000",
            "--deadline-ms",
            "1000",
        ],
    );
    assert!(!missing.status.success());
    assert!(!absent.exists(), "a read-only scope created a store");
}
