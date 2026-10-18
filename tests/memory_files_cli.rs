//! Real CLI subprocesses with explicit synthetic repositories; no server calls.
use serde_json::{Value, json};
use std::{
    path::Path,
    process::{Command, Output},
};

fn run(root: &Path, args: &[&str]) -> Output {
    let executable =
        std::env::var_os("QUIPU_TEST_CLI").unwrap_or_else(|| env!("CARGO_BIN_EXE_quipu").into());
    Command::new(executable)
        .args(args)
        .current_dir(root)
        .env("HOME", root)
        .env_remove("QUIPU_SERVER")
        .output()
        .unwrap()
}

fn ok(root: &Path, args: &[&str]) -> Value {
    let output = run(root, args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn cli_commit_export_and_cross_owner_read_share_the_core_contract() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let graph = "urn:example:memory";
    let create = run(root, &["graph", "create", graph, "--db", "fixture.db"]);
    assert!(create.status.success());
    let owner = json!({"deployment":"urn:example:deployment", "project":"urn:example:project", "crew":"urn:example:alpha"});
    let source = "\u{feff}# Notes\r\nUnicode λ\r\n[topic](topics/one.md)\r\n";
    let mut request = json!({"owner":owner,"path":"MEMORY.md","text":source,
        "expected_snapshot":null,"expected_file":null,"idempotency_key":"one",
        "harness":"fixture","account":"selected"});
    std::fs::write(root.join("owner.json"), owner.to_string()).unwrap();
    std::fs::write(root.join("commit.json"), request.to_string()).unwrap();
    let args = [
        "memory",
        "commit",
        "commit.json",
        "--graph",
        graph,
        "--db",
        "fixture.db",
        "--local-authority",
    ];
    let committed = ok(root, &args);
    assert_eq!(committed["authority"], "local_filesystem");
    assert_eq!(ok(root, &args), committed); // replay, not a second revision
    let export = ok(
        root,
        &[
            "memory",
            "export",
            "owner.json",
            "--graph",
            graph,
            "--db",
            "fixture.db",
        ],
    );
    assert_eq!(export["format"], "quipu.memory-files/v1");
    assert_eq!(export["files"][0]["text"], source);
    assert_eq!(
        export["snapshot"]["revision"],
        committed["receipt"]["snapshot"]
    );
    let mut other = owner.clone();
    other["crew"] = json!("urn:example:beta");
    std::fs::write(root.join("other.json"), other.to_string()).unwrap();
    let absent = ok(
        root,
        &[
            "memory",
            "read",
            "other.json",
            "--graph",
            graph,
            "--db",
            "fixture.db",
        ],
    );
    assert!(absent["snapshot"].is_null());
    let mut beta_request = request.clone();
    beta_request["owner"] = other;
    beta_request["text"] = json!("BETA synthetic body");
    std::fs::write(root.join("beta.json"), beta_request.to_string()).unwrap();
    ok(
        root,
        &[
            "memory",
            "commit",
            "beta.json",
            "--graph",
            graph,
            "--db",
            "fixture.db",
            "--local-authority",
        ],
    );
    let beta = ok(
        root,
        &[
            "memory",
            "export",
            "other.json",
            "--graph",
            graph,
            "--db",
            "fixture.db",
        ],
    );
    assert_eq!(beta["files"][0]["text"], "BETA synthetic body");
    assert_ne!(beta["snapshot"]["owner"], export["snapshot"]["owner"]);
    request["idempotency_key"] = json!("stale-new-intent");
    std::fs::write(root.join("commit.json"), request.to_string()).unwrap();
    assert!(!run(root, &args).status.success());
    assert_eq!(
        ok(
            root,
            &[
                "memory",
                "export",
                "owner.json",
                "--graph",
                graph,
                "--db",
                "fixture.db"
            ]
        ),
        export
    );
}

#[test]
fn cli_help_and_missing_explicit_authority_never_open_default_store() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    assert!(run(root, &["memory", "--help"]).status.success());
    assert!(
        !run(root, &["memory", "commit", "missing.json"])
            .status
            .success()
    );
    assert!(!root.join(".bobbin").exists());
    assert!(!root.join("fixture.db").exists());
}
