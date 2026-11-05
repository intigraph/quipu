//! Supported owner commands enroll observed pins; no policy SQL in the writer.
#![cfg(feature = "shacl")]
use quipu::writer_scope::{GraphInstance, WriteGrant, WriteOperation};
use std::{
    collections::BTreeSet,
    path::Path,
    process::{Command, Output},
};

fn cli(dir: &Path, db: &Path, verb: &str, value: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quipu"))
        .current_dir(dir)
        .args(["graph", verb, value, "--db"])
        .arg(db)
        .output()
        .unwrap()
}

#[test]
fn owner_cli_observes_enrolls_revises_and_withdraws_without_policy_sql() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("data.db");
    let store = quipu::Store::open(db.to_str().unwrap()).unwrap();
    store.graph_create("urn:owner:target").unwrap();
    drop(store);
    let observed = cli(dir.path(), &db, "writer-instance", "urn:owner:target");
    assert!(
        observed.status.success(),
        "{}",
        String::from_utf8_lossy(&observed.stderr)
    );
    let instance: GraphInstance = serde_json::from_slice(&observed.stdout).unwrap();
    let mut grant = WriteGrant {
        version: 1,
        id: "owner-test".into(),
        principal: "urn:principal:curator".into(),
        credential_id: "curator-1".into(),
        instance,
        operations: BTreeSet::from([WriteOperation::Update]),
        valid_from: 0,
        valid_until: quipu::time::epoch_secs() + 3600,
        revision: 1,
        revoked: false,
    };
    let file = dir.path().join("grant.json");
    std::fs::write(&file, serde_json::to_vec(&grant).unwrap()).unwrap();
    let apply = || cli(dir.path(), &db, "writer-grant", file.to_str().unwrap());
    let enrolled = apply();
    assert!(
        enrolled.status.success(),
        "{}",
        String::from_utf8_lossy(&enrolled.stderr)
    );
    assert!(
        !apply().status.success(),
        "same/older revision cannot replace policy"
    );
    let mut store = quipu::Store::open(db.to_str().unwrap()).unwrap();
    let identity = quipu::transaction_auth::Identity {
        principal: grant.principal.clone(),
        credential_id: Some(grant.credential_id.clone()),
        auth_class: "scoped_named_bearer".into(),
        attestation: Default::default(),
    };
    let targets = BTreeSet::from([grant.instance.graph.clone()]);
    quipu::transaction_auth::with_identity(Some(identity.clone()), || {
        let result: quipu::Result<()> =
            store.with_writer_scope(WriteOperation::Update, &targets, |_| Ok(()));
        assert!(result.is_ok());
    });
    grant.revision = 2;
    grant.revoked = true;
    std::fs::write(&file, serde_json::to_vec(&grant).unwrap()).unwrap();
    assert!(apply().status.success());
    quipu::transaction_auth::with_identity(Some(identity), || {
        let result: quipu::Result<()> =
            store.with_writer_scope(WriteOperation::Update, &targets, |_| Ok(()));
        assert!(result.is_err());
    });
    assert!(
        !cli(dir.path(), &db, "writer-instance", "urn:owner:unknown")
            .status
            .success()
    );
    assert_eq!(store.lookup("urn:owner:unknown").unwrap(), None);
}
