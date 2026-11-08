//! Receiver-side blast-radius evidence for data merges.
//!
//! This planner is read-only. It never treats absent query/dependency coverage
//! as a zero count; adapters must supply complete context before auto-apply.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::change_risk::{Risk, Verdict, score};
use crate::error::{Error, Result};
use crate::share_merge::{Graph, parse_graph};
use crate::store::Store;

#[path = "merge_plan_scan.rs"]
mod scan;

const TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const SUBCLASS: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
const MAX_TRIPLES: usize = 50_000;
const MAX_DEPENDENTS: usize = 10_000;

/// Receiver-owned policy. It is never decoded from an incoming merge file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// Automatic application requires a score at or below this value.
    pub threshold: u8,
    /// Classes whose instances must always be decided by a person.
    pub protected_classes: BTreeSet<String>,
    /// Schema/policy/credential terms that automatic writes cannot touch.
    pub protected_terms: BTreeSet<String>,
    /// Concrete receiver-admitted recall probes; None means coverage UNKNOWN.
    pub recall_queries: Option<BTreeMap<String, String>>,
}

/// A stored query with different complete answers.
#[derive(Debug, Serialize)]
pub struct QueryChange {
    /// Stored query name.
    pub name: String,
    /// Canonical before-answer digest.
    pub before: String,
    /// Canonical after-answer digest.
    pub after: String,
}

/// A graph change and all limitations on its automatic application.
#[derive(Debug, Serialize)]
pub struct MergePlan {
    /// Versioned evidence representation.
    pub schema: &'static str,
    /// Store and transaction the receiver actually read.
    pub store_id: String,
    /// Transaction head the receiver actually read.
    pub root_tx: i64,
    /// Exact before/after graph hashes; no sender-reported counts are accepted.
    pub before_hash: String,
    /// Hash of the candidate graph.
    pub after_hash: String,
    /// Receiver policy digest.
    pub policy_hash: String,
    /// Active shapes digest.
    pub shapes_hash: String,
    /// Query definitions, scopes, parameter specs and defaults digest.
    pub query_catalog_hash: String,
    /// Actual writer operations when planned at a persistence boundary.
    pub operation_hash: Option<String>,
    /// Intended destinations, separate from referenced context graphs.
    pub write_graphs: BTreeSet<String>,
    /// Entities and predicates with a net change.
    pub changed_slots: BTreeSet<(String, String)>,
    /// Reverse reference closure across both graph states.
    pub dependents: BTreeSet<String>,
    /// Protected entities/terms implicated in the candidate change.
    pub protected: BTreeSet<String>,
    /// Exact changed answers for the qualified query surface.
    pub changed_queries: Vec<QueryChange>,
    /// Pre-existing SHACL issue keys.
    pub preexisting_issues: usize,
    /// SHACL issue keys introduced by this data change.
    pub newly_failing_issues: usize,
    /// Distinct focus nodes with newly failing SHACL issues.
    pub newly_failing_instances: usize,
    /// Registered live context not yet proved unaffected; ROOT is the destination.
    pub named_graphs: Vec<String>,
    /// Registered frozen snapshots, retained read-only.
    pub snapshots: Vec<String>,
    /// Reasons the automatic result is not determined.
    pub undetermined: Vec<String>,
    /// Shared schema/data risk vocabulary and exit-code semantics.
    pub risk: Risk,
}

fn digest(value: &[u8]) -> String {
    crate::share::sha256(value)
}

fn types(graph: &Graph) -> BTreeMap<String, BTreeSet<String>> {
    let mut result = BTreeMap::<String, BTreeSet<String>>::new();
    for triple in graph {
        if triple.predicate.as_str() == TYPE {
            result
                .entry(triple.subject.to_string())
                .or_default()
                .insert(triple.object.to_string());
        }
    }
    result
}

