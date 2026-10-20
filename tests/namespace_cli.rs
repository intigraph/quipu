//! Real CLI configuration, persistence and mint/list contract.
#![cfg(feature = "shacl")]
use std::process::Command;

#[test]
fn local_cli_namespace_registry_is_readable_persistent_and_public_by_default() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".bobbin")).unwrap();
    std::fs::write(
        dir.path().join(".bobbin/config.toml"),
        "[[quipu.namespaces]]\nprefix = 'public'\n",
    )
    .unwrap();
    let db = dir.path().join("store.db");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_quipu"))
            .current_dir(dir.path())
            .env("HOME", dir.path())
            .args(args)
            .args(["--db", db.to_str().unwrap()])
            .output()
            .unwrap()
    };
    let list = run(&["namespaces", "list"]);
    assert!(
        list.status.success(),
        "{}",
        String::from_utf8_lossy(&list.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap();
    assert_eq!(value["namespaces"][0]["audience"], "public");
    assert_eq!(
        value["namespaces"][0]["base_iri"],
        "https://w3id.org/intigraph/"
    );
    assert!(
        quipu::Store::open(db.to_str().unwrap())
            .unwrap()
            .lookup(quipu::entity_namespaces::REGISTRY_GRAPH)
            .unwrap()
            .is_none()
    );
    assert!(run(&["namespaces", "apply"]).status.success());
    std::fs::write(
        dir.path().join("request.json"),
        r#"{"namespace":"public","type":"urn:example:Host","label":"A mutable label"}"#,
    )
    .unwrap();
    let minted = run(&["entity-ref", "mint", "request.json"]);
    assert!(
        minted.status.success(),
        "{}",
        String::from_utf8_lossy(&minted.stderr)
    );
    let minted: serde_json::Value = serde_json::from_slice(&minted.stdout).unwrap();
    assert!(
        minted["iri"]
            .as_str()
            .unwrap()
            .starts_with("https://w3id.org/intigraph/")
    );
    std::fs::write(dir.path().join(".bobbin/config.toml"), "[quipu]\n").unwrap();
    let retained = run(&["namespaces", "list"]);
    assert!(retained.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&retained.stdout).unwrap(),
        value
    );
    std::fs::write(
        dir.path().join("bad.json"),
        r#"{"namespace":"missing","type":"urn:example:Host"}"#,
    )
    .unwrap();
    assert!(!run(&["entity-ref", "mint", "bad.json"]).status.success());
}
