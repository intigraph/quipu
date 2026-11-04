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

#[test]
fn store_status_reports_inactive_without_implicitly_adopting() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    let original = quipu::Store::open(path.to_str().unwrap()).unwrap();
    let before = original.log_epoch().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_quipu"))
        .args(["store", "status", "--db", path.to_str().unwrap()])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let status: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["ownership"], "INACTIVE");
    assert_eq!(original.log_epoch().unwrap(), before);
    assert!(original.require_ownership().is_err());
    assert!(!path.with_extension("db.ownership.json").exists());
}

#[test]
fn legacy_cli_restart_preserves_data_and_epoch_without_activating_enforcement() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    // A pre-ownership schema image: no owner table or filesystem record.
    let legacy = quipu::Store::open_in_memory().unwrap();
    std::fs::write(&path, legacy.serialize_db().unwrap()).unwrap();
    let epoch = legacy.log_epoch().unwrap();
    let fact = dir.path().join("fact.ttl");
    std::fs::write(&fact, r#"<urn:legacy:s> <urn:legacy:p> "kept" ."#).unwrap();
    for _ in 0..2 {
        let out = Command::new(env!("CARGO_BIN_EXE_quipu"))
            .args([
                "knot",
                fact.to_str().unwrap(),
                "--db",
                path.to_str().unwrap(),
            ])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        let reopened = quipu::Store::open_with_attachments(path.to_str().unwrap(), &[]).unwrap();
        assert_eq!(reopened.log_epoch().unwrap(), epoch);
        assert_eq!(
            reopened.ownership_state().unwrap(),
            quipu::store::OwnershipState::Inactive
        );
        assert!(reopened.require_ownership().is_err());
    }
    assert!(!path.with_extension("db.ownership.json").exists());
}
