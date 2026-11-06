//! Bounded, explicitly lexical answers for stores without a complete vector index.
//! Lexical evidence has its own contract; it never manufactures cosine similarity.
use crate::{Error, Result, Store};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};

pub(crate) fn read_lexical(store: &Store, read: impl FnOnce() -> Result<Value>) -> Result<Value> {
    let ms = crate::time::request_deadline().map_or(1000, |d| d.remaining_millis().min(1000));
    let _budget = crate::time::set_request_deadline(Some(crate::time::Deadline::after_millis(ms)));
    let mut response = read()?;
    response["retrieval"] = json!({"mode":"lexical","status":"degraded",
        "reason":"literal_matching_not_semantic","embeddings":store.vector_store().vector_count()?,
        "provider_configured":store.embedding_provider().is_some(),"budget_ms":ms});
    Ok(response)
}

pub(crate) fn run(
    store: &Store,
    input: &Value,
    semantic: impl FnOnce() -> Result<Value>,
) -> Result<Value> {
    super::search_confidence::Policy::parse(input)?;
    // Preserve explicit vector, structured and anchored contracts and their gates.
    if input.get("query").and_then(Value::as_str).is_none()
        || ["embedding", "anchor", "structured_query", "filters", "mode"]
            .iter()
            .any(|key| input.get(*key).is_some())
    {
        return semantic();
    }
    let vectors = store.vector_store().vector_count()?;
    if store.embedding_provider().is_none() || vectors == 0 {
        return lexical(store, input, vectors);
    }
    let mut response = semantic()?;
    if response.get("context_results").is_none() && response["results"].is_array() {
        super::search_confidence::Policy::parse(input)?.annotate(input, &mut response)?;
    }
    // Supplement candidates whose current vector cannot be established. Existing vector
    // hits still pass the semantic context admission gate before reaching MCP.
    match lexical(store, input, vectors) {
        Ok(mut fallback) => {
            let mut unindexed = Vec::new();
            for row in fallback["results"].as_array().into_iter().flatten() {
                let iri = row["entity"].as_str().unwrap_or("");
                if let Some(id) = store.lookup(iri)?
                    && store.vector_store().current_embedding_text(id)?.is_none()
                {
                    unindexed.push(row.clone());
                }
            }
            if !unindexed.is_empty() {
                let mut rows = response["context_results"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let has_semantic_rows = !rows.is_empty();
                let prefixes = crate::compact::PrefixMap::from_store(store)?;
                let seen: HashSet<_> = rows
                    .iter()
                    .filter_map(|r| r["entity"].as_str().map(|iri| prefixes.expand(iri)))
                    .collect();
                rows.extend(
                    unindexed
                        .into_iter()
                        .filter(|r| !seen.contains(r["entity"].as_str().unwrap_or(""))),
                );
                rows.truncate(
                    store
                        .search_config()
                        .clamp_limit(input.get("limit").and_then(Value::as_u64)),
                );
                fallback["results"] = json!(rows);
                fallback["count"] = json!(rows.len());
                fallback["retrieval"]["mode"] = json!(if has_semantic_rows {
                    "mixed"
                } else {
                    "lexical"
                });
                fallback["retrieval"]["semantic_admission"] = json!(if has_semantic_rows {
                    "per_row"
                } else {
                    "unavailable"
                });
                fallback["semantic_context"] = response["context"].clone();
                return Ok(fallback);
            }
            response["retrieval"] = json!({"mode":"semantic","status":"ok","embeddings":vectors});
        }
        Err(error) => {
            response["retrieval"] = json!({"mode":"semantic","status":"degraded",
                "embeddings":vectors,"lexical_status":"unknown","reason":error.to_string()});
        }
    }
    Ok(response)
}

fn lexical(store: &Store, input: &Value, vectors: usize) -> Result<Value> {
    let query = input["query"]
        .as_str()
        .filter(|q| !q.trim().is_empty() && q.len() <= 4096)
        .ok_or_else(|| {
            Error::InvalidValue("lexical fallback requires 1..4096 bytes of query text".into())
        })?;
    let budget_ms =
        crate::time::request_deadline().map_or(1000, |d| d.remaining_millis().min(1000));
    let _budget =
        crate::time::set_request_deadline(Some(crate::time::Deadline::after_millis(budget_ms)));
    let scope = crate::search_graph_scope::GraphScope::parse_keyword(store, input)?;
    let groups: Option<Vec<&str>> = input
        .get("group_ids")
        .and_then(Value::as_array)
        .map(|g| g.iter().filter_map(Value::as_str).collect());
    let valid_at = input.get("valid_at").and_then(Value::as_str);
    let allowed = super::search::scoped_entity_iris_at(
        store,
        input.get("entity_type").and_then(Value::as_str),
        groups.as_deref(),
        valid_at,
        true,
        Some(&scope),
    )?;
    let sparql_scope = input
        .get("sparql")
        .and_then(Value::as_str)
        .map(|q| super::search_scoped::candidates(store, q, valid_at))
        .transpose()?
        .map(|rows| rows.into_iter().collect::<HashSet<_>>());
    let literal = serde_json::to_string(&query.to_lowercase())
        .map_err(|e| Error::Serialization(e.to_string()))?;
    let body = format!(
        "?s ?p ?text . FILTER(isLiteral(?text)) FILTER(CONTAINS(LCASE(STR(?text)), {literal}) || CONTAINS(LCASE(STR(?s)), {literal}))"
    );
    let pattern = scope.patterns(store, &body)?;
    // Match before LIMIT. The legacy node/fact oversample can miss a late label.
    let query = format!("SELECT DISTINCT ?s ?text WHERE {{ {pattern} }} LIMIT 1001");
    let context = crate::sparql::TemporalContext {
        valid_at: valid_at.map(str::to_owned),
        row_cap: Some(1001),
        result_limit: Some(1001),
        ..Default::default()
    };
    let found = crate::sparql::query_temporal(store, &query, &context)?;
    let truncated = found.rows().len() == 1001;
    let mut entities = BTreeMap::new();
    for row in found.rows() {
        let Some(crate::types::Value::Ref(id)) = row.get("s") else {
            continue;
        };
        let iri = store.resolve(*id)?;
        if allowed.as_ref().is_some_and(|a| !a.contains(&iri))
            || sparql_scope.as_ref().is_some_and(|a| !a.contains(&iri))
        {
            continue;
        }
        if let Some(
            crate::types::Value::Str(text)
            | crate::types::Value::Lang { lexical: text, .. }
            | crate::types::Value::Typed { lexical: text, .. },
        ) = row.get("text")
        {
            entities
                .entry(iri)
                .or_insert_with(|| text.chars().take(4096).collect::<String>());
        }
    }
    let limit = store
        .search_config()
        .clamp_limit(input.get("limit").and_then(Value::as_u64));
    let matched_entities = entities.len();
    let rows: Vec<_> = entities
        .into_iter()
        .take(limit)
        .map(|(iri, text)| {
            json!({
        "entity":iri,"text":text,"source":"knowledge","ranking_reason":"literal_substring",
        "evidence_mode":"lexical"})
        })
        .collect();
    Ok(
        json!({"results":rows,"count":rows.len(),"ranking":"lexical","scoped":scope.explicit || allowed.is_some() || sparql_scope.is_some(),
        "search_scope":scope.coverage(store,input),
        "retrieval":{"contract":"quipu-lexical-results/v1","mode":"lexical","status":"degraded",
            "reason":if vectors == 0 { "empty_embedding_index" } else if store.embedding_provider().is_none() { "no_embedding_provider" } else { "embedding_coverage_unverified" },
            "embeddings":vectors,"matched_lexical_entities":matched_entities,
            "coverage_scope":"whole-store vector count; bounded matched lexical candidates","provider_configured":store.embedding_provider().is_some(),
            "candidate_limit":1001,"truncated":truncated,"budget_ms":budget_ms,"semantic_admission":"unavailable"}}),
    )
}

#[cfg(test)]
#[path = "fresh_search_tests.rs"]
mod tests;
