//! Metadata-only eligibility observation for an exact MAIN graph scope
//! (aegis-ehf G5).
//!
//! An observation says whether each named graph would pass the direct-main
//! acquisition admission and the store's configured label floor AT ONE
//! OBSERVED HEAD. It is not an acquisition, a receipt, an applied-pin policy
//! proof, or authority to acquire: its schema is distinct and it carries
//! `observed_head_tx`, never `applied_tx`.
//!
//! It reads only `main.terms`, `main.graphs`, the dataset registry, transaction
//! ids, store identity and cached graph labels. It never reads facts, never
//! writes, never touches the composed acquisition topology or anchors, and a
//! configured trust- or time-derived floor refuses before any label read
//! (the same guard `main_graph::metadata` applies to acquisition).

use std::collections::{BTreeMap, BTreeSet};

use oxrdf::NamedNode;
use serde::{Deserialize, Serialize};

use super::{invalid, main_graph};
use crate::{Error, Result, Store};

/// Distinct from every acquisition contract; consumers must recognize it.
pub const CONTRACT: &str = "quipu-eligibility/v1";

/// What an observation is, stated in every response so it cannot be mistaken
/// for an acquisition receipt.
pub const AUTHORITY: &str = "observation-only: current policy at one observed head; \
     not an acquisition receipt, applied-pin policy proof, or authority to acquire";

/// Exact graph selection; no pin, because eligibility is observed at the head.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Expected main lineage; a mismatch refuses.
    pub source_store_id: String,
    /// Exact registered named graph IRIs.
    pub graphs: Vec<String>,
    /// When given, refuse unless the observed head is exactly this one.
    #[serde(default, deserialize_with = "super::strict_tx")]
    pub expected_head_tx: Option<i64>,
}

/// Per-graph outcome. `Refused` is a definite answer from admission or policy;
/// `Unknown` means the check could not complete and is NOT eligibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Status {
    /// Passes direct-main admission and the configured floor.
    Eligible,
    /// Refused by scope admission or the label floor.
    Refused,
    /// The observation failed; eligibility is unknown.
    Unknown,
}

/// One graph's outcome and its reason (refusal or failure text).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphEligibility {
    /// Outcome.
    pub status: Status,
    /// Why it was refused or unknown; absent when eligible.
    pub reason: Option<String>,
}

/// Sanitized identity of the configured label floor: which axes are set and a
/// digest of their values, never the values themselves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyIdentity {
    /// SHA-256 of the canonical floor; equal digests mean equal floors.
    pub sha256: String,
    /// Configured axes, sorted: `data_kinds`, `freshness`, `policy_tokens`, `trust`.
    pub floors: Vec<String>,
}

/// The observation. It exists only if the head and policy were unchanged
/// across it; a graph whose check failed is `Unknown`, never eligible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    /// [`CONTRACT`].
    pub schema: String,
    /// [`AUTHORITY`].
    pub authority: String,
    /// Main lineage observed inside the read transaction.
    pub source_store_id: String,
    /// Committed main head observed before and after the graph checks.
    pub observed_head_tx: i64,
    /// Configured label floor, sanitized.
    pub policy: PolicyIdentity,
    /// Compiled producer version.
    pub producer_version: String,
    /// Compiled source commit.
    pub producer_git_sha: String,
    /// Whether the producer was built from modified source.
    pub producer_git_dirty: bool,
    /// Every requested graph's outcome.
    pub graphs: BTreeMap<String, GraphEligibility>,
}

