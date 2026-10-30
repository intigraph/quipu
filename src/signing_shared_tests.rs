//! Consumer controls use the pre-extraction fixture, not a newly generated oracle.
use super::*;
use ring::signature::Ed25519KeyPair;

#[test]
fn published_crate_and_quipu_preserve_the_fixed_verdict_vector() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../crates/quipu-signing/tests/legacy-v1.json")).unwrap();
    let pair = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    let hash = quipu_signing::evidence_hash("shared-signing-fixture");
    assert_eq!(hash, fixture["hash"]);
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
    assert_eq!(bytes, fixture["message"].as_str().unwrap().as_bytes());
    let public = public_key_hex(&pair);
    let signature = sign_hex(&pair, &bytes);
    assert_eq!(public, fixture["public_key"]);
    assert_eq!(signature, fixture["signature"]);
    assert!(verify_hex(&public, &bytes, &signature));
    assert!(quipu_signing::verify_hex(&public, &bytes, &signature));
    for field in 0..6 {
        let mut changed = fields;
        changed[field] = "tampered";
        assert!(!verify_hex(&public, &message(changed), &signature));
    }
    let other = Ed25519KeyPair::from_seed_unchecked(&[8; 32]).unwrap();
    assert!(!verify_hex(&public_key_hex(&other), &bytes, &signature));
    for malformed in ["", "zz", "00", "ab"] {
        assert!(!verify_hex(malformed, &bytes, &signature));
        assert!(!verify_hex(&public, &bytes, malformed));
    }
}

#[test]
fn actual_sync_verifier_uses_shared_crypto_without_changing_wire_domain() {
    use crate::replicate_wire::{Signed, message, verify};
    let pair = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    let public = public_key_hex(&pair);
    let frame = |session| Signed::Session {
        session,
        store_id: "fixture-store",
        log_epoch: "fixture-epoch",
        scope: "fixture-scope",
        since: 42,
    };
    let bytes = message(&frame("fixture-session"));
    let signature = quipu_signing::sign_hex(&pair, &bytes);
    assert!(verify(&public, &frame("fixture-session"), &signature));
    assert!(!verify(&public, &frame("changed-session"), &signature));
    let other = Ed25519KeyPair::from_seed_unchecked(&[8; 32]).unwrap();
    assert!(!verify(
        &public_key_hex(&other),
        &frame("fixture-session"),
        &signature
    ));
    assert!(!verify("malformed", &frame("fixture-session"), &signature));
    assert!(!verify(&public, &frame("fixture-session"), "malformed"));
    let verdict = verdict_message("p", "t", "satisfied", "hash", "committed", "quipu");
    assert!(!verify_hex(&public, &verdict, &signature));
    let verdict_signature = sign_hex(&pair, &verdict);
    assert!(!verify(
        &public,
        &frame("fixture-session"),
        &verdict_signature
    ));
    let identity: SigningIdentity = quipu_signing::SigningIdentity::new(pair, "fixture");
    assert_eq!(identity.sign(&bytes), signature);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn shared_key_error_keeps_quipu_error_kind_and_preserves_invalid_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("invalid.pk8");
    std::fs::write(&path, b"invalid disposable fixture").unwrap();
    assert!(matches!(
        load_or_generate(&path),
        Err(crate::Error::Store(_))
    ));
    assert!(matches!(
        SigningIdentity::load(&path, "fixture").map_err(crate::Error::from),
        Err(crate::Error::Store(_))
    ));
    assert_eq!(std::fs::read(&path).unwrap(), b"invalid disposable fixture");
}
