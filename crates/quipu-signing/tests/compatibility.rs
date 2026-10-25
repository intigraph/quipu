//! Fixed output from the pre-extraction Quipu module on disposable seed [7;32].
//! Signature validity remains separate from verifier authorization.
use quipu_signing::{evidence_hash, public_key_hex, sign_hex, verdict_message, verify_hex};
use ring::signature::Ed25519KeyPair;

#[test]
fn legacy_quipu_vector_and_all_message_fields_remain_identical() {
    let vector: serde_json::Value = serde_json::from_str(include_str!("legacy-v1.json")).unwrap();
    let pair = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    let hash = evidence_hash("shared-signing-fixture");
    assert_eq!(hash, vector["hash"].as_str().unwrap());
    let fields = [
        "fixture-policy",
        "urn:fixture:target",
        "satisfied",
        &hash,
        "tree-sitter",
        "yupana",
    ];
    let message = |f: [&str; 6]| verdict_message(f[0], f[1], f[2], f[3], f[4], f[5]);
    let bytes = message(fields);
    let public_key = public_key_hex(&pair);
    let signature = sign_hex(&pair, &bytes);
    assert_eq!(bytes, vector["message"].as_str().unwrap().as_bytes());
    assert_eq!(public_key, vector["public_key"].as_str().unwrap());
    assert_eq!(signature, vector["signature"].as_str().unwrap());
    assert!(verify_hex(&public_key, &bytes, &signature));
    for index in 0..6 {
        let mut changed = fields;
        changed[index] = "substituted-fixture";
        assert!(
            !verify_hex(&public_key, &message(changed), &signature),
            "field {index} escaped the signature"
        );
    }
    let foreign = Ed25519KeyPair::from_seed_unchecked(&[8; 32]).unwrap();
    assert!(!verify_hex(&public_key_hex(&foreign), &bytes, &signature));
    for invalid in ["", "zz", "ab", "00"] {
        assert!(!verify_hex(invalid, &bytes, &signature));
        assert!(!verify_hex(&public_key, &bytes, invalid));
    }
}

#[test]
fn hashing_includes_exact_bytes_without_trimming_or_case_changes() {
    assert_ne!(evidence_hash("fixture"), evidence_hash("fixture\n"));
    assert_ne!(evidence_hash("fixture"), evidence_hash("Fixture"));
    assert_ne!(evidence_hash("a\0b"), evidence_hash("a"));
    assert_eq!(
        evidence_hash(""),
        "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn existing_invalid_key_is_never_silently_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("invalid-fixture.pk8");
    let bytes = b"intentionally-not-a-private-key";
    std::fs::write(&file, bytes).unwrap();
    assert!(quipu_signing::load_or_generate(&file).is_err());
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
}

#[test]
fn bare_filename_custody_preserves_the_existing_yupana_contract() {
    let directory = tempfile::tempdir().unwrap();
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "bare_filename_child", "--nocapture"])
        .current_dir(directory.path())
        .env("SIGNING_BARE_PATH_CHILD", "fixture")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(directory.path().join("fixture.pk8").is_file());
}

#[test]
fn bare_filename_child() {
    if std::env::var("SIGNING_BARE_PATH_CHILD").as_deref() != Ok("fixture") {
        return;
    }
    let path = std::path::Path::new("fixture.pk8");
    let first = quipu_signing::load_or_generate(path).unwrap();
    let second = quipu_signing::load_or_generate(path).unwrap();
    assert_eq!(public_key_hex(&first), public_key_hex(&second));
}
