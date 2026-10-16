//! Stable community descriptions; persistence is opt-in and graph-scoped.
use super::community_identity::{digest, match_groups};
use super::{ProjectedGraph, louvain};
use crate::types::Value;
use crate::{Error, Result, Store, namespace};
use serde::Serialize;
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize)]
pub(super) struct NamedCommunity {
    pub community: String,
    pub entities: Vec<String>,
    pub size: usize,
    pub label: String,
    pub label_basis: Json,
    pub derived_from: Vec<String>,
}

pub(super) fn previous(store: &Store, graph: &str) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let Some(g) = store.registered_graph_id(graph)? else {
        return Ok(BTreeMap::new());
    };
    let Some(pred) = store.lookup(&format!("{}memberOfCommunity", namespace::QUIPU))? else {
        return Ok(BTreeMap::new());
    };
    let mut groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for fact in store.current_facts_for_attributes_in_graph(&[pred], g)? {
        if let Value::Ref(group) = fact.value {
            groups
                .entry(store.resolve(group)?)
                .or_default()
                .insert(store.resolve(fact.entity)?);
        }
    }
    Ok(groups)
}

pub(super) fn scope(input: &Json) -> String {
    digest(&[
        "louvain-community/v1".into(),
        json!(input.get("graph").and_then(Json::as_str)).to_string(),
        json!(input.get("type").and_then(Json::as_str)).to_string(),
        json!(input.get("predicate").and_then(Json::as_str)).to_string(),
    ])
}

pub(super) fn describe(
    store: &Store,
    pg: &ProjectedGraph,
    groups: &[Vec<i64>],
    input: &Json,
) -> Result<(String, Vec<NamedCommunity>)> {
    let threshold = match input.get("match_threshold") {
        Some(value) => value
            .as_f64()
            .filter(|v| v.is_finite() && *v > 0.0 && *v <= 1.0)
            .ok_or_else(|| Error::InvalidValue("match_threshold must be in (0, 1]".into()))?,
        None => 0.5,
    };
    let scope_id = scope(input);
    let derived = format!("urn:quipu:communities:{scope_id}");
    let old = previous(store, &derived)?;
    let mut member_iris = Vec::new();
    let mut seen = BTreeSet::new();
    for group in groups {
        if group.is_empty() {
            return Err(Error::InvalidValue("empty community".into()));
        }
        let mut iris = Vec::new();
        for &id in group {
            if !seen.insert(id) {
                return Err(Error::InvalidValue(
                    "community partition repeats a member".into(),
                ));
            }
            iris.push(store.resolve(id)?);
        }
        iris.sort();
        member_iris.push(iris);
    }
    let identities = match_groups(&scope_id, &member_iris, &old, threshold);
    let source_graph = super::resolve_graph(store, input.get("graph").and_then(Json::as_str))?;
    let labels = super::community_labels::labels(store, pg, groups, source_graph)?;
    let descriptions = member_iris
        .into_iter()
        .zip(identities)
        .zip(labels)
        .map(|((entities, id), (label, mut basis))| {
            basis["source_graph"] = input.get("graph").cloned().unwrap_or(Json::Null);
            basis["type_filter"] = input.get("type").cloned().unwrap_or(Json::Null);
            basis["predicate_filter"] = input.get("predicate").cloned().unwrap_or(Json::Null);
            NamedCommunity {
                community: id.iri,
                size: entities.len(),
                entities,
                label,
                label_basis: basis,
                derived_from: id.derived_from,
            }
        })
        .collect();
    Ok((derived, descriptions))
}

pub(super) fn run(store: &mut Store, pg: &ProjectedGraph, input: &Json) -> Result<Json> {
    let partition = louvain(pg);
    let (derived_graph, descriptions) = describe(store, pg, &partition.groups, input)?;
    let persisted = if input
        .get("persist")
        .and_then(Json::as_bool)
        .unwrap_or(false)
    {
        let now = crate::time::now_iso();
        let timestamp = input
            .get("timestamp")
            .and_then(Json::as_str)
            .unwrap_or(&now);
        super::community_persist::persist(
            store,
            &derived_graph,
            &descriptions,
            input,
            partition.modularity,
            timestamp,
        )?;
        Some(descriptions.iter().map(|d| d.size).sum::<usize>())
    } else {
        None
    };
    Ok(
        json!({"algorithm":"louvain", "communities":descriptions, "count":partition.groups.len(),
        "modularity":partition.modularity, "persisted":persisted, "derived_graph":derived_graph}),
    )
}

/// Retained public entry point: default-scope partition, now in a derived graph.
pub fn persist_communities(
    store: &mut Store,
    groups: &[Vec<i64>],
    timestamp: &str,
) -> Result<usize> {
    let input = json!({});
    let pg = super::project(store, None, None)?;
    let (derived, descriptions) = describe(store, &pg, groups, &input)?;
    super::community_persist::persist(store, &derived, &descriptions, &input, 0.0, timestamp)?;
    Ok(descriptions.iter().map(|d| d.size).sum())
}
