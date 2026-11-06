use sha2::{Digest, Sha256};

use super::SnapshotHeader;
use crate::{Result, signing::SigningIdentity};

const SCHEMA: &str = "quipu-logical-store-snapshot/v1";
const POLICY: &str = "quipu-logical-store/all-local-data-current-and-future-graphs/v1";

/// A complete logical cut. Its signature is separate from transport access;
/// none of these received fields enrolls a source key or grants write rights.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FullStoreManifest {
    /// Exact supported logical snapshot format.
    pub schema: String,
    /// Whole-store profile, including later graph registration and metadata.
    pub policy: String,
    /// One source snapshot, schema contract, generation and composite cursor.
    pub header: SnapshotHeader,
    /// SHA-256 of the complete ordered JSONL spool, including its header.
    pub spool_sha256: String,
    /// Complete data row count.
    pub rows: u64,
    /// Complete spool byte count.
    pub bytes: u64,
}

impl FullStoreManifest {
    /// Construct this build's explicit profile from a completed logical cut.
    /// Construction does not sign the cut or establish receiver trust.
    #[must_use]
    pub fn new(header: SnapshotHeader, spool_sha256: String, rows: u64, bytes: u64) -> Self {
        Self {
            schema: SCHEMA.into(),
            policy: POLICY.into(),
            header,
            spool_sha256,
            rows,
            bytes,
        }
    }
}

fn canonical(manifest: &FullStoreManifest) -> Result<Vec<u8>> {
    let value =
        serde_json::to_value(manifest).map_err(|e| crate::Error::InvalidValue(e.to_string()))?;
    Ok(crate::replicate_wire::canonical(&value).into_bytes())
}

fn message(manifest: &FullStoreManifest) -> Result<Vec<u8>> {
    let bytes = canonical(manifest)?;
    let mut message = b"quipu-logical-store-snapshot/v1\0".to_vec();
    message.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    message.extend_from_slice(&bytes);
    Ok(message)
}

/// Exact canonical descriptor hash to pin through an independently admitted
/// bootstrap contract, never learned from an untrusted response itself.
///
/// # Errors
/// Manifest serialization failure.
pub fn manifest_hash(manifest: &FullStoreManifest) -> Result<String> {
    Ok(hex::encode(Sha256::digest(canonical(manifest)?)))
}

/// Sign only a completed logical cut using the existing source identity.
/// Does not copy a private key or authorize serving the cut to any principal.
///
/// # Errors
/// Manifest serialization failure.
pub fn sign_snapshot(manifest: &FullStoreManifest, identity: &SigningIdentity) -> Result<String> {
    Ok(identity.sign(&message(manifest)?))
}

/// Out-of-band receiver admission. A response can never supply its own trust.
#[derive(Debug, Clone)]
pub struct SnapshotTrust {
    /// Independently pinned source public key.
    pub public_key_hex: String,
    /// Independently pinned source lineage and epoch.
    pub source: crate::replica_manifest::Source,
    /// Independently pinned journal generation.
    pub journal_generation: String,
    /// Independently admitted exact local table/column/profile contract.
    pub catalog_digest: String,
    /// Independently admitted exact canonical bootstrap manifest hash.
    pub manifest_sha256: String,
}

impl SnapshotTrust {
    /// Check the pinned descriptor, profile and source signature. This alone
    /// does not verify spool bytes or publish data/progress in an Applier.
    #[must_use]
    pub fn accepts(&self, manifest: &FullStoreManifest, signature: &str) -> bool {
        manifest.schema == SCHEMA
            && manifest.policy == POLICY
            && manifest.header.source == self.source
            && manifest.header.journal.generation == self.journal_generation
            && manifest.header.journal.catalog_digest == self.catalog_digest
            && manifest_hash(manifest).is_ok_and(|hash| hash == self.manifest_sha256)
            && message(manifest).is_ok_and(|bytes| {
                crate::signing::verify_hex(&self.public_key_hex, &bytes, signature)
            })
    }
}

fn revision_message(revision: &super::Revision) -> Result<Vec<u8>> {
    let value =
        serde_json::to_value(revision).map_err(|e| crate::Error::InvalidValue(e.to_string()))?;
    let bytes = crate::replicate_wire::canonical(&value).into_bytes();
    if bytes.len() > 8 * 1024 * 1024 || revision.changes.len() > 100_000 {
        return Err(super::invalid(
            "logical revision exceeds signed frame resource cap",
        ));
    }
    let mut message = b"quipu-logical-store-revision/v1\0".to_vec();
    message.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    message.extend_from_slice(&bytes);
    Ok(message)
}

/// Sign a complete committed logical revision, including metadata-only
/// changes and its exact composite source cursor. No permission is granted.
///
/// # Errors
/// Serialization or bounded-frame refusal.
pub fn sign_revision(revision: &super::Revision, identity: &SigningIdentity) -> Result<String> {
    Ok(identity.sign(&revision_message(revision)?))
}

pub(crate) fn revision_hash(revision: &super::Revision) -> Result<String> {
    Ok(hex::encode(Sha256::digest(revision_message(revision)?)))
}

pub(crate) fn verify_revision(revision: &super::Revision, key: &str, signature: &str) -> bool {
    revision_message(revision).is_ok_and(|bytes| crate::signing::verify_hex(key, &bytes, signature))
}
