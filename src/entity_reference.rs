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
    /// Key-field values for `minting = "key"` namespaces, by predicate IRI.
    keys: Option<serde_json::Map<String, Value>>,
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
    if request.action == "verify" {
        if request.keys.is_some()
            || request.as_of.is_some()
            || request.namespace.is_some()
            || request.node_type.is_some()
            || request.label.is_some()
            || request.source.is_some()
        {
            return Err(Error::InvalidValue(
                "verify accepts only iri and optional graph".into(),
            ));
        }
        let iri = required(&request.iri, "iri")?;
        oxrdf::NamedNode::new(iri).map_err(|e| Error::InvalidValue(e.to_string()))?;
        let graph = match request.graph.as_deref() {
            None => 0,
            Some(iri) => store
                .lookup(iri)?
                .ok_or_else(|| Error::InvalidValue("unknown graph".into()))?,
        };
        return crate::entity_key_guard::verify(store, iri, graph);
    }
    if request.action == "resolve" {
        if request.keys.is_some() {
            return Err(Error::InvalidValue(
                "resolve takes an exact iri, not keys".into(),
            ));
        }
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
            "use mint (without iri/asOf), resolve or verify".into(),
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
    if request.keys.is_some() != (entry.minting == "key") {
        return Err(Error::InvalidValue(
            "keys are required for, and only accepted by, key-minting namespaces".into(),
        ));
    }
    let mut key_triples = Vec::new();
    let key = match entry.minting.as_str() {
        "opaque" => opaque_key()?,
        "key" => {
            let shapes = store.get_combined_shapes()?.ok_or_else(|| {
                Error::InvalidValue("key minting requires a loaded keyed shape".into())
            })?;
            let spec = crate::entity_key::key_spec(&shapes, node_type)?;
            if spec.strategy == crate::entity_key::Strategy::Lexical && entry.types.len() != 1 {
                return Err(Error::InvalidValue(
                    "lexical keys omit the type, so the namespace must select exactly one type"
                        .into(),
                ));
            }
            let keys = request.keys.as_ref().expect("checked above");
            key_triples = crate::entity_key::key_terms(&spec, keys)?;
            crate::entity_key::derive_key(&spec, node_type, &key_triples)
        }
        _ => {
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
        }
    };
    let iri = format!("{}{key}", entry.base_iri);
    // The term dictionary outlives retracted facts: IDs are never recycled.
    if let Some(id) = store.lookup(&iri)? {
        if entry.minting == "key" {
            return converge(store, id, graph, &iri, node_type, &key_triples, prefix);
        }
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
    for (field, term) in &key_triples {
        text.push_str(&format!("<{iri}> <{field}> {term} .\n"));
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
    let mut out = json!({"iri":iri,"tx_id":tx,"count":count,"namespace":prefix});
    if entry.minting == "key" {
        out["converged"] = json!(false);
    }
    Ok(out)
}

/// A key IRI that already exists is the same entity, so a second writer gets
/// the existing IRI and writes nothing. It converges only onto a live entity
/// of the same type carrying exactly these key values in the target graph;
/// anything else (retracted, other type, other graph, mutated key, a hash
/// collision) refuses rather than reviving or overwriting identity.
fn converge(
    store: &Store,
    id: i64,
    graph: i64,
    iri: &str,
    node_type: &str,
    key_triples: &[(String, oxrdf::Term)],
    prefix: &str,
) -> Result<Value> {
    let mut current: std::collections::BTreeMap<String, Vec<oxrdf::Term>> = Default::default();
    for fact in store.entity_facts_in_graph(id, graph)? {
        current
            .entry(store.resolve(fact.attribute)?)
            .or_default()
            .push(crate::rdf::value_to_term(store, &fact.value)?);
    }
    let typed = current.get(crate::namespace::RDF_TYPE).is_some_and(|ts| {
        ts.iter()
            .any(|t| matches!(t, oxrdf::Term::NamedNode(n) if n.as_str() == node_type))
    });
    let same_keys = key_triples.iter().all(|(field, term)| {
        current
            .get(field)
            .is_some_and(|vs| vs.as_slice() == [term.clone()])
    });
    if !(typed && same_keys) {
        return Err(Error::InvalidValue(
            "key IRI exists without a live entity of this type and key in this graph; \
             restore or supersede it explicitly, never re-mint"
                .into(),
        ));
    }
    Ok(json!({"iri":iri,"tx_id":null,"count":0,"namespace":prefix,"converged":true}))
}

fn opaque_key() -> Result<String> {
    let mut bytes = [0u8; 16];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes)
        .map_err(|_| Error::CannotVerify("OS entropy unavailable".into()))?;
    Ok(crate::entity_key::base32_lower(&bytes))
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
            "description":"Mint a typed stable entity, resolve a registered reference or verify a local key IRI against its key triples without fetching. Uncertain mint responses must be reconciled, never blindly retried.",
            "inputSchema":{"type":"object","additionalProperties":false,
                "required":["action"],"properties":{
                    "action":{"type":"string","enum":["mint","resolve","verify"]},
                    "namespace":{"type":"string"},"type":{"type":"string"},
                    "graph":{"type":"string"},"label":{"type":"string"},
                    "iri":{"type":"string"},"source":{"type":"string"},
                    "asOf":{"type":"string"},
                    "keys":{"type":"object","description":"Key-field values by predicate IRI, for minting=key namespaces. Same type + keys = same IRI; a repeat converges without writing."}}}
        }),
    ]
}
