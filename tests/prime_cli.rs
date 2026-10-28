//! `quipu prime`: static primer always, live summary read-only and bounded.
#![cfg(feature = "shacl")]

use std::path::Path;
use std::process::{Command, Output};

fn prime(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quipu"))
        .current_dir(dir)
        .env("HOME", dir)
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
    assert!(!text.contains("## Live state"));
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
    assert!(String::from_utf8_lossy(&out.stdout).contains("Store absent"));
    assert!(!db.parent().unwrap().exists());
}

#[test]
fn cold_wal_summary_is_read_only_and_fast() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("live ?#% ü.db");
    {
        let store = quipu::Store::open(db.to_str().unwrap()).unwrap();
        store
            .graph_create_recorded("urn:fixture:ops", "2026-10-27T00:00:00Z", Some("fixture"))
            .unwrap();
    }
    let wal = std::path::PathBuf::from(format!("{}-wal", db.display()));
    let shm = std::path::PathBuf::from(format!("{}-shm", db.display()));
    assert!(!wal.exists() && !shm.exists(), "fixture must be cold");
    let before = std::fs::read(&db).unwrap();
    let metadata = std::fs::metadata(&db).unwrap();
    let entries = std::fs::read_dir(dir.path()).unwrap().count();
    let out = prime(dir.path(), &["--db", db.to_str().unwrap(), "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["live"]["summary"]["status"], "ok");
    // A fresh store also registers its own metadata graph.
    let names = v["live"]["summary"]["graphs"]["names"].as_array().unwrap();
    assert!(names.iter().any(|n| n == "urn:fixture:ops"), "{names:?}");
    assert!(
        v["primer"]["text"]
            .as_str()
            .unwrap()
            .contains("## MCP tools")
    );
    assert_eq!(
        std::fs::read(&db).unwrap(),
        before,
        "prime modified the store"
    );
    // The summary's own clock, against its 400 ms budget. Process start-up
    // is not prime's to bound and varies by host (over 1 s on some Macs).
    assert_eq!(
        std::fs::metadata(&db).unwrap().modified().unwrap(),
        metadata.modified().unwrap()
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), entries);
    let elapsed_ms = v["live"]["elapsed_ms"].as_u64().unwrap();
    assert!(elapsed_ms <= 400, "summary took {elapsed_ms} ms");
}

#[test]
fn existing_wal_is_read_without_losing_committed_graphs() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("live.db");
    let store = quipu::Store::open(db.to_str().unwrap()).unwrap();
    {
        let checkpoint = rusqlite::Connection::open(&db).unwrap();
        checkpoint
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .unwrap();
    }
    let checkpointed = std::fs::read(&db).unwrap();
    store
        .graph_create_recorded(
            "urn:fixture:wal-only",
            "2026-10-28T00:00:00Z",
            Some("fixture"),
        )
        .unwrap();
    assert_eq!(
        std::fs::read(&db).unwrap(),
        checkpointed,
        "fixture write must remain in WAL"
    );
    let wal = dir.path().join("live.db-wal");
    let shm = dir.path().join("live.db-shm");
    assert!(wal.exists() && shm.exists());
    let before_wal = std::fs::read(&wal).unwrap();
    assert!(!before_wal.is_empty());
    let db_time = std::fs::metadata(&db).unwrap().modified().unwrap();
    let wal_time = std::fs::metadata(&wal).unwrap().modified().unwrap();
    let mut paths = vec![db.clone()];
    #[cfg(unix)]
    {
        let alias = dir.path().join("alias.db");
        std::os::unix::fs::symlink(&db, &alias).unwrap();
        paths.push(alias);
    }
    let entries = std::fs::read_dir(dir.path()).unwrap().count();
    for path in paths {
        let out = prime(dir.path(), &["--db", path.to_str().unwrap(), "--json"]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["live"]["status"], "ok", "{v}");
        let names = v["live"]["summary"]["graphs"]["names"].as_array().unwrap();
        assert!(
            names.iter().any(|name| name == "urn:fixture:wal-only"),
            "{v}"
        );
        assert_eq!(std::fs::read(&db).unwrap(), checkpointed);
        assert_eq!(std::fs::read(&wal).unwrap(), before_wal);
        assert_eq!(std::fs::metadata(&db).unwrap().modified().unwrap(), db_time);
        assert_eq!(
            std::fs::metadata(&wal).unwrap().modified().unwrap(),
            wal_time
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), entries);
        assert!(shm.exists());
    }
}

