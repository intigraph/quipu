//! Pure identity planning and locally bound acceptance, shared by merge adapters.
//! Source authors and serialized receipts are never an acceptance authority.
use std::collections::{BTreeMap, BTreeSet, HashSet};

use oxrdf::{NamedNode, NamedOrBlankNode, Term, Triple};
use serde::{Deserialize, Serialize};

use super::interchange::{ExternalRow, ExternalSet, digest, invalid};
use crate::error::Result;

pub mod ledger;

/// Orientation names the incoming endpoint first, retained local endpoint second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Auto,
    SubjectToObject,
    ObjectToSubject,
}

/// Computed by the adapter from current immutable inputs, not a supplied receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub operation: String,
    pub installation: String,
    pub base: String,
    pub ours: String,
    pub theirs: String,
    pub scope: String,
    pub profile: String,
}

/// All rows are displayed, including relations that cannot authorize identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub ordinal: usize,
    pub row_sha256: String,
    pub incoming: String,
    pub retained: String,
    pub direction: Direction,
    pub predicate: String,
    pub identity_eligible: bool,
}

/// A reproducible proposal; its fields carry no authority to execute it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema: String,
    pub context: Context,
    pub mapping_set_id: String,
    pub source_sha256: String,
    pub rows: Vec<Row>,
}

fn entities(graph: &HashSet<Triple>) -> BTreeSet<String> {
    graph
        .iter()
        .filter_map(|t| match &t.subject {
            NamedOrBlankNode::NamedNode(n) => Some(n.as_str().into()),
            NamedOrBlankNode::BlankNode(_) => None,
        })
        .collect()
}

fn schema_iris(graph: &HashSet<Triple>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for t in graph {
        out.insert(t.predicate.as_str().into());
        match &t.object {
            Term::Literal(l) => {
                out.insert(l.datatype().as_str().into());
            }
            Term::NamedNode(n) if t.predicate.as_str() == crate::namespace::RDF_TYPE => {
                out.insert(n.as_str().into());
            }
            _ => {}
        }
    }
    out
}

fn orient(
    row: &ExternalRow,
    ours: &BTreeSet<String>,
    theirs: &BTreeSet<String>,
    direction: Direction,
) -> Result<(String, String, Direction)> {
    let forward = theirs.contains(&row.subject) && ours.contains(&row.object);
    let reverse = theirs.contains(&row.object) && ours.contains(&row.subject);
    let direction = match (direction, forward, reverse) {
        (Direction::Auto, true, false) => Direction::SubjectToObject,
        (Direction::Auto, false, true) => Direction::ObjectToSubject,
        (Direction::SubjectToObject, true, _) => Direction::SubjectToObject,
        (Direction::ObjectToSubject, _, true) => Direction::ObjectToSubject,
        _ => {
            return Err(invalid(format!(
                "row {} has absent or ambiguous endpoints; explicit direction and side membership required",
                row.ordinal
            )));
        }
    };
    let (incoming, retained) = match direction {
        Direction::SubjectToObject => (&row.subject, &row.object),
        Direction::ObjectToSubject => (&row.object, &row.subject),
        Direction::Auto => unreachable!("orientation resolved"),
    };
    if incoming != retained && ours.contains(incoming) {
        return Err(invalid("crosswalk would rename an existing local identity"));
    }
    Ok((incoming.clone(), retained.clone(), direction))
}

impl Plan {
    /// Plan only after original snapshot/envelope verification. Shape vocabulary
    /// IRIs must be supplied by the adapter along with the effective profile.
    ///
    /// # Errors
    /// Ambiguous direction, schema remaps, contradictory or conflicting rows.
    pub fn prepare(
        set: &ExternalSet,
        context: Context,
        ours: &HashSet<Triple>,
        theirs: &HashSet<Triple>,
        shape_iris: &BTreeSet<String>,
        direction: Direction,
    ) -> Result<Self> {
        set.verify_source()?;
        let (ours_entities, theirs_entities) = (entities(ours), entities(theirs));
        let mut schema = schema_iris(ours);
        schema.extend(schema_iris(theirs));
        schema.extend(shape_iris.iter().cloned());
        let mut rows = Vec::new();
        let mut proposed = BTreeMap::new();
        let mut pairs: BTreeMap<(String, String), BTreeSet<bool>> = BTreeMap::new();
        for row in &set.rows {
            let (incoming, retained, direction) =
                orient(row, &ours_entities, &theirs_entities, direction)?;
            let pair = if incoming <= retained {
                (incoming.clone(), retained.clone())
            } else {
                (retained.clone(), incoming.clone())
            };
            pairs.entry(pair).or_default().insert(row.negated);
            if row.identity_eligible() {
                if schema.contains(&incoming) || schema.contains(&retained) {
                    return Err(invalid(
                        "predicate, class, datatype or shape vocabulary remap refused",
                    ));
                }
                if proposed
                    .insert(incoming.clone(), retained.clone())
                    .is_some_and(|old| old != retained)
                {
                    return Err(invalid(
                        "one incoming identity has conflicting retained targets",
                    ));
                }
            }
            rows.push(Row {
                ordinal: row.ordinal,
                row_sha256: row.row_sha256.clone(),
                incoming,
                retained,
                direction,
                predicate: row.predicate.clone(),
                identity_eligible: row.identity_eligible(),
            });
        }
        if pairs.values().any(|states| states.len() > 1) {
            return Err(invalid("contradictory positive/negative mapping rows"));
        }
        if proposed
            .values()
            .any(|target| proposed.contains_key(target))
        {
            return Err(invalid(
                "mapping chains, cycles or self mappings are not supported",
            ));
        }
        Ok(Self {
            schema: "https://quipu.dev/sssom-crosswalk/v1".into(),
            context,
            mapping_set_id: set.mapping_set_id.clone(),
            source_sha256: set.source_sha256.clone(),
            rows,
        })
    }

    /// Canonical proposal identity. It contains every context/row/direction binding.
    ///
    /// # Errors
    /// A field cannot be serialized to canonical JSON.
    pub fn sha256(&self) -> Result<String> {
        digest(self)
    }

    /// Row acceptance key is bound to the entire plan, not just its endpoints.
    pub(crate) fn row_key(plan_sha256: &str, row: &Row) -> Result<String> {
        digest(&("quipu-sssom-acceptance-v1", plan_sha256, row))
    }
}

/// Identity projection is called only with replacements returned by the local
/// ledger verifier; literals, predicates, blank node identity and scopes survive.
///
/// # Errors
/// A replacement is not an absolute IRI. This pure utility does not authorize
/// a store write; adapters must independently reserve and verify local acceptance.
pub fn project(
    graph: &HashSet<Triple>,
    replacements: &BTreeMap<String, String>,
) -> Result<HashSet<Triple>> {
    graph
        .iter()
        .map(|t| {
            let mut t = t.clone();
            if let NamedOrBlankNode::NamedNode(n) = &t.subject
                && let Some(iri) = replacements.get(n.as_str())
            {
                t.subject = NamedNode::new(iri).map_err(invalid)?.into();
            }
            if let Term::NamedNode(n) = &t.object
                && let Some(iri) = replacements.get(n.as_str())
            {
                t.object = NamedNode::new(iri).map_err(invalid)?.into();
            }
            Ok(t)
        })
        .collect()
}

#[cfg(all(test, unix))]
mod tests;
