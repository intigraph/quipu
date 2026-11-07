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

fn group_snapshot(dir: &Path) {
    fs::create_dir(dir.join(".bobbin")).unwrap();
    fs::write(
        dir.join(".bobbin/config.toml"),
        "[quipu]\nbase_ns = \"https://custom.example/ns/\"\n",
    )
    .unwrap();
    let mut store = quipu::Store::open(dir.join("snapshot.db").to_str().unwrap()).unwrap();
    quipu::ingest_rdf(
        &mut store,
        br#"@prefix c: <https://custom.example/ns/> .
            @prefix prov: <http://www.w3.org/ns/prov#> .
            c:wantedEpisode c:groupId "wanted" .
            c:wantedEntity prov:wasGeneratedBy c:wantedEpisode ; c:label "selected" .
            c:otherEpisode c:groupId "other" .
            c:otherEntity prov:wasGeneratedBy c:otherEpisode ; c:label "other" .
            c:literalEpisode c:groupId "--read-only" .
            c:literalEntity prov:wasGeneratedBy c:literalEpisode ; c:label "literal" .
            c:wrongNamespaceEpisode <urn:decoy:groupId> "wanted" .
            c:wrongNamespaceEntity prov:wasGeneratedBy c:wrongNamespaceEpisode .
        "#,
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-01-01T00:00:00Z",
        None,
        None,
    )
    .unwrap();
}

#[test]
fn readonly_group_export_preserves_configured_namespace_and_scope() {
    let dir = tempfile::tempdir().unwrap();
    group_snapshot(dir.path());
    let normal = run(
        dir.path(),
        &["export", "--db", "snapshot.db", "--group-id", "wanted"],
    );
    assert!(normal.status.success(), "{normal:?}");
    let db = dir.path().join("snapshot.db");
    let before = fs::read(&db).unwrap();
    let modified = fs::metadata(&db).unwrap().modified().unwrap();
    let readonly = run(
        dir.path(),
        &["export", "--read-only", "--db", "snapshot.db", "--group-id", "wanted"],
    );
    assert!(readonly.status.success(), "{readonly:?}");
    assert_eq!(readonly.stdout, normal.stdout);
    let text = String::from_utf8(readonly.stdout).unwrap();
    assert!(text.contains("wantedEntity") && text.contains("wantedEpisode"));
    for excluded in ["otherEntity", "literalEntity", "wrongNamespaceEntity"] {
        assert!(!text.contains(excluded), "{text}");
    }
    assert_eq!(fs::read(&db).unwrap(), before);
    assert_eq!(fs::metadata(db).unwrap().modified().unwrap(), modified);
}

#[test]
fn literal_readonly_group_value_does_not_select_snapshot_mode() {
    let dir = tempfile::tempdir().unwrap();
    group_snapshot(dir.path());
    // Deliberately leave a sidecar: only the actual flag must refuse it.
    let sidecar = dir.path().join("snapshot.db-journal");
    fs::write(&sidecar, b"").unwrap();
    let literal = run(
        dir.path(),
        &["export", "--db", "snapshot.db", "--group-id", "--read-only"],
    );
    assert!(literal.status.success(), "{literal:?}");
    assert!(String::from_utf8_lossy(&literal.stdout).contains("literalEntity"));
    fs::write(sidecar, b"").unwrap();
    let flagged = run(
        dir.path(),
        &["export", "--group-id", "--read-only", "--read-only", "--db", "snapshot.db"],
    );
    assert!(!flagged.status.success(), "{flagged:?}");
    assert!(String::from_utf8_lossy(&flagged.stderr).contains("sidecar exists"));
}

#[test]
fn literal_readonly_database_value_keeps_default_creation() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["export", "--db", "--read-only"]);
    assert!(out.status.success(), "{out:?}");
    assert!(dir.path().join("--read-only").is_file());
    let flagged_dir = tempfile::tempdir().unwrap();
    let out = run(
        flagged_dir.path(),
        &["export", "--db", "--read-only", "--read-only"],
    );
    assert!(!out.status.success(), "{out:?}");
    assert!(!flagged_dir.path().join("--read-only").exists());
}
