//! Exercise the actual operator executable against a private file-backed store.
#![cfg(feature = "shacl")]
use std::process::{Command, Output};

fn cli(db: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quipu"))
        .arg("signer")
        .args(args)
        .arg("--db")
        .arg(db)
        .output()
        .unwrap()
}
fn json(out: &Output) -> serde_json::Value {
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}
fn enroll(db: &std::path::Path, extra: &[&str]) -> Output {
    use base64::Engine as _;
    let mut key = vec![0xa4, 0x01, 0x01, 0x03, 0x27, 0x20, 0x06, 0x21, 0x58, 0x20];
    key.extend(1..=32u8);
    let key = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key);
    let expiry = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 86400)
        .to_string();
    let mut args = vec![
        "enroll",
        "--credential-id",
        "Y3JlZGVudGlhbC0x",
        "--cose-key",
        &key,
        "--scheme",
        "webauthn-eddsa",
        "--origin",
        "https://signer.example.org",
        "--rp-id",
        "signer.example.org",
        "--actor",
        "operator@example.org",
        "--expires-at",
        &expiry,
    ];
    args.extend_from_slice(extra);
    cli(db, &args)
}
#[test]
fn explicit_enrollment_graph_protection_switch_and_revocation() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("signers.db");
    assert_eq!(
        json(&cli(&db, &["status"])),
        serde_json::json!({"enabled":false,"signers":[]})
    );
    let out = enroll(
        &db,
        &[
            "--graph",
            "https://example.org/answers",
            "--graph",
            "https://example.org/other",
        ],
    );
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("9bb1-7e67-b268-ca8b"));
    let status = json(&cli(&db, &["status"]));
    assert_eq!(status["enabled"], false);
    assert_eq!(
        status["signers"][0]["graphs"],
        serde_json::json!(["https://example.org/answers", "https://example.org/other"])
    );
    assert_eq!(status["signers"][0]["fingerprint"], "9bb1-7e67-b268-ca8b");
    {
        let store = quipu::Store::open(db.to_str().unwrap()).unwrap();
        assert_eq!(store.attest_signers().unwrap().len(), 1);
        assert!(
            !store
                .attest_graph_is_protected("https://example.org/answers")
                .unwrap()
        );
    }
    assert!(
        cli(&db, &["protect-graph", "https://example.org/answers"])
            .status
            .success()
    );
    assert!(
        quipu::Store::open(db.to_str().unwrap())
            .unwrap()
            .attest_graph_is_protected("https://example.org/answers")
            .unwrap()
    );
    let proposal = quipu::store::attested_write::Proposal {
        signer: "Y3JlZGVudGlhbC0x".into(),
        graph: "https://example.org/answers".into(),
        datums: vec![quipu::store::attested_write::AttestedDatum {
            s: "https://example.org/subject".into(),
            p: "https://example.org/predicate".into(),
            o: quipu::store::attested_write::TypedValue::Str("fixture".into()),
        }],
        expect: vec![],
    };
    assert!(
        quipu::Store::open(db.to_str().unwrap())
            .unwrap()
            .attest_challenge(&proposal)
            .is_err()
    );
    assert!(cli(&db, &["enable"]).status.success());
    assert_eq!(json(&cli(&db, &["status"]))["enabled"], true);
    {
        let mut store = quipu::Store::open(db.to_str().unwrap()).unwrap();
        let challenge = store.attest_challenge(&proposal).unwrap();
        assert_eq!(challenge.actor, "operator@example.org");
        assert_eq!(challenge.proposal, proposal);
        let mut foreign = proposal.clone();
        foreign.graph = "https://example.org/foreign".into();
        store.attest_protect_graph(&foreign.graph).unwrap();
        assert!(store.attest_challenge(&foreign).is_err());
    }
    assert!(cli(&db, &["revoke", "Y3JlZGVudGlhbC0x"]).status.success());
    assert!(
        quipu::Store::open(db.to_str().unwrap())
            .unwrap()
            .attest_challenge(&proposal)
            .is_err()
    );
    let status = json(&cli(&db, &["status"]));
    assert_eq!(status["signers"][0]["revoked"], true);
    assert_eq!(status["signers"][0]["sign_count"], 0);
    assert!(cli(&db, &["disable"]).status.success());
    assert_eq!(json(&cli(&db, &["status"]))["enabled"], false);
    assert!(
        !enroll(&db, &["--graph", "https://example.org/answers"])
            .status
            .success()
    );
    assert_eq!(
        json(&cli(&db, &["status"]))["signers"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn invalid_inputs_leave_registry_and_switch_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("signers.db");
    let before = json(&cli(&db, &["status"]));
    for extra in [
        vec![],
        vec!["--graph", "not-an-iri"],
        vec![
            "--graph",
            "https://example.org/answers",
            "--origin",
            "https://other.example.org",
        ],
        vec![
            "--graph",
            "https://example.org/answers",
            "--graph",
            "https://example.org/answers",
        ],
    ] {
        let out = enroll(&db, &extra);
        assert!(!out.status.success(), "{out:?}");
        assert_eq!(json(&cli(&db, &["status"])), before);
    }
    for args in [
        vec!["revoke", "unknown"],
        vec!["protect-graph", "urn:quipu:reserved"],
        vec!["enroll"],
        vec!["revoke"],
        vec!["protect-graph"],
    ] {
        assert!(!cli(&db, &args).status.success());
        assert_eq!(json(&cli(&db, &["status"])), before);
    }
    assert!(
        !quipu::Store::open(db.to_str().unwrap())
            .unwrap()
            .attest_graph_is_protected("urn:quipu:reserved")
            .unwrap()
    );
}
#[test]
fn help_never_opens_the_store_or_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("never-created.db");
    let out = cli(&db, &["--help"]);
    assert!(out.status.success());
    assert!(!db.exists());
    assert!(String::from_utf8_lossy(&out.stdout).contains("signer enroll"));
}

