//! Labels use only facts in the selected source graph, with lexical tie breaks.
use super::{PageRankConfig, ProjectedGraph, page_rank};
use crate::types::Value;
use crate::{Result, Store, namespace};
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

fn text(value: &Value) -> Option<&str> {
    match value {
        Value::Str(s) => Some(s),
        Value::Lang { lexical, .. } => Some(lexical),
        _ => None,
    }
}
fn short(value: &str, limit: usize) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(limit)
        .collect()
}
fn local(iri: &str) -> String {
    short(
        iri.rsplit(['/', '#', ':'])
            .find(|s| !s.is_empty())
            .unwrap_or(iri),
        60,
    )
}
fn label(store: &Store, entity: i64, graph: i64) -> Result<String> {
    let mut labels = Vec::new();
    for fact in store.entity_facts_in_graph(entity, graph)? {
        let pred = store.resolve(fact.attribute)?;
        if (pred == format!("{}label", namespace::RDFS)
            || pred == "http://www.w3.org/2004/02/skos/core#prefLabel")
            && let Some(s) = text(&fact.value)
        {
            let s = short(s, 60);
            if !s.is_empty() {
                labels.push(s);
            }
        }
    }
    labels.sort();
    Ok(labels
        .into_iter()
        .next()
        .unwrap_or(local(&store.resolve(entity)?)))
}

pub(super) fn labels(
    store: &Store,
    pg: &ProjectedGraph,
    groups: &[Vec<i64>],
    graph: i64,
) -> Result<Vec<(String, Json)>> {
    let ranks: HashMap<i64, f32> = page_rank(pg, &PageRankConfig::default())?
        .into_iter()
        .collect();
    groups.iter().map(|members| {
        let mut top = Vec::new();
        let mut types: BTreeMap<String, usize> = BTreeMap::new();
        let mut pairs: BTreeMap<(String, String), usize> = BTreeMap::new();
        for &member in members {
            let iri = store.resolve(member)?;
            top.push((ranks.get(&member).copied().unwrap_or(0.0), iri, label(store, member, graph)?));
            let mut seen_types = BTreeSet::new();
            let mut seen_pairs = BTreeSet::new();
            for fact in store.entity_facts_in_graph(member, graph)? {
                let pred = store.resolve(fact.attribute)?;
                if pred == namespace::RDF_TYPE {
                    if let Value::Ref(t) = fact.value { seen_types.insert(store.resolve(t)?); }
                    continue;
                }
                if pred.starts_with("http://www.w3.org/ns/prov#") || pred.starts_with(namespace::QUIPU)
                    || pred == format!("{}label", namespace::RDFS)
                    || pred == "http://www.w3.org/2004/02/skos/core#prefLabel"
                    || pred == "https://schema.org/mentions" { continue; }
                let object = match &fact.value {
                    Value::Ref(id) => store.resolve(*id)?,
                    value => match text(value) { Some(s) if s.chars().count() <= 256 => s.to_string(), _ => continue },
                };
                seen_pairs.insert((pred, object));
            }
            for t in seen_types { *types.entry(t).or_default() += 1; }
            for pair in seen_pairs { *pairs.entry(pair).or_default() += 1; }
        }
        top.sort_by(|a,b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        let dominant = types.into_iter().filter(|(_, n)| *n * 5 >= members.len() * 2)
            .min_by(|a,b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let shared = pairs.into_iter().filter(|(_, n)| *n >= 2)
            .min_by(|a,b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let mut segments = Vec::new();
        if let Some((t,_)) = &dominant { segments.push(local(t)); }
        if let Some(((_, object),_)) = &shared { segments.push(local(object)); }
        if let Some((_,_,name)) = top.first() { segments.push(name.clone()); }
        segments.dedup();
        let name = short(&segments.join(" · "), 60);
        Ok((name, json!({
            "algorithm":"louvain-label/v1",
            "top_members":top.iter().take(2).map(|(score,iri,label)|json!({"entity":iri,"page_rank":score,"label":label})).collect::<Vec<_>>(),
            "dominant_type":dominant.map(|(iri,count)|json!({"entity":iri,"member_count":count})),
            "shared_fact":shared.map(|((predicate,object),count)|json!({"predicate":predicate,"object":object,"member_count":count})),
        })))
    }).collect()
}
