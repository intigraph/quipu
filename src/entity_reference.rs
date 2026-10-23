//! Stable registered minting and reference descriptors, without network fetches.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{Error, Result, Store, entity_namespaces};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    action: String,
    namespace: Option<String>,
    #[serde(rename = "type")]
    node_type: Option<String>,
    graph: Option<String>,
    label: Option<String>,
    iri: Option<String>,
    source: Option<String>,
    #[serde(rename = "asOf")]
    as_of: Option<String>,
}

fn required<'a>(value: &'a Option<String>, name: &str) -> Result<&'a str> {
    value
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::InvalidValue(format!("entity reference requires {name}")))
}

/// List effective namespaces. Read-only and never mints or applies config.
pub fn tool_namespaces(store: &Store, input: &Value) -> Result<Value> {
    if !input.is_object()
        || input
            .as_object()
            .is_none_or(|o| o.keys().any(|k| k != "action"))
        || input.get("action").is_some_and(|a| a != "list")
    {
        return Err(Error::InvalidValue(
            "namespaces supports only action=list".into(),
        ));
    }
    Ok(json!({"namespaces":entity_namespaces::list(store)?}))
}

/// Registered identity write / resolution. HTTP uses the existing authenticated
/// writer path; caller source hints never establish authority or fetch a URL.
pub fn tool_entity_ref(store: &mut Store, input: &Value) -> Result<Value> {
    let request: Request = serde_json::from_value(input.clone())
        .map_err(|e| Error::InvalidValue(format!("entity reference: {e}")))?;
    let namespaces = entity_namespaces::list(store)?;
    if request.action == "resolve" {
        let iri = required(&request.iri, "iri")?;
        oxrdf::NamedNode::new(iri).map_err(|e| Error::InvalidValue(e.to_string()))?;
        let entry = namespaces
            .iter()
            .find(|ns| iri.starts_with(&ns.base_iri))
            .ok_or_else(|| Error::InvalidValue("IRI has no registered namespace".into()))?;
        if let Some(time) = &request.as_of
            && crate::time::normalize_rfc3339_utc(time).is_none()
        {
            return Err(Error::InvalidValue("asOf must be RFC3339".into()));
        }
        let encoded: String = iri.bytes().map(|b| format!("%{b:02X}")).collect();
        let mut resolver = entry
            .resolver_url
            .as_ref()
            .map(|url| url.replace("{iri}", &encoded));
        if let (Some(url), Some(time)) = (&mut resolver, &request.as_of) {
            url.push(if url.contains('?') { '&' } else { '?' });
            url.push_str("asOf=");
            for byte in time.bytes() {
                url.push_str(&format!("%{byte:02X}"));
            }
        }
        return Ok(
            json!({"iri":iri,"source":request.source,"asOf":request.as_of,
            "namespace":entry.prefix,"audience":entry.audience,"resolver_url":resolver,
            "known":store.lookup(iri)?.is_some(),
            "historical_rendering":"use temporal query/history; /entity is current-state"}),
        );
    }
    if request.action != "mint" || request.iri.is_some() || request.as_of.is_some() {
        return Err(Error::InvalidValue(
            "use mint (without iri/asOf) or resolve".into(),
        ));
    }
    let prefix = required(&request.namespace, "namespace")?;
    let entry = namespaces
        .iter()
        .find(|ns| ns.prefix == prefix)
        .ok_or_else(|| Error::InvalidValue("unregistered namespace".into()))?;
    entity_namespaces::require_registered(store, entry)?;
    let node_type = required(&request.node_type, "type")?;
    oxrdf::NamedNode::new(node_type).map_err(|e| Error::InvalidValue(e.to_string()))?;
    if !entry.types.is_empty() && !entry.types.iter().any(|t| t == node_type) {
        return Err(Error::PolicyDenied(
            "namespace type selector refused".into(),
        ));
    }
    let graph_iri = request.graph.as_deref().unwrap_or("urn:quipu:graph:root");
    if !entry.graphs.is_empty() && !entry.graphs.iter().any(|g| g == graph_iri) {
        return Err(Error::PolicyDenied(
            "namespace graph selector refused".into(),
        ));
    }
    let graph = if request.graph.is_none() {
        0
    } else {
        let id = store
            .lookup(graph_iri)?
            .ok_or_else(|| Error::InvalidValue("unknown graph".into()))?;
        if store.graph_class(id)?.as_deref() != Some("committed") {
            return Err(Error::InvalidValue(
                "mint target must be a registered committed graph".into(),
            ));
        }
        id
    };
    let key = if entry.minting == "opaque" {
        opaque_key()?
    } else {
        let label = required(&request.label, "label for label minting")?;
        if !label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        {
            return Err(Error::InvalidValue(
                "label key must contain only letters, digits, - or _".into(),
            ));
        }
        label.to_string()
    };
    let iri = format!("{}{key}", entry.base_iri);
    // The term dictionary outlives retracted facts: IDs are never recycled.
    if store.lookup(&iri)?.is_some() {
        return Err(Error::InvalidValue(
            "entity key already allocated; never reuse".into(),
        ));
    }
    let mut text = format!("<{iri}> <{}> <{node_type}> .\n", crate::namespace::RDF_TYPE);
    if let Some(label) = &request.label {
        text.push_str(&format!(
            "<{iri}> <{}> {} .\n",
            crate::namespace::RDFS_LABEL,
            oxrdf::Literal::new_simple_literal(label)
        ));
    }
    let (tx, count) = crate::rdf::ingest_rdf_to_graph(
        store,
        text.as_bytes(),
        oxrdfio::RdfFormat::NTriples,
        None,
        &crate::time::now_iso(),
        None,
        request.source.as_deref(),
        graph,
    )?;
    Ok(json!({"iri":iri,"tx_id":tx,"count":count,"namespace":prefix}))
}

fn opaque_key() -> Result<String> {
    let mut bytes = [0u8; 16];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes)
        .map_err(|_| Error::CannotVerify("OS entropy unavailable".into()))?;
    let mut out = String::new();
    let alphabet = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut bits = 0u32;
    let mut available = 0;
    for byte in bytes {
        bits = (bits << 8) | u32::from(byte);
        available += 8;
        while available >= 5 {
            available -= 5;
            out.push(alphabet[((bits >> available) & 31) as usize] as char);
        }
    }
    if available > 0 {
        out.push(alphabet[((bits << (5 - available)) & 31) as usize] as char);
    }
    Ok(out)
}

/// Shared REST/MCP schemas. Namespace listing is read-only; entity minting is
/// always authenticated, including the resolve action on the same route.
pub fn definitions() -> Vec<Value> {
    vec![
        json!({
            "name":"quipu_namespaces",
            "description":"List the effective owner namespace registry; no writes or allocation.",
            "inputSchema":{"type":"object","additionalProperties":false,
                "properties":{"action":{"type":"string","enum":["list"]}}}
        }),
        json!({
            "name":"quipu_entity_ref",
            "description":"Mint a typed stable entity in a registered namespace, or describe an exact registered reference without fetching. Uncertain mint responses must be reconciled, never blindly retried.",
            "inputSchema":{"type":"object","additionalProperties":false,
                "required":["action"],"properties":{
                    "action":{"type":"string","enum":["mint","resolve"]},
                    "namespace":{"type":"string"},"type":{"type":"string"},
                    "graph":{"type":"string"},"label":{"type":"string"},
                    "iri":{"type":"string"},"source":{"type":"string"},
                    "asOf":{"type":"string"}}}
        }),
    ]
}
