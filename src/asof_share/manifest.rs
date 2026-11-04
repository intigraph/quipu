//! The as-of share's manifest types.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Contract version of the manifest.
pub const SCHEMA: &str = "quipu-asof-share/v1";

/// Read policy identity of the supported scoped cut.
pub const POLICY: &str = "quipu-asof-policy/write-class-principal/v1";

/// Portable scope digest used by cuts, stream sessions and client proposals.
/// The lists are tagged and sorted; context never becomes writable scope.
#[must_use]
pub fn scope_digest(scope: &[String], context: &[String]) -> String {
    use sha2::{Digest, Sha256};
    let mut scope = scope.to_vec();
    let mut context = context.to_vec();
    scope.sort();
    context.sort();
    let text = crate::replicate_wire::canonical(
        &serde_json::json!({"scope": scope, "context_graphs": context}),
    );
    format!("sha256:{}", hex::encode(Sha256::digest(text.as_bytes())))
}

#[path = "signature.rs"]
mod signature;
pub use signature::{manifest_message, sign_manifest, verify_manifest};

/// Compact UTF-8 JSON with recursively sorted keys, independent of target or
/// `serde_json` map feature flags.
///
/// # Errors
/// Serialization failure.
pub fn manifest_bytes(manifest: &Manifest) -> crate::Result<Vec<u8>> {
    let value = serde_json::to_value(manifest)
        .map_err(|error| crate::Error::InvalidValue(error.to_string()))?;
    Ok(crate::replicate_wire::canonical(&value).into_bytes())
}

/// The source log a follower ties the cut to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Persistent source lineage.
    pub store_id: String,
    /// Current log epoch; a change means re-bootstrap, never splice.
    pub log_epoch: String,
}

/// What the cut holds for one graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphCompleteness {
    /// Live facts at `T`, one record each.
    pub records: usize,
    /// The applier's `share_graph_digest`: SHA-256 over the graph's records
    /// as canonical compact JSON lines, sorted and joined by `\n` (no
    /// trailing newline), with a `sha256:` prefix. Send order does not reach
    /// it.
    pub digest: String,
    /// The frozen pack the records were read from; always null until frozen
    /// graphs can be cut (aegis-s24.1.1).
    pub frozen: Option<serde_json::Value>,
}

/// What the cut can and cannot carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Completeness {
    /// Records carry `valid_from` and `valid_to`.
    pub valid_time: bool,
    /// How byte values are carried.
    pub bytes: String,
    /// Every graph in the cut.
    pub graphs: BTreeMap<String, GraphCompleteness>,
}

/// Build that produced the cut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Producer {
    /// Compiled version.
    pub version: String,
    /// Compiled source commit.
    pub git_sha: String,
    /// Whether the build had modified source.
    pub git_dirty: bool,
}

/// The cut's manifest. Its hash names the spool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// [`SCHEMA`].
    pub schema: String,
    /// Source identity read inside the cut.
    pub source: Source,
    /// Last source transaction included; the follower tails from it.
    pub watermark_tx: i64,
    /// Committed head when the cut was taken.
    pub source_head_tx: i64,
    /// Requested scope, sorted.
    pub scope: Vec<String>,
    /// Declared context graphs, sorted.
    pub context_graphs: Vec<String>,
    /// Digest of the scope and context lists.
    pub scope_digest: String,
    /// SHA-256 of the read rule the cut was served under ([`POLICY`]).
    pub policy_digest: String,
    /// SHA-256 of the shape sets in force at `T`, in the share encoding.
    pub shapes_hash: String,
    /// Per-graph record counts and digests.
    pub completeness: Completeness,
    /// Producing build.
    pub producer: Producer,
}