pub(crate) fn protected(
    before: &Graph,
    after: &Graph,
    slots: &BTreeSet<(String, String)>,
    policy: &Policy,
) -> BTreeSet<String> {
    let mut supers = BTreeMap::<String, BTreeSet<String>>::new();
    for triple in before.iter().chain(after) {
        if triple.predicate.as_str() == SUBCLASS {
            supers
                .entry(triple.subject.to_string())
                .or_default()
                .insert(triple.object.to_string());
        }
    }
    let mut result = BTreeSet::new();
    // BOTH states matter: deleting rdf:type must not remove protection.
    for state in [before, after] {
        let types = types(state);
        for (subject, predicate) in slots {
            if policy.protected_terms.contains(predicate)
                || policy
                    .protected_terms
                    .contains(subject.trim_matches(['<', '>']))
            {
                result.insert(subject.clone());
            }
            let mut seen = BTreeSet::new();
            let mut todo = types.get(subject).cloned().unwrap_or_default();
            while let Some(class) = todo.pop_first() {
                if !seen.insert(class.clone()) {
                    continue;
                }
                if policy
                    .protected_classes
                    .contains(class.trim_matches(['<', '>']))
                {
                    result.insert(subject.clone());
                }
                todo.extend(supers.get(&class).into_iter().flatten().cloned());
            }
        }
    }
    result
}

/// Compute a conservative plan without writing the supplied store.
///
/// Unsupported operators or incomplete graph/recall coverage produce an
/// undetermined verdict. Configured adapters recompute before any persistence.
///
/// # Errors
/// Invalid RDF or a failed store identity/transaction read.
pub fn plan(store: &Store, before: &str, after: &str, policy: &Policy) -> Result<MergePlan> {
    plan_touched(store, before, after, policy, &BTreeSet::new())
}

pub(crate) fn plan_touched(
    store: &Store,
    before: &str,
    after: &str,
    policy: &Policy,
    touched: &BTreeSet<(String, String)>,
) -> Result<MergePlan> {
    let (before, after) = (
        parse_graph(before, "merge plan before")?,
        parse_graph(after, "merge plan after")?,
    );
    let mut changed_slots: BTreeSet<_> = before
        .symmetric_difference(&after)
        .map(|triple| {
            (
                triple.subject.to_string(),
                triple.predicate.as_str().to_owned(),
            )
        })
        .collect();
    changed_slots.extend(touched.iter().cloned());
    let mut undetermined = Vec::new();
    if policy.recall_queries.is_none() {
        undetermined.push("recall probe catalog is not admitted".into());
    }
    let graphs = store.list_graphs(None, None)?;
    if graphs.iter().any(|g| g.g != crate::schema::ROOT_GRAPH) {
        undetermined.push("named graph/snapshot dependency context needs qualification".into());
    }
    let mut changed_queries = Vec::new();
    let mut preexisting_issues = 0;
    let mut newly_failing_issues = 0;
    let mut newly_failing_instances = 0;
    if before.len().saturating_add(after.len()) <= MAX_TRIPLES {
        match (scan::scratch(&before), scan::scratch(&after)) {
            (Ok(left), Ok(right)) => {
                match scan::queries(store, &left, &right, policy.recall_queries.as_ref()) {
                    Ok(changed) => changed_queries = changed,
                    Err(error) => undetermined.push(format!("query scan: {error}")),
                }
            }
            _ => undetermined.push("isolated query graph construction failed".into()),
        }
        match scan::issues(
            &store.get_combined_shapes()?.unwrap_or_default(),
            &before,
            &after,
        ) {
            Ok((old, new, instances)) => {
                preexisting_issues = old;
                newly_failing_issues = new;
                newly_failing_instances = instances;
            }
            Err(error) => undetermined.push(format!("SHACL scan: {error}")),
        }
    } else {
        undetermined.push("before/after scans exceed the triple bound".into());
    }
    if before
        .iter()
        .chain(&after)
        .any(|t| t.subject.to_string().starts_with("_:") || t.object.to_string().starts_with("_:"))
    {
        undetermined.push("blank-node query identity needs qualification".into());
    }
    if policy.threshold > 100
        || policy.protected_classes.is_empty()
        || policy
            .protected_classes
            .iter()
            .chain(&policy.protected_terms)
            .any(|iri| oxrdf::NamedNode::new(iri.clone()).is_err())
    {
        undetermined.push("receiver policy is incomplete or invalid".into());
    }
    let protected = protected(&before, &after, &changed_slots, policy);
    let mut dependents: BTreeSet<_> = changed_slots.iter().map(|s| s.0.clone()).collect();
    if before.len().saturating_add(after.len()) > MAX_TRIPLES {
        undetermined.push("dependency scan exceeds the triple bound".into());
    } else {
        let mut reverse = BTreeMap::<String, BTreeSet<String>>::new();
        for triple in before.iter().chain(&after) {
            reverse
                .entry(triple.object.to_string())
                .or_default()
                .insert(triple.subject.to_string());
        }
        let mut frontier = dependents.clone();
        while let Some(node) = frontier.pop_first() {
            for subject in reverse.get(&node).into_iter().flatten() {
                if dependents.insert(subject.clone()) {
                    frontier.insert(subject.clone());
                }
            }
            if dependents.len() > MAX_DEPENDENTS {
                undetermined.push("dependency closure exceeds its bound".into());
                break;
            }
        }
    }
    let mut risk = score(
        &[
            ("changed_slots", changed_slots.len()),
            ("dependents", dependents.len()),
            ("stored_queries", changed_queries.len()),
            ("newly_failing_instances", newly_failing_instances),
        ],
        policy.threshold.min(100),
        undetermined.is_empty(),
    );
    if !protected.is_empty() {
        risk.verdict = Verdict::Fail;
    }
    Ok(MergePlan {
        schema: "quipu-data-merge-plan/v1",
        store_id: store.store_id()?,
        root_tx: store.latest_tx_id()?,
        before_hash: digest(crate::merge_session::render(&before).as_bytes()),
        after_hash: digest(crate::merge_session::render(&after).as_bytes()),
        policy_hash: digest(
            &serde_json::to_vec(policy).map_err(|e| Error::Serialization(e.to_string()))?,
        ),
        shapes_hash: digest(store.get_combined_shapes()?.unwrap_or_default().as_bytes()),
        query_catalog_hash: catalog_hash(store)?,
        operation_hash: None,
        write_graphs: BTreeSet::from(["urn:quipu:graph:root".into()]),
        changed_slots,
        dependents,
        protected,
        changed_queries,
        preexisting_issues,
        newly_failing_issues,
        newly_failing_instances,
        named_graphs: graphs
            .iter()
            .filter(|g| {
                g.g != crate::schema::ROOT_GRAPH && g.lifecycle.as_deref() != Some("frozen")
            })
            .map(|g| g.iri.clone())
            .collect(),
        snapshots: graphs
            .iter()
            .filter(|g| g.lifecycle.as_deref() == Some("frozen"))
            .map(|g| g.iri.clone())
            .collect(),
        undetermined,
        risk,
    })
}

