//! Cold snapshot export exercises the actual CLI, not just argument parsing.
#![cfg(feature = "shacl")]
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quipu"))
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap()
}

#[test]
fn readonly_export_preserves_bytes_mtime_and_directory() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("input.nt"),
        "<urn:positive> <urn:predicate> <urn:object> .\n",
    )
    .unwrap();
    let setup = run(dir.path(), &["knot", "input.nt", "--db", "snapshot.db"]);
    assert!(setup.status.success(), "{setup:?}");
    let path = dir.path().join("snapshot.db");
    let before = fs::read(&path).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let names = || {
        let mut paths: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|p| p.unwrap().file_name())
            .collect();
        paths.sort();
        paths
    };
    let files = names();
    let exported = run(
        dir.path(),
        &["export", "--read-only", "--db", "snapshot.db"],
    );
    assert!(exported.status.success(), "{exported:?}");
    assert!(String::from_utf8_lossy(&exported.stdout).contains("<urn:positive>"));
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    assert_eq!(names(), files);
    let normal = run(dir.path(), &["export", "--db", "snapshot.db"]);
    assert!(normal.status.success(), "{normal:?}");
    assert_eq!(normal.stdout, exported.stdout);
}

#[test]
fn readonly_export_missing_input_refuses_but_default_still_creates() {
    let dir = tempfile::tempdir().unwrap();
    let refused = run(dir.path(), &["export", "--read-only", "--db", "missing.db"]);
    assert!(!refused.status.success());
    assert!(!dir.path().join("missing.db").exists());
    let normal = run(dir.path(), &["export", "--db", "default.db"]);
    assert!(normal.status.success(), "{normal:?}");
    assert!(dir.path().join("default.db").exists());
}

#[test]
fn readonly_export_named_graph_keeps_root_out() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("root.nt"),
        "<urn:root-only> <urn:p> <urn:o> .\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("named.nt"),
        "<urn:named-only> <urn:p> <urn:o> .\n",
    )
    .unwrap();
    for args in [
        vec!["knot", "root.nt", "--db", "snapshot.db"],
        vec![
            "knot",
            "named.nt",
            "--db",
            "snapshot.db",
            "--graph",
            "urn:selected",
        ],
    ] {
        let setup = run(dir.path(), &args);
        assert!(setup.status.success(), "{setup:?}");
    }
    let db = dir.path().join("snapshot.db");
    let before = fs::read(&db).unwrap();
    let modified = fs::metadata(&db).unwrap().modified().unwrap();
    let out = run(
        dir.path(),
        &[
            "export",
            "--read-only",
            "--db",
            "snapshot.db",
            "--graph",
            "urn:selected",
        ],
    );
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("<urn:named-only>"));
    assert!(!text.contains("<urn:root-only>"));
    assert_eq!(fs::read(&db).unwrap(), before);
    assert_eq!(fs::metadata(&db).unwrap().modified().unwrap(), modified);
}
