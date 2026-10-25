//! Shared v1 verdict message, Ed25519 encoding and host-file custody primitives.
//!
//! This crate provides cryptography, not registration or authorization. Keys stay
//! with each caller. Signature validity alone never establishes trust.
#![forbid(unsafe_code)]

pub mod error;

#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

#[cfg(not(target_arch = "wasm32"))]
use ring::rand::SystemRandom;
use ring::signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey};

#[cfg(not(target_arch = "wasm32"))]
use crate::error::{Error, Result};

/// Load a PKCS#8 keypair, or generate and persist a new host-file key if absent.
/// The caller supplies its own custody path; this crate resolves no environment
/// variables and never selects another application's identity. On Unix a new
/// key is set to mode 0600, preserving the existing v1 custody behavior.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_or_generate(path: &Path) -> Result<Ed25519KeyPair> {
    let pkcs8: Vec<u8> = if path.exists() {
        std::fs::read(path).map_err(|e| Error::Store(format!("read signing key: {e}")))?
    } else {
        let rng = SystemRandom::new();
        let doc = Ed25519KeyPair::generate_pkcs8(&rng)
            .map_err(|_| Error::Store("generate signing key".into()))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::Store(format!("mkdir for signing key: {e}")))?;
        }
        std::fs::write(path, doc.as_ref())
            .map_err(|e| Error::Store(format!("write signing key: {e}")))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        doc.as_ref().to_vec()
    };
    Ed25519KeyPair::from_pkcs8(&pkcs8)
        .map_err(|_| Error::Store("parse signing key (bad PKCS#8)".into()))
}

/// Hex-encoded public key of a keypair (goes into the registry so a human can
/// register this verifier's identity, and into a verdict so it round-trips).
#[must_use]
pub fn public_key_hex(keypair: &Ed25519KeyPair) -> String {
    hex::encode(keypair.public_key().as_ref())
}

/// Sign `message`, returning a hex-encoded ed25519 signature.
#[must_use]
pub fn sign_hex(keypair: &Ed25519KeyPair, message: &[u8]) -> String {
    hex::encode(keypair.sign(message).as_ref())
}

/// Verify a hex signature over `message` against a hex ed25519 public key.
/// Returns false on any decode/verify failure (never panics).
#[must_use]
pub fn verify_hex(public_key_hex: &str, message: &[u8], signature_hex: &str) -> bool {
    let (Ok(pk), Ok(sig)) = (hex::decode(public_key_hex), hex::decode(signature_hex)) else {
        return false;
    };
    UnparsedPublicKey::new(&ED25519, pk)
        .verify(message, &sig)
        .is_ok()
}

/// The canonical byte string signed for a verdict. Deterministic field order so
/// the signature is reproducible: any verifier re-derives the exact same message
/// from the verdict fields and checks the signature (checked, not trusted).
#[must_use]
pub fn verdict_message(
    predicate_id: &str,
    target_ref: &str,
    outcome: &str,
    evidence_hash: &str,
    tier: &str,
    verifier: &str,
) -> Vec<u8> {
    format!("v1|{predicate_id}|{target_ref}|{outcome}|{evidence_hash}|{tier}|{verifier}")
        .into_bytes()
}

/// SHA-256 evidence hash sealed by the v1 message, with the `sha256:` prefix.
#[must_use]
pub fn evidence_hash(evidence: &str) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "sha256:{}",
        hex::encode(Sha256::digest(evidence.as_bytes()))
    )
}

/// A loaded signing identity: the keypair plus the verifier name it attests as.
/// Held by the `Store` so `policy_check` can sign verdicts.
pub struct SigningIdentity {
    keypair: Ed25519KeyPair,
    /// The verifier name this identity attests as (v1: "quipu").
    pub verifier: String,
}

impl SigningIdentity {
    #[must_use]
    pub fn new(keypair: Ed25519KeyPair, verifier: impl Into<String>) -> Self {
        Self {
            keypair,
            verifier: verifier.into(),
        }
    }

    /// Load (or generate) a host-file key and wrap it as `verifier`.
    /// On wasm there is no host file — construct via [`SigningIdentity::new`]
    /// from key bytes instead.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load(path: &Path, verifier: impl Into<String>) -> Result<Self> {
        Ok(Self::new(load_or_generate(path)?, verifier))
    }

    #[must_use]
    pub fn public_key_hex(&self) -> String {
        public_key_hex(&self.keypair)
    }

    /// Sign an arbitrary canonical message (hex signature). For records whose
    /// message is defined next to the record — the denial quarantine's seal —
    /// rather than here; a verdict signs through [`Self::sign_verdict`].
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> String {
        sign_hex(&self.keypair, message)
    }

    /// Sign a verdict, returning its hex signature over the canonical message.
    #[must_use]
    pub fn sign_verdict(
        &self,
        predicate_id: &str,
        target_ref: &str,
        outcome: &str,
        evidence_hash: &str,
        tier: &str,
    ) -> String {
        let msg = verdict_message(
            predicate_id,
            target_ref,
            outcome,
            evidence_hash,
            tier,
            &self.verifier,
        );
        sign_hex(&self.keypair, &msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