fn catalog_hash(store: &Store) -> Result<String> {
    let mut definitions = Vec::new();
    for query in store.query_list()? {
        let params: Vec<_> = query
            .params
            .iter()
            .map(|p| (&p.name, &p.kind, p.required, &p.default, &p.description))
            .collect();
        definitions.push(
            serde_json::to_string(&(&query.name, &query.template, &query.dataset, &params))
                .map_err(|e| Error::Serialization(e.to_string()))?,
        );
    }
    definitions.sort();
    Ok(digest(
        &serde_json::to_vec(&definitions).map_err(|e| Error::Serialization(e.to_string()))?,
    ))
}

/// Recompute evidence against the receiver without trusting reported counts.
/// The adapter must hold its writer reservation across this check and apply.
///
/// # Errors
/// Stale ROOT, altered evidence, or a failed/undetermined automatic verdict.
pub fn verify(
    store: &Store,
    before: &str,
    after: &str,
    policy: &Policy,
    evidence: &serde_json::Value,
) -> Result<()> {
    let actual = crate::share_merge::root_graph(store)?.0;
    if crate::merge_session::render(&actual)
        != crate::merge_session::render(&parse_graph(before, "merge proof ROOT")?)
    {
        return Err(Error::InvalidValue(
            "merge proof ROOT differs from receiver".into(),
        ));
    }
    let recomputed = plan(store, before, after, policy)?;
    let expected =
        serde_json::to_value(&recomputed).map_err(|e| Error::Serialization(e.to_string()))?;
    if &expected != evidence {
        return Err(Error::InvalidValue(
            "merge proof differs from receiver recomputation".into(),
        ));
    }
    if recomputed.risk.verdict != Verdict::Pass {
        return Err(Error::InvalidValue(format!(
            "automatic merge needs a decision: {:?}; {}",
            recomputed.risk.verdict,
            recomputed.undetermined.join("; ")
        )));
    }
    Ok(())
}

#[cfg(test)]
#[path = "merge_plan_tests.rs"]
mod tests;
