//! Disposable compatibility fixture, never an operational signing identity.
use quipu_signing::{evidence_hash, public_key_hex, sign_hex, verdict_message, verify_hex};

fn main() {
    let pair = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    let hash = evidence_hash("shared-signing-fixture");
    let message = verdict_message(
        "fixture-policy",
        "urn:fixture:target",
        "satisfied",
        &hash,
        "tree-sitter",
        "yupana",
    );
    let public_key = public_key_hex(&pair);
    let signature = sign_hex(&pair, &message);
    assert!(verify_hex(&public_key, &message, &signature));
    println!(
        "{}",
        serde_json::json!({
            "hash": hash, "message": String::from_utf8(message).unwrap(),
            "public_key": public_key, "signature": signature
        })
    );
}
