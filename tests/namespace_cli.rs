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
    let retained: serde_json::Value = serde_json::from_slice(&retained.stdout).unwrap();
    assert_eq!(retained["namespaces"], value["namespaces"]);
    // Persistence and minting advance the store-bound transaction receipt,
    // even though removing config leaves the effective policy unchanged.
    assert_ne!(retained["registry_hash"], value["registry_hash"]);
    let unchanged = run(&["namespaces", "list"]);
    assert!(unchanged.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&unchanged.stdout).unwrap(),
        retained
    );
    std::fs::write(
        dir.path().join("bad.json"),
        r#"{"namespace":"missing","type":"urn:example:Host"}"#,
    )
    .unwrap();
    assert!(!run(&["entity-ref", "mint", "bad.json"]).status.success());
}

fn pack_fixture(private: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".bobbin")).unwrap();
    std::fs::write(
        dir.path().join(".bobbin/config.toml"),
        "[[quipu.namespaces]]\nprefix='team'\nbase_iri='https://team.example/id/'\nvisibility='private'\naudience='org:example'\nresolver_url='https://team.example/entity?iri={iri}'\n\n[[quipu.namespace_destinations]]\nstore_id='receiver'\naudience='org:example'\n",
    )
    .unwrap();
    let mut store = quipu::Store::open(dir.path().join("store.db").to_str().unwrap()).unwrap();
    let subject = if private {
        "https://team.example/id/s"
    } else {
        "urn:test:s"
    };
    quipu::ingest_rdf(
        &mut store,
        format!("<{subject}> <urn:test:p> \"content\" .").as_bytes(),
        oxrdfio::RdfFormat::NTriples,
        None,
        "2026-10-21T00:00:00Z",
        None,
        Some("namespace-pack-cli"),
    )
    .unwrap();
    let graph = store.overlay_create("urn:test:catalogue", 0).unwrap();
    quipu::rdf::ingest_rdf_to_graph(
        &mut store,
        include_bytes!("fixtures/share-catalogue.ttl").as_slice(),
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-10-21T00:00:00Z",
        None,
        Some("namespace-pack-policy"),
        graph,
    )
    .unwrap();
    drop(store);
    let applied = pack_cli(dir.path(), &["namespaces", "apply"]);
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    dir
}

fn pack_cli(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_quipu"))
        .current_dir(dir)
        .env("HOME", dir)
        .args(args)
        .args(["--db", dir.join("store.db").to_str().unwrap()])
        .output()
        .unwrap()
}

#[test]
fn pack_cli_preserves_audience_in_binary_and_turtle_and_refuses_wrong_audience() {
    let dir = pack_fixture(true);
    let destination = r#"{"audience":"org:example","store_id":"receiver"}"#;
    for turtle in [false, true] {
        let out = dir.path().join(if turtle {
            "allowed-turtle"
        } else {
            "allowed.db"
        });
        let mut args = vec![
            "pack",
            "--out",
            out.to_str().unwrap(),
            "--destination",
            destination,
        ];
        if turtle {
            args.extend(["--format", "turtle"]);
        }
        let result = pack_cli(dir.path(), &args);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let actual = if turtle {
            let manifest: serde_json::Value =
                serde_json::from_slice(&std::fs::read(out.join("manifest.json")).unwrap()).unwrap();
            manifest["destination"].clone()
        } else {
            serde_json::to_value(
                quipu::pack::read_manifest(out.to_str().unwrap())
                    .unwrap()
                    .destination
                    .unwrap(),
            )
            .unwrap()
        };
        assert_eq!(
            actual,
            serde_json::from_str::<serde_json::Value>(destination).unwrap()
        );
        for (index, denied) in [
            "outward",
            "internal",
            r#"{"audience":"org:example","store_id":"forged"}"#,
        ]
        .iter()
        .enumerate()
        {
            let refused = dir.path().join(format!("refused-{turtle}-{index}"));
            let mut args = vec![
                "pack",
                "--out",
                refused.to_str().unwrap(),
                "--destination",
                denied,
            ];
            if turtle {
                args.extend(["--format", "turtle"]);
            }
            assert!(!pack_cli(dir.path(), &args).status.success());
            assert!(!refused.exists(), "refusal must leave no artifact");
        }
    }
}