fn policy_identity(store: &Store) -> Result<PolicyIdentity> {
    let floor = store.labels_config();
    let canonical = serde_json::json!({
        "min_freshness": floor.min_freshness,
        "min_trust_rank": floor.min_trust_rank,
        "min_trust_chain": floor.min_trust_chain,
        "deny_policy_tokens": floor.deny_policy_tokens.iter().collect::<BTreeSet<_>>(),
        "deny_data_kinds": floor.deny_data_kinds.iter().collect::<BTreeSet<_>>(),
    });
    let bytes = serde_json::to_vec(&canonical).map_err(|e| invalid(e.to_string()))?;
    let mut floors = Vec::new();
    if !floor.deny_data_kinds.is_empty() {
        floors.push("data_kinds".to_string());
    }
    if floor.min_freshness.is_some() {
        floors.push("freshness".to_string());
    }
    if !floor.deny_policy_tokens.is_empty() {
        floors.push("policy_tokens".to_string());
    }
    if floor.min_trust_rank.is_some() || floor.min_trust_chain.is_some() {
        floors.push("trust".to_string());
    }
    Ok(PolicyIdentity {
        sha256: crate::share::sha256(&bytes),
        floors,
    })
}

/// Observe eligibility in one read transaction on `store`'s connection.
///
/// The caller supplies the connection and is responsible for holding it
/// without side effects (the server uses a dedicated writer guard that does
/// not settle attestations or synchronize providers).
///
/// # Errors
/// Refuses a malformed or mismatched request, a moved or unexpected head, and
/// an expired deadline; per-graph refusals and failures are reported in the
/// observation, not as errors.
pub fn observe(store: &Store, request: &Request) -> Result<Observation> {
    let started = crate::time::Stopwatch::start();
    let check_deadline = || {
        if let Some(deadline) =
            crate::time::request_deadline().filter(crate::time::Deadline::passed)
        {
            return Err(Error::QueryTimeout {
                elapsed_ms: started.elapsed_ms(),
                limit_ms: deadline.millis_from(&started),
            });
        }
        Ok(())
    };
    check_deadline()?;
    if request.graphs.is_empty() || request.graphs.len() > 16 {
        return Err(invalid("eligibility requires 1..=16 explicit named graphs"));
    }
    let mut seen = BTreeSet::new();
    for iri in &request.graphs {
        NamedNode::new(iri.clone()).map_err(|e| invalid(format!("graph IRI: {e}")))?;
        if !seen.insert(iri) {
            return Err(invalid("duplicate eligibility graph"));
        }
    }
    let read = store.conn.unchecked_transaction()?;
    let source_store_id = store.store_id()?;
    if source_store_id != request.source_store_id {
        return Err(invalid("eligibility source store identity mismatch"));
    }
    let head = store.latest_tx_id()?;
    if head == 0 {
        return Err(invalid("eligibility source has no committed transaction"));
    }
    if request
        .expected_head_tx
        .is_some_and(|expected| expected != head)
    {
        return Err(invalid(
            "eligibility observed head differs from the expected head",
        ));
    }
    let policy = policy_identity(store)?;
    let mut graphs = BTreeMap::new();
    for iri in &request.graphs {
        check_deadline()?;
        let outcome = match main_graph::metadata(store, iri) {
            Ok(_) => GraphEligibility {
                status: Status::Eligible,
                reason: None,
            },
            Err(Error::InvalidValue(reason) | Error::PolicyDenied(reason)) => GraphEligibility {
                status: Status::Refused,
                reason: Some(reason),
            },
            Err(other @ Error::QueryTimeout { .. }) => return Err(other),
            Err(other) => GraphEligibility {
                status: Status::Unknown,
                reason: Some(other.to_string()),
            },
        };
        graphs.insert(iri.clone(), outcome);
    }
    if store.latest_tx_id()? != head || policy_identity(store)? != policy {
        return Err(invalid(
            "eligibility head or policy changed during the observation",
        ));
    }
    check_deadline()?;
    read.commit()?;
    Ok(Observation {
        schema: CONTRACT.into(),
        authority: AUTHORITY.into(),
        source_store_id,
        observed_head_tx: head,
        policy,
        producer_version: env!("CARGO_PKG_VERSION").into(),
        producer_git_sha: env!("QUIPU_GIT_SHA").into(),
        producer_git_dirty: env!("QUIPU_GIT_DIRTY") == "true",
        graphs,
    })
}

#[cfg(test)]
#[path = "eligibility_tests.rs"]
mod tests;
