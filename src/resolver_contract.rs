//! Content-bound prospective resolver contract, without Store or planner access.
//!
//! This is a wire/evidence boundary, not admission authority. A producer supplies
//! the actual staged graph, registry/cache snapshots and validator settings.
//! See `docs/design/resolver-contract.md` for the framing and trust boundary.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::Result;

mod admission;
mod encoding;
mod validation;

/// SHA-256 of bytes, serialized as 32 unsigned octets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentDigest(pub [u8; 32]);

/// A retained timestamp or a time that only master admission may assign.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ProspectiveTime {
    RetainedUtc(String),
    AdmissionAssigned,
    /// Current admission's assigned time; retain its origin in the encoding.
    AdmissionResolved(String),
}

/// Canonical N-Triples terms without the final dot; no blank nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProspectiveRow {
    pub triple: String,
    pub valid_from: ProspectiveTime,
    pub valid_to: Option<ProspectiveTime>,
}

/// An exact canonical subject/predicate pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    pub subject: String,
    pub predicate: String,
}

/// Immutable input identity. `None` scope means the entire bound graph;
/// `Some([])` means an explicitly empty scope, never the entire graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceBinding {
    pub store_id: String,
    pub log_epoch: String,
    pub base_tx: u64,
    pub graph: String,
    pub base_slice_hash: ContentDigest,
    pub scope: Option<Vec<Slot>>,
    /// Ordered exact input artifacts (base, ours, theirs, decisions, etc.).
    pub inputs: Vec<NamedContent>,
}

/// Exact named bytes plus their registry/source version (e.g. `loaded_at`).
/// Version is an opaque binding, not a timestamp interpreted by this encoder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedContent {
    pub name: String,
    pub version: String,
    pub content: Vec<u8>,
}

/// Ordered decision evidence, distinct from the final nonempty data rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProspectiveDecision {
    pub slot: Slot,
    pub choice: DecisionChoice,
    pub operations: Vec<ProspectiveOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DecisionChoice {
    Base,
    Ours,
    Theirs,
    Both,
    Continue,
    Defer,
    Values(Vec<String>),
}

/// Receive/decide sequence remains explicit; a receipt is never a `BaseRow`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ProspectiveOperation {
    Assert(ProspectiveRow),
    Close {
        triple: String,
        at: ProspectiveTime,
    },
    /// The admission writer records `[at, at)` as history, not live data.
    Received {
        triple: String,
        at: ProspectiveTime,
    },
}

/// Final prospective rows plus ordered decisions, bound to one validator manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProspectiveCandidate {
    pub source: SourceBinding,
    pub rows: Vec<ProspectiveRow>,
    pub decisions: Vec<ProspectiveDecision>,
    pub validator: ContentDigest,
}

/// Implementation identity and exact options; no implicit engine profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidatorImplementation {
    pub artifact: ContentDigest,
    pub dependency_lock: ContentDigest,
    pub compiled_features: BTreeSet<String>,
    /// Include engine/version, parser mode, base IRIs and validation mode.
    pub options: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextCoverage {
    Complete,
    Unknown,
    BudgetExhausted,
}

/// The incremental gate is separate from full staged-graph validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IncrementalContext {
    pub conditional: bool,
    pub selected_shapes: Vec<NamedContent>,
    pub write_shapes: Option<Vec<u8>>,
    pub facts: Vec<String>,
    pub selection_policy: String,
    pub value_node_budget: u64,
    pub fact_budget: u64,
    /// Exact implementation contract, including optional value-context fallback.
    pub fallback_contract: String,
    pub coverage: ContextCoverage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidatorFlags {
    pub shacl_config: bool,
    pub owl_validate_on_write: bool,
    pub recording_verdicts: bool,
    /// Separate postcommit work; never evidence of precommit inference.
    pub postcommit_reactive_materialize: bool,
}

/// Actual validator inputs. Registry order is the producer's `ORDER BY name`.
/// `combined_shapes` must be the exact two-newline join. Ontologies use one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidatorManifest {
    pub implementation: ValidatorImplementation,
    pub loaded_shapes: Vec<NamedContent>,
    pub combined_shapes: Vec<u8>,
    pub loaded_ontologies: Vec<NamedContent>,
    pub combined_ontologies: Vec<u8>,
    /// SHA-256 of the *actual cache's source bytes*, from a proved snapshot.
    /// None means freshness/equivalence is unproved, not an empty ontology.
    pub ontology_cache_source: Option<ContentDigest>,
    /// Exact renderer bytes for the complete staged graph, including inference.
    pub staged_graph: Vec<u8>,
    pub incremental: IncrementalContext,
    pub flags: ValidatorFlags,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationOutcome {
    Conforms,
    Violates,
    Unknown,
    RequiresAdmissionTime,
}

/// Offline evidence only. No assigned transaction or accepted-at is invented.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationEvidence {
    pub candidate: ContentDigest,
    pub validator: ContentDigest,
    pub outcome: ValidationOutcome,
}

/// Coordinates supplied by the admission writer, never assigned offline here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionCoordinates {
    pub accepted_at: String,
    pub transaction: u64,
}

/// Embed this binding in the existing authenticated master receipt. These
/// bytes alone are neither a signature nor a claim that a transaction committed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionValidation {
    pub evidence: ValidationEvidence,
    pub master: AdmissionCoordinates,
}

impl AdmissionValidation {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        encoding::encode(b"quipu-resolver-admission-validation-v1\0", self)
    }
}

impl ValidatorManifest {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        validation::manifest(self)?;
        encoding::encode(b"quipu-resolver-validator-manifest-v1\0", self)
    }

    pub fn identity(&self) -> Result<ContentDigest> {
        Ok(encoding::digest(&self.canonical_bytes()?))
    }
}

impl ProspectiveCandidate {
    /// Substitute a writer-supplied time, then recheck intervals. This does not
    /// run validation, reserve a writer, obtain the time or commit anything.
    pub fn assign_admission_time(&self, actual_accepted_at: &str) -> Result<Self> {
        admission::assign(self, actual_accepted_at)
    }

    /// Bind a fresh post-assignment conforming result to actual writer coordinates.
    /// Normal admission must authenticate/commit/sign this through its own gates.
    pub fn bind_admission_validation(
        &self,
        manifest: &ValidatorManifest,
        observed: ValidationOutcome,
        actual: AdmissionCoordinates,
    ) -> Result<AdmissionValidation> {
        admission::bind(self, manifest, observed, actual)
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        validation::candidate(self)?;
        encoding::encode(b"quipu-resolver-candidate-v1\0", self)
    }

    pub fn identity(&self) -> Result<ContentDigest> {
        Ok(encoding::digest(&self.canonical_bytes()?))
    }

    /// Bind an external validator result without promoting incomplete evidence.
    /// This neither runs the validator nor authenticates its producer.
    pub fn bind_validation(
        &self,
        manifest: &ValidatorManifest,
        observed: ValidationOutcome,
    ) -> Result<ValidationEvidence> {
        validation::evidence(self, manifest, observed)
    }
}

#[cfg(test)]
mod tests;