#[test]
fn section_and_brief_select_text() {
    let dir = tempfile::tempdir().unwrap();
    let out = prime(dir.path(), &["--static", "--section", "writing"]);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with("## Writing safely"));
    let reconcile = prime(dir.path(), &["--static", "--section", "reconcil"]);
    assert!(reconcile.status.success());
    let text = String::from_utf8(reconcile.stdout).unwrap();
    assert!(text.starts_with("## Reconciling a failed merge"));
    assert!(!text.contains("## Next steps"));

    let bad = prime(dir.path(), &["--static", "--section", "nope"]);
    assert_eq!(bad.status.code(), Some(2));
    let brief = prime(dir.path(), &["--static", "--brief"]);
    let text = String::from_utf8(brief.stdout).unwrap();
    assert!((5..=10).contains(&text.lines().count()), "{text}");
    assert!(text.starts_with(concat!("quipu ", env!("CARGO_PKG_VERSION"), ":")));
    assert!(text.ends_with("Run `quipu prime` for the full primer.\n"));
}

#[test]
fn a_garbage_db_file_is_reported_unreadable() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("garbage.db");
    std::fs::write(&db, b"not a database\n".repeat(500)).unwrap();
    let out = prime(dir.path(), &["--db", db.to_str().unwrap(), "--json"]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["live"]["summary"]["status"], "unreadable", "{v}");
    let text = prime(dir.path(), &["--db", db.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&text.stdout).contains("Store unreadable"));
}

#[test]
fn json_contract_covers_static_missing_and_brief_without_creating_paths() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let db = dir.path().join("nested/q.db");
    for offline in [false, true] {
        for brief in [false, true] {
            let mut cmd = Command::new(env!("CARGO_BIN_EXE_quipu"));
            cmd.current_dir(dir.path()).env("HOME", &home).args([
                "prime",
                "--json",
                "--db",
                db.to_str().unwrap(),
            ]);
            if offline {
                cmd.arg("--static");
            }
            if brief {
                cmd.arg("--brief");
            }
            let out = cmd.output().unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(v["schema"], "stack-prime/1");
            assert_eq!(v["tool"], "quipu");
            assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
            assert!(
                v["primer"]["sections"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|s| s == "MCP tools")
            );
            assert_eq!(v["primer"].get("text").is_none(), brief);
            assert_eq!(
                v["live"]["status"],
                if offline { "skipped" } else { "absent" }
            );
            assert_eq!(v["status"], if offline { "ok" } else { "degraded" });
            assert!(v["live"]["elapsed_ms"].as_u64().unwrap() < 1000);
            assert!(v.get("store").is_none());
            assert!(!db.parent().unwrap().exists());
            assert_eq!(std::fs::read_dir(&home).unwrap().count(), 0);
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        }
    }
    let out = prime(dir.path(), &["--brief", "--db", db.to_str().unwrap()]);
    assert!(out.status.success());
    assert!((5..=10).contains(&String::from_utf8(out.stdout).unwrap().lines().count()));
}

#[test]
fn invalid_flags_still_exit_two() {
    let dir = tempfile::tempdir().unwrap();
    for args in [&["--unknown"][..], &["--section"][..]] {
        assert_eq!(prime(dir.path(), args).status.code(), Some(2));
    }
}
