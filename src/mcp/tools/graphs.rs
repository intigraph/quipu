//! MCP tool: `quipu_graph_list` — the graph registry, listed.
//!
//! The read half of the graph-kinds surface (the writes are
//! `quipu_graph_create` / `quipu_graph_label` in `mcp::governance`). Also the
//! consumer capability probe: a store serving this tool has the kind axis; a
//! 404/unknown-tool answer means the store predates it, which a consumer must
//! treat as "cannot tell", never as "no graphs".

use serde_json::Value as JsonValue;

use crate::error::{Error, Result};
use crate::store::Store;

/// MCP tool: `quipu_graph_create` -- register a committed-class named graph.
///
/// Input: `{ "graph": "<iri>", "actor"?: "<declared creator>", "timestamp"?: "<RFC3339>" }`.
/// Output includes `graph`, `g`, `created` and retained creation provenance.
///
/// This exists because `Store::graph_create` had no caller outside its own
/// tests (camayoc-s0h): the registration `set_graph_label` presumes was
/// written and unreachable, so a consumer could write into a named graph via
/// `/episode` — which interns the IRI without registering it — and then find
/// the graph could not be labelled. Routing without labelling is worse than
/// neither, because it produces separate graphs every query still reads at
/// equal trust.
///
/// Idempotent. Re-creating an existing committed graph is a no-op, and
/// `created` reports which happened rather than leaving the caller to guess
/// from a bare success.
pub fn tool_graph_create(store: &Store, input: &JsonValue) -> Result<JsonValue> {
    let graph = input
        .get("graph")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| Error::InvalidValue("missing 'graph' parameter".into()))?;
    let now = crate::time::now_iso();
    let timestamp =
        match input.get("timestamp") {
            None => now,
            Some(value) => crate::time::normalize_rfc3339_utc(value.as_str().ok_or_else(|| {
                Error::InvalidValue("'timestamp' must be an RFC 3339 string".into())
            })?)
            .ok_or_else(|| {
                Error::InvalidValue("'timestamp' must be an RFC 3339 date-time".into())
            })?,
        };
    let actor = match input.get("actor") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| Error::InvalidValue("'actor' must be a non-empty string".into()))?,
        ),
    };
    store.graph_create_recorded(graph, &timestamp, actor)
}

/// List registered graphs, optionally filtered.
///
/// Input: `{ "kind": "<token>", "lifecycle": "frozen" }` — both optional.
/// Output: `{ "graphs": [{iri, g, class, source, lifecycle, labels: {…}}, …],
/// "count": N }`.
pub fn tool_graph_list(store: &Store, input: &JsonValue) -> Result<JsonValue> {
    if let Some(graph) = input.get("graph") {
        if input.get("kind").is_some() || input.get("lifecycle").is_some() {
            return Err(Error::InvalidValue(
                "graph cannot be combined with kind or lifecycle".into(),
            ));
        }
        let graph = graph
            .as_str()
            .ok_or_else(|| Error::InvalidValue("graph must be an absolute IRI string".into()))?;
        let rows: Vec<JsonValue> = store.graph_metadata(graph)?.into_iter().collect();
        return Ok(serde_json::json!({"count": rows.len(), "graphs": rows}));
    }
    let kind = match input.get("kind").and_then(JsonValue::as_str) {
        // Strict parse, matching quipu_graph_label: an unrecognised shape is
        // an error, never a filter that silently matches nothing.
        Some(k) => Some(crate::lattice_kind::DataKind::parse(k)?),
        None => None,
    };
    let lifecycle = input.get("lifecycle").and_then(JsonValue::as_str);
    if let Some(lc) = lifecycle
        && lc != "frozen"
    {
        return Err(Error::InvalidValue(format!(
            "unknown lifecycle filter '{lc}'; the only lifecycle state is 'frozen'"
        )));
    }

    let graphs = store.list_graphs(
        kind.as_ref().map(crate::lattice_kind::DataKind::as_str),
        lifecycle,
    )?;
    let mut rows: Vec<JsonValue> = Vec::with_capacity(graphs.len());
    for gi in &graphs {
        let mut row = serde_json::json!({
            "iri": gi.iri,
            "g": gi.g,
            "class": gi.class,
            "source": gi.source,
            "lifecycle": gi.lifecycle,
            "creation": store.graph_creation_provenance(gi.g)?,
            "labels": {
                "freshness": gi.freshness,
                "durability": gi.durability,
                "trust_rank": gi.trust_rank,
                "trust_chain": gi.trust_chain,
                "policy": gi.policy,
                "kind": gi.kind,
            },
        });
        // A mapped graph serves its materialization stamp (quipu-212): the
        // mapping, the source subject, and the verified source hash of the
        // last RML materialization — the comparands a freshness verdict
        // needs. Omitted rather than faked when the graph has none.
        if let Some(m) = store.mapped_provenance(gi.g)? {
            row["materialization"] = m.to_json();
        }
        rows.push(row);
    }
    Ok(serde_json::json!({ "count": rows.len(), "graphs": rows }))
}

/// MCP tool: `quipu_graph_freeze` — relocate a graph's full history into a
/// read-only archive pack (deep freeze).
///
/// Input: `{ "graph": "<iri>", "out_dir": "<dir>", "timestamp": "...",
/// "actor": "..." }` (`out_dir` optional — defaults beside the store file).
pub fn tool_graph_freeze(store: &mut Store, input: &JsonValue) -> Result<JsonValue> {
    let graph = input
        .get("graph")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| Error::InvalidValue("missing 'graph' parameter".into()))?;
    let timestamp = input
        .get("timestamp")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| Error::InvalidValue("missing 'timestamp' parameter".into()))?;
    let out_dir = match input.get("out_dir").and_then(JsonValue::as_str) {
        Some(d) => d.to_string(),
        None => store.db_parent_dir().ok_or_else(|| {
            Error::InvalidValue(
                "this store has no file path (in-memory?); pass 'out_dir' explicitly".into(),
            )
        })?,
    };
    let actor = input.get("actor").and_then(JsonValue::as_str);
    let r = store.freeze_graph(graph, &out_dir, timestamp, actor)?;
    Ok(serde_json::json!({
        "graph": r.graph_iri,
        "pack": r.path,
        "alias": r.alias,
        "content_hash": r.content_hash,
        "facts": r.facts,
        "transactions": r.transactions,
        "vectors": r.vectors,
        "vectors_omitted": r.vectors_omitted,
    }))
}

/// MCP tool: `quipu_graph_thaw` — restore a frozen graph's history from its
/// archive pack and reopen it for writes.
pub fn tool_graph_thaw(store: &mut Store, input: &JsonValue) -> Result<JsonValue> {
    let graph = input
        .get("graph")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| Error::InvalidValue("missing 'graph' parameter".into()))?;
    let timestamp = input
        .get("timestamp")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| Error::InvalidValue("missing 'timestamp' parameter".into()))?;
    let actor = input.get("actor").and_then(JsonValue::as_str);
    let (facts, vectors) = store.thaw_graph(graph, timestamp, actor)?;
    Ok(serde_json::json!({
        "graph": graph,
        "facts_restored": facts,
        "vectors_restored": vectors,
    }))
}
