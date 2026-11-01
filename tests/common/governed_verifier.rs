//! Authentic human-root endorsement for private workflow fixtures only.
use quipu::{
    Store,
    signing::{public_key_hex, sign_hex},
};
use ring::{rand::SystemRandom, signature::Ed25519KeyPair};

/// Private fixture: use the same human-root ceremony and signed endorsement
/// as production. Ordinary graph registration alone no longer grants trust.
pub fn endorse(s: &mut Store, registration: &str, timestamp: &str) {
    use quipu::governance::trust_root::{bootstrap, bootstrap_message, endorse, endorse_challenge};
    let doc = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let root = Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap();
    let key = public_key_hex(&root);
    let proof = sign_hex(
        &root,
        &bootstrap_message(&s.store_id().unwrap(), "fixture-human", &key),
    );
    bootstrap(s, "fixture-human", &key, &[], &proof, timestamp).unwrap();
    let challenge = endorse_challenge(s, registration, "agent").unwrap();
    endorse(
        s,
        registration,
        "agent",
        "fixture-human",
        &challenge.nonce,
        &sign_hex(&root, &challenge.message),
        "1970-01-01T00:00:00Z",
    )
    .unwrap();
}