#[test]
fn pack_cli_invalid_destination_refuses_without_artifact_for_both_formats() {
    let dir = pack_fixture(false);
    for turtle in [false, true] {
        for (index, invalid) in ["typo", "{", r#"{"audience":"org:example"}"#]
            .iter()
            .enumerate()
        {
            let out = dir.path().join(format!("invalid-{turtle}-{index}"));
            let mut args = vec![
                "pack",
                "--out",
                out.to_str().unwrap(),
                "--destination",
                invalid,
            ];
            if turtle {
                args.extend(["--format", "turtle"]);
            }
            let result = pack_cli(dir.path(), &args);
            assert_eq!(
                result.status.code(),
                Some(2),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert!(!out.exists(), "invalid destination must leave no artifact");
        }
    }
}

#[test]
fn conditional_namespace_cli_removes_and_restores_exact_policy_and_refuses_replay() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".bobbin")).unwrap();
    let config_path = dir.path().join(".bobbin/config.toml");
    let configured = "[[quipu.namespaces]]\nprefix = 'fixture'\nbase_iri = 'https://example.test/private/'\nvisibility = 'private'\naudience = 'org:test'\ngraphs = ['urn:example:graph']\ntypes = ['urn:example:Host']\n";
    std::fs::write(&config_path, "[quipu]\n").unwrap();
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
    let success = |args: &[&str]| -> serde_json::Value {
        let output = run(args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    };
    let initial = success(&["namespaces", "list"]);
    std::fs::write(&config_path, configured).unwrap();
    success(&["namespaces", "apply"]);
    std::fs::write(&config_path, "[quipu]\n").unwrap();
    let retained = success(&["namespaces", "list"]);
    assert_eq!(retained["namespaces"].as_array().unwrap().len(), 1);
    let request = serde_json::json!({
        "expected_registry_hash": retained["registry_hash"],
        "namespace": retained["namespaces"][0],
    });
    std::fs::write(
        dir.path().join("request.json"),
        serde_json::to_vec(&request).unwrap(),
    )
    .unwrap();
    let mut malformed = request.clone();
    malformed["unexpected"] = true.into();
    std::fs::write(
        dir.path().join("bad.json"),
        serde_json::to_vec(&malformed).unwrap(),
    )
    .unwrap();
    assert!(!run(&["namespaces", "remove", "bad.json"]).status.success());
    assert_eq!(success(&["namespaces", "list"]), retained);
    assert!(!run(&["namespaces", "remove"]).status.success());
    assert_eq!(success(&["namespaces", "list"]), retained);
    success(&["namespaces", "remove", "request.json"]);
    let removed = success(&["namespaces", "list"]);
    assert_eq!(removed["namespaces"], initial["namespaces"]);
    assert_ne!(removed["registry_hash"], retained["registry_hash"]);
    assert!(
        !run(&["namespaces", "remove", "request.json"])
            .status
            .success()
    );
    assert_eq!(success(&["namespaces", "list"]), removed);
    std::fs::write(&config_path, configured).unwrap();
    // Configuring the declaration changes the effective registry and receipt.
    let restore_predecessor = success(&["namespaces", "list"]);
    let mut restore = request;
    restore["expected_registry_hash"] = restore_predecessor["registry_hash"].clone();
    std::fs::write(
        dir.path().join("restore.json"),
        serde_json::to_vec(&restore).unwrap(),
    )
    .unwrap();
    assert!(
        !run(&["namespaces", "apply-if", "request.json"])
            .status
            .success()
    );
    assert_eq!(success(&["namespaces", "list"]), restore_predecessor);
    success(&["namespaces", "apply-if", "restore.json"]);
    std::fs::write(&config_path, "[quipu]\n").unwrap();
    assert_eq!(
        success(&["namespaces", "list"])["namespaces"],
        retained["namespaces"]
    );
}
