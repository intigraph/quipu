//! Source-local catalogue/read clock must match before any publication.
#![cfg(all(feature = "shacl", any(target_os = "linux", target_os = "macos")))]
use std::path::Path;
use std::process::{Command, Output};

fn cli(root: &Path, db: &Path, args: &[&str]) -> Output {
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    Command::new(env!("CARGO_BIN_EXE_quipu"))
        .args(args)
        .args(["--db", db.to_str().unwrap()])
        .current_dir(root)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .output()
        .unwrap()
}
fn good(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn render(root: &Path, db: &Path, dest: &Path, tx: &str) -> Output {
    cli(
        root,
        db,
        &[
            "render",
            "--query",
            "SELECT ?title WHERE { VALUES ?title { 'Pinned' } }",
            "--dest",
            dest.to_str().unwrap(),
            "--expected-tx",
            tx,
        ],
    )
}
#[test]
fn matching_clock_publishes_and_advanced_clock_refuses_untouched() {
    let root = tempfile::tempdir().unwrap();
    let db = root.path().join("fixture.db");
    let input = root.path().join("input.ttl");
    std::fs::write(
        &input,
        "<urn:example:thing> <http://www.w3.org/2000/01/rdf-schema#label> \"One\" .",
    )
    .unwrap();
    good(cli(root.path(), &db, &["knot", input.to_str().unwrap()]));
    let dest = root.path().join("output");
    let mismatch = render(root.path(), &db, &dest, "2");
    assert!(!mismatch.status.success());
    assert!(String::from_utf8_lossy(&mismatch.stderr).contains("differs from --expected-tx"));
    assert!(!dest.exists());
    let first: serde_json::Value =
        serde_json::from_str(&good(render(root.path(), &db, &dest, "1"))).unwrap();
    assert_eq!(first["source_tx"], 1);
    let report = std::fs::read(dest.join("report.md")).unwrap();
    let manifest = std::fs::read(dest.join(".quipu-render.json")).unwrap();
    let modified = std::fs::metadata(dest.join("report.md"))
        .unwrap()
        .modified()
        .unwrap();
    std::fs::write(
        &input,
        "<urn:example:other> <http://www.w3.org/2000/01/rdf-schema#label> \"Two\" .",
    )
    .unwrap();
    good(cli(root.path(), &db, &["knot", input.to_str().unwrap()]));
    assert!(!render(root.path(), &db, &dest, "1").status.success());
    assert_eq!(std::fs::read(dest.join("report.md")).unwrap(), report);
    assert_eq!(
        std::fs::read(dest.join(".quipu-render.json")).unwrap(),
        manifest
    );
    assert_eq!(
        std::fs::metadata(dest.join("report.md"))
            .unwrap()
            .modified()
            .unwrap(),
        modified
    );
    let current: serde_json::Value =
        serde_json::from_str(&good(render(root.path(), &db, &dest, "2"))).unwrap();
    assert_eq!(current["source_tx"], 2);
}
#[test]
fn malformed_or_duplicate_pins_refuse_before_store_creation() {
    let root = tempfile::tempdir().unwrap();
    let db = root.path().join("never-created.db");
    let dest = root.path().join("output");
    for tx in ["0", "-1", "nope", "9223372036854775808"] {
        assert!(!render(root.path(), &db, &dest, tx).status.success());
        assert!(!db.exists());
        assert!(!dest.exists());
    }
    let duplicate = cli(
        root.path(),
        &db,
        &[
            "render",
            "--query",
            "SELECT ?s WHERE {?s ?p ?o}",
            "--dest",
            dest.to_str().unwrap(),
            "--expected-tx",
            "1",
            "--expected-tx",
            "2",
        ],
    );
    assert!(!duplicate.status.success());
    assert!(!db.exists());
    assert!(!dest.exists());
}
