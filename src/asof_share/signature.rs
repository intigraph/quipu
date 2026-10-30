//! A bootstrap manifest is authenticated independently of its transport bearer.
use super::{Manifest, manifest_bytes};
use crate::{Result, signing::SigningIdentity};

/// Domain-separated, length-delimited canonical manifest bytes. The manifest
/// includes lineage, epoch, scope, policy, watermark, shapes and graph digests.
///
/// # Errors
/// Manifest serialization failure.
pub fn manifest_message(manifest: &Manifest) -> Result<Vec<u8>> {
    let bytes = manifest_bytes(manifest)?;
    let mut message = b"quipu-asof-manifest/v1\0".to_vec();
    message.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    message.extend_from_slice(&bytes);
    Ok(message)
}

/// Sign a completed cut with the source's existing stream identity. This
/// neither exports the private key nor establishes trust in that identity.
///
/// # Errors
/// Manifest serialization failure.
pub fn sign_manifest(manifest: &Manifest, identity: &SigningIdentity) -> Result<String> {
    Ok(identity.sign(&manifest_message(manifest)?))
}

/// Check the manifest under a public key pinned outside the received frame.
/// Malformed signatures or keys fail closed.
#[must_use]
pub fn verify_manifest(manifest: &Manifest, public_key_hex: &str, signature: &str) -> bool {
    manifest_message(manifest)
        .is_ok_and(|message| crate::signing::verify_hex(public_key_hex, &message, signature))
}