#[test]
fn global_database_option_before_subcommand_uses_same_store() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("signers.db");
    let out = Command::new(env!("CARGO_BIN_EXE_quipu"))
        .args(["signer", "--db"])
        .arg(&db)
        .arg("status")
        .output()
        .unwrap();
    assert_eq!(json(&out), json(&cli(&db, &["status"])));
}

#[test]
fn flag_shaped_actor_never_redirects_database_selection() {
    use base64::Engine as _;
    let mut key = vec![0xa4, 0x01, 0x01, 0x03, 0x27, 0x20, 0x06, 0x21, 0x58, 0x20];
    key.extend(1..=32u8);
    let key = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key);
    let expiry = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 86400)
        .to_string();
    for actor in ["--db", "operator@example.org"] {
        // All are syntax-admitted positions for the real global option.
        for position in [1, 2, 14, 16, 20] {
            let dir = tempfile::tempdir().unwrap();
            let db = dir.path().join("chosen.db");
            let mut args: Vec<String> = [
                "signer",
                "enroll",
                "--credential-id",
                "Y3JlZGVudGlhbC0x",
                "--cose-key",
                &key,
                "--scheme",
                "webauthn-eddsa",
                "--origin",
                "https://signer.example.org",
                "--rp-id",
                "signer.example.org",
                "--expires-at",
                &expiry,
                "--actor",
                actor,
                "--graph",
                "https://example.org/answers",
                "--graph",
                "https://example.org/other",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect();
            args.splice(
                position..position,
                ["--db".into(), db.to_str().unwrap().into()],
            );
            let out = Command::new(env!("CARGO_BIN_EXE_quipu"))
                .args(&args)
                .current_dir(dir.path())
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "actor={actor} position={position}: {out:?}"
            );
            assert!(db.is_file(), "chosen store missing: {args:?}");
            assert!(
                !dir.path().join("--graph").exists(),
                "actor redirected the store"
            );
            let status = json(&cli(&db, &["status"]));
            assert_eq!(status["enabled"], false);
            assert_eq!(status["signers"].as_array().unwrap().len(), 1);
            assert_eq!(status["signers"][0]["actor"], actor);
            assert_eq!(
                status["signers"][0]["graphs"],
                serde_json::json!(["https://example.org/answers", "https://example.org/other"])
            );
        }
    }
}
