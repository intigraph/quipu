//! Separate read-only proposal/decision and destructive apply transports.
//! Declared decision authors are provenance, not authenticated credentials.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::shape_mapping::ShapeMapping;
use crate::shape_mapping::catalog::{MappingProposal, RowDecision};
use crate::{Error, Result, Store};

fn input<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|error| Error::InvalidValue(format!("mapping input: {error}")))
}

fn output<T: Serialize>(value: T) -> Result<Value> {
    serde_json::to_value(value).map_err(|error| Error::Serialization(error.to_string()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Propose {
    mapping: ShapeMapping,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Decide {
    proposal: MappingProposal,
    reviewer: String,
    decisions: Vec<RowDecision>,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Apply {
    Apply {
        mapping: ShapeMapping,
        expected_digest: String,
        actor: String,
        operations_graph: Option<String>,
    },
    Retire {
        mapping: String,
        expected_digest: String,
        actor: String,
    },
}

/// READ. Qualify explicit candidate correspondences against registered scope
/// and exact current pins. Writes no graph, decision or transaction.
pub fn tool_mapping_propose(store: &Store, value: &Value) -> Result<Value> {
    let Propose { mapping } = input(value)?;
    output(store.propose_shape_mapping(mapping)?)
}

/// READ. Pure full-row decision transformation bound to the proposal digest.
pub fn tool_mapping_decide(value: &Value) -> Result<Value> {
    let Decide {
        proposal,
        reviewer,
        decisions,
    } = input(value)?;
    output(proposal.decide(&reviewer, decisions)?)
}

/// WRITE. Only this transport applies or retires a mapping. It cannot create
/// graphs, issue a grant, suppress validation or supply a forged authority IRI.
pub fn tool_mapping_apply(store: &mut Store, value: &Value) -> Result<Value> {
    match input(value)? {
        Apply::Apply {
            mapping,
            expected_digest,
            actor,
            operations_graph,
        } => output(store.apply_shape_mapping(
            mapping,
            &expected_digest,
            &actor,
            operations_graph.as_deref(),
        )?),
        Apply::Retire {
            mapping,
            expected_digest,
            actor,
        } => output(store.retire_shape_mapping(&mapping, &expected_digest, &actor)?),
    }
}
