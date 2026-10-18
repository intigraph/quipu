//! Versioned file wire records. Source text is separate from extracted facts.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// Missing is not an explicit null: in particular it must never mean delete.
fn explicit_nullable<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Owner {
    pub deployment: String,
    pub project: String,
    pub crew: String,
}

/// Only the HTTP adapter may construct Named from its verified registry result.
/// Local is explicit repository authority, not proof of a remote crew identity.
#[derive(Clone, Copy)]
pub enum Authority<'a> {
    Named(&'a crate::crew_credentials::CrewPrincipal),
    Local,
    Unproven,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Commit {
    pub owner: Owner,
    pub path: String,
    #[serde(deserialize_with = "explicit_nullable")]
    pub text: Option<String>, // None is an explicit tombstone, never a scan omission.
    #[serde(deserialize_with = "explicit_nullable")]
    pub expected_snapshot: Option<String>,
    #[serde(deserialize_with = "explicit_nullable")]
    pub expected_file: Option<String>,
    pub idempotency_key: String,
    pub harness: String,
    pub account: String, // Opaque selected-account identity, never credentials.
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileHead {
    pub revision: String,
    pub sha256: String,
    pub bytes: usize,
    pub tombstone: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub owner: Owner,
    pub revision: String,
    pub parent: Option<String>,
    pub committed_at: String,
    pub files: BTreeMap<String, FileHead>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub owner: Owner,
    pub snapshot: String,
    pub path: String,
    pub file: FileHead,
    pub request_hash: String,
    pub committed_at: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    pub owner: Owner,
    pub path: String,
    pub parent: Option<String>,
    pub text: Option<String>,
    pub harness: String,
    pub account: String,
    pub committed_at: String,
}

/// A file-data envelope; native filesystem projection is a separate adapter.
#[derive(Serialize, Deserialize)]
pub struct ExportFile {
    pub revision: String,
    #[serde(flatten)]
    pub file: Revision,
}

/// One immutable manifest with its explicitly identified file revisions.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Export {
    pub format: String,
    pub coverage: String,
    pub snapshot: Option<Snapshot>,
    pub files: Vec<ExportFile>,
}
