//! Execute the operator-facing verbs against isolated databases only.
#![cfg(feature = "shacl")]
use std::process::Command;

#[test]
fn store_ownership_cli_restore_and_repeated_adopt_mint_new_epochs() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.db");
    let target = dir.path().join("target.db");
    let store = quipu::Store::open(source.to_str().unwrap()).unwrap();
    let old = store.log_epoch().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_quipu"))
        .args([
            "store",
            "restore",
            "--db",
            target.to_str().unwrap(),
            source.to_str().unwrap(),
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let restored: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_ne!(restored["log_epoch"], old);
    let output = Command::new(env!("CARGO_BIN_EXE_quipu"))
        .args(["store", "adopt", "--db", target.to_str().unwrap()])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let adopted: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_ne!(adopted["log_epoch"], restored["log_epoch"]);
    assert_eq!(store.log_epoch().unwrap(), old);
    assert!(
        quipu::Store::open(target.to_str().unwrap())
            .unwrap()
            .require_ownership()
            .is_ok()
    );
}

#[test]
fn store_ownership_cli_adopt_does_not_create_a_missing_store() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("absent.db");
    let output = Command::new(env!("CARGO_BIN_EXE_quipu"))
        .args(["store", "adopt", "--db", path.to_str().unwrap()])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!path.exists());
}
