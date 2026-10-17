//! One runtime executable owns HTTP, native MCP and the transition alias.
#![cfg(all(feature = "shacl", feature = "onnx", feature = "server"))]
use std::process::Command;

#[test]
fn mcp_requires_no_companion_or_configuration_for_identity() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".bobbin")).unwrap();
    std::fs::write(root.path().join(".bobbin/config.toml"), "not valid TOML [").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_quipu"))
        .args(["mcp", "--version"])
        .current_dir(root.path())
        .env("ORT_DYLIB_PATH", root.path().join("missing-runtime"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains(env!("QUIPU_GIT_SHA"))
    );
    assert!(!root.path().join(".quipu").exists());
}

#[test]
fn runtime_layout_is_a_pure_full_feature_capability() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_quipu"))
        .arg("--runtime-layout")
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "quipu-runtime-layout/1;single-executable=1;server=1"
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
#[cfg(unix)]
fn legacy_alias_and_serve_report_the_same_compiled_server() {
    let root = tempfile::tempdir().unwrap();
    let alias = root.path().join("quipu-server");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_quipu"), &alias).unwrap();
    let old = Command::new(alias)
        .arg("--version")
        .current_dir(root.path())
        .output()
        .unwrap();
    let new = Command::new(env!("CARGO_BIN_EXE_quipu"))
        .args(["serve", "--version"])
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(old.status.success() && new.status.success());
    assert_eq!(old.stdout, new.stdout);
    assert!(old.stderr.is_empty() && new.stderr.is_empty());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}
