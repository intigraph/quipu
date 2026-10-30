//! Quipu's application adapter to the published shared signing implementation.
//!
//! The shared crate owns v1 message bytes, Ed25519 encoding and host-file I/O.
//! Quipu retains its key-path convention and its registration/authorization
//! checks. Signature validity alone is never authorization. Replication and
//! session/share envelopes retain their own canonical domain-separated messages.

#[cfg(not(target_arch = "wasm32"))]
use crate::error::Result;
#[cfg(not(target_arch = "wasm32"))]
use ring::signature::Ed25519KeyPair;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

pub use quipu_signing::{SigningIdentity, public_key_hex, sign_hex, verdict_message, verify_hex};

/// Where the host-file signing key lives: `$QUIPU_SIGNING_KEY`, else
/// `.quipu/verifier.pk8`.
///
/// Factored out because `quipu-server` and the `share --attest` CLI path both
/// need it, and two inline copies of a path convention drift silently -- the
/// failure being a signature minted with one key and verified against another.
#[cfg(not(target_arch = "wasm32"))]
pub fn default_key_path() -> std::path::PathBuf {
    std::env::var("QUIPU_SIGNING_KEY").map_or_else(
        |_| Path::new(".quipu").join("verifier.pk8"),
        std::path::PathBuf::from,
    )
}

#[cfg(not(target_arch = "wasm32"))]
pub fn load_or_generate(path: &Path) -> Result<Ed25519KeyPair> {
    quipu_signing::load_or_generate(path).map_err(Into::into)
}

#[cfg(test)]
#[path = "signing_shared_tests.rs"]
mod shared_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;

    #[test]
    fn sign_roundtrips_and_rejects_tampering() {
        let rng = SystemRandom::new();
        let doc = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let kp = Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap();
        let pk = public_key_hex(&kp);
        let msg = verdict_message(
            "has-test",
            "sym1",
            "satisfied",
            "fnv1a:ab",
            "committed",
            "quipu",
        );
        let sig = sign_hex(&kp, &msg);
        assert!(
            verify_hex(&pk, &msg, &sig),
            "a genuine signature must verify"
        );
        // tampered message
        let tampered = verdict_message(
            "has-test",
            "sym1",
            "unsatisfied",
            "fnv1a:ab",
            "committed",
            "quipu",
        );
        assert!(
            !verify_hex(&pk, &tampered, &sig),
            "flipping the outcome must break the sig"
        );
        // wrong key
        let doc2 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let kp2 = Ed25519KeyPair::from_pkcs8(doc2.as_ref()).unwrap();
        assert!(
            !verify_hex(&public_key_hex(&kp2), &msg, &sig),
            "a different key must not verify"
        );
        // garbage inputs never panic
        assert!(!verify_hex("zz", &msg, &sig));
        assert!(!verify_hex(&pk, &msg, "not-hex"));
    }

    #[test]
    fn load_or_generate_persists_and_reloads_same_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keys").join("verifier.pk8");
        let kp1 = load_or_generate(&path).unwrap();
        assert!(path.exists(), "key file must be created");
        let pk1 = public_key_hex(&kp1);
        // second load reads the same persisted key
        let kp2 = load_or_generate(&path).unwrap();
        assert_eq!(
            pk1,
            public_key_hex(&kp2),
            "reload must yield the same identity"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "key file must be 0600");
        }
    }
}
