//! The real syntax-admitted CLI must lint without reading config or a store.
#![cfg(feature = "shacl")]
use std::process::Command;

fn run(root: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_quipu"))
        .current_dir(root)
        .env_clear()
        .env("HOME", root)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn lint_is_store_free_and_reports_a_broken_question_by_id() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".bobbin")).unwrap();
    std::fs::write(
        root.path().join(".bobbin/config.toml"),
        "this is invalid TOML!",
    )
    .unwrap();
    let contract = r#"schema = "quipu-competency/1"
[[question]]
id = "present"
ask = "Is someone present?"
query = "ASK { ?p a <urn:Person> }"
justifies = ["urn:PersonShape"]
"#;
    std::fs::write(root.path().join("competency.toml"), contract).unwrap();
    // The target DB path is a directory. Any attempt to open it as a store
    // would fail; successful lint is a positive control for the early dispatch.
    let good = run(root.path(), &["competency", "lint", ".", "--db", "."]);
    assert!(
        good.status.success(),
        "{}",
        String::from_utf8_lossy(&good.stderr)
    );
    assert!(String::from_utf8_lossy(&good.stdout).contains("1 question(s), 0 command(s)"));
    assert!(good.stderr.is_empty());
    std::fs::write(
        root.path().join("competency.toml"),
        contract.replace("ASK", "BROKEN"),
    )
    .unwrap();
    let bad = run(root.path(), &["competency", "lint", "."]);
    assert_eq!(bad.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("question present"));
    let entries: Vec<_> = std::fs::read_dir(root.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(entries.len(), 2, "lint must not create a database");
}

#[test]
fn help_does_not_read_files_and_unknown_subcommand_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let help = run(root.path(), &["competency", "lint", "--help"]);
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("competency lint"));
    let bad = run(root.path(), &["competency", "execute", "."]);
    assert_eq!(bad.status.code(), Some(2));
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
