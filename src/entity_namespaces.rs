//! Registered stable entity namespaces. Configuration is owner policy, not an
//! assertion imported from an untrusted pendant.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Error, Result, Store};

/// Reserved owner registry graph; declarations in other graphs have no authority.
pub const REGISTRY_GRAPH: &str = "urn:quipu:graph:namespaces";
const RECORD: &str = "https://w3id.org/intigraph/vocab/namespaceDeclaration";

/// A namespace's immutable identity and publication policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Namespace {
    pub prefix: String,
    pub base_iri: String,
    pub visibility: String,
    pub audience: String,
    pub resolver_url: Option<String>,
    pub minting: String,
    pub graphs: Vec<String>,
    pub types: Vec<String>,
}

impl Default for Namespace {
    fn default() -> Self {
        Self {
            prefix: String::new(),
            base_iri: "https://w3id.org/intigraph/".into(),
            visibility: "public".into(),
            audience: "public".into(),
            resolver_url: None,
            minting: "opaque".into(),
            graphs: Vec::new(),
            types: Vec::new(),
        }
    }
}

/// Protected operator binding, never learned from a request or imported RDF.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DestinationBinding {
    pub store_id: String,
    pub audience: String,
}

/// Runtime configuration supplied through the normal owner-config open path.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    pub namespaces: Vec<Namespace>,
    pub destinations: Vec<DestinationBinding>,
}

fn invalid(message: &str) -> Error {
    Error::InvalidValue(format!("namespace registry: {message}"))
}

fn audience_valid(value: &str) -> bool {
    value == "public"
        || ["org:", "personal:"].iter().any(|prefix| {
            value.strip_prefix(prefix).is_some_and(|id| {
                !id.is_empty()
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            })
        })
}

impl Namespace {
    fn validate(&self) -> Result<()> {
        let base = &self.base_iri;
        let public = self.visibility == "public";
        if self.prefix.is_empty()
            || !self
                .prefix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || !self.prefix.as_bytes()[0].is_ascii_alphabetic()
            || !["public", "private"].contains(&self.visibility.as_str())
            || !audience_valid(&self.audience)
            || public != (self.audience == "public")
            || !["opaque", "label"].contains(&self.minting.as_str())
        {
            return Err(invalid(
                "invalid prefix, visibility, audience or minting policy",
            ));
        }
        let authority = base
            .split_once("://")
            .map(|(_, rest)| rest.split('/').next().unwrap_or(""));
        if !(base.starts_with("https://") || (!public && base.starts_with("http://")))
            || authority.is_none_or(|host| host.is_empty() || host.contains('@'))
            || base.contains('?')
            || !base.ends_with(['/', '#'])
            || base.matches('#').count() > usize::from(base.ends_with('#'))
        {
            return Err(invalid(
                "base must be an absolute HTTP(S) namespace ending / or #",
            ));
        }
        oxrdf::NamedNode::new(base.clone()).map_err(|_| invalid("invalid base IRI"))?;
        for iri in self.graphs.iter().chain(&self.types) {
            oxrdf::NamedNode::new(iri.clone()).map_err(|_| invalid("invalid selector IRI"))?;
        }
        if let Some(template) = &self.resolver_url {
            if template.matches("{iri}").count() != 1 {
                return Err(invalid("resolver URL needs exactly one {iri} placeholder"));
            }
            let resolver = template.replace("{iri}", "entity");
            oxrdf::NamedNode::new(resolver.clone()).map_err(|_| invalid("invalid resolver URL"))?;
            if template.strip_prefix("https://").is_none_or(|rest| {
                rest.split(['/', '?'])
                    .next()
                    .is_none_or(|authority| authority.is_empty() || authority.contains('{'))
            }) || !resolver.starts_with("https://")
                || resolver.contains('@')
                || resolver.contains('#')
            {
                return Err(invalid(
                    "resolver must use HTTPS without credentials or fragment",
                ));
            }
        }
        Ok(())
    }
}

/// Install config in memory. Does not write registry facts or change identity.
pub fn configure(store: &mut Store, config: &crate::QuipuConfig) -> Result<()> {
    let settings = Settings {
        namespaces: config.namespaces.clone(),
        destinations: config.namespace_destinations.clone(),
    };
    for entry in &settings.namespaces {
        entry.validate()?;
    }
    for binding in &settings.destinations {
        if binding.store_id.is_empty() || !audience_valid(&binding.audience) {
            return Err(invalid("invalid trusted destination binding"));
        }
    }
    store.namespace_settings = settings;
    list(store)?;
    Ok(())
}

/// Effective config + retained owner policy. Never consult imported registry graphs.
fn persisted(store: &Store) -> Result<BTreeMap<String, Namespace>> {
    let mut entries = BTreeMap::<String, Namespace>::new();
    if let Some(graph) = store.lookup(REGISTRY_GRAPH)?
        && let Some(predicate) = store.lookup(RECORD)?
    {
        for fact in store.current_facts_in_graph(graph)? {
            if fact.attribute != predicate {
                continue;
            }
            let crate::types::Value::Str(text) = fact.value else {
                return Err(invalid("non-string persisted declaration"));
            };
            let entry: Namespace = serde_json::from_str(&text)
                .map_err(|_| invalid("malformed persisted declaration"))?;
            insert(&mut entries, entry)?;
        }
    }
    Ok(entries)
}

/// Effective declared and persisted owner namespace policy.
pub fn list(store: &Store) -> Result<Vec<Namespace>> {
    let mut entries = persisted(store)?;
    for entry in &store.namespace_settings.namespaces {
        insert(&mut entries, entry.clone())?;
    }
    Ok(entries.into_values().collect())
}

/// Minting requires durable policy, so removing config cannot declassify IDs.
pub(crate) fn require_registered(store: &Store, entry: &Namespace) -> Result<()> {
    if persisted(store)?.get(&entry.prefix) != Some(entry) {
        return Err(invalid(
            "namespace is declared but not registered; run namespaces apply before minting",
        ));
    }
    Ok(())
}

fn insert(entries: &mut BTreeMap<String, Namespace>, entry: Namespace) -> Result<()> {
    entry.validate()?;
    if let Some(old) = entries.get(&entry.prefix) {
        if old == &entry {
            return Ok(());
        }
        return Err(invalid(
            "existing prefix cannot be reclassified or reminted",
        ));
    }
    if entries.values().any(|old| {
        old.base_iri.starts_with(&entry.base_iri) || entry.base_iri.starts_with(&old.base_iri)
    }) {
        return Err(invalid("duplicate or overlapping namespace bases"));
    }
    entries.insert(entry.prefix.clone(), entry);
    Ok(())
}

/// Explicit owner action: preserve declarations in the reserved graph.
pub fn apply(store: &mut Store) -> Result<serde_json::Value> {
    let entries = list(store)?;
    if entries.is_empty() {
        return Ok(serde_json::json!({"count":0}));
    }
    let graph = store.graph_create(REGISTRY_GRAPH)?;
    let mut text = String::new();
    for entry in &entries {
        let record = serde_json::to_string(entry).map_err(|e| invalid(&e.to_string()))?;
        text.push_str(&format!(
            "<urn:quipu:namespace:{}> <{RECORD}> {} .\n",
            entry.prefix,
            oxrdf::Literal::new_simple_literal(record)
        ));
    }
    let (tx, count) = crate::rdf::ingest_rdf_to_graph(
        store,
        text.as_bytes(),
        oxrdfio::RdfFormat::NTriples,
        None,
        &crate::time::now_iso(),
        None,
        Some("namespace-config"),
        graph,
    )?;
    Ok(serde_json::json!({"tx_id":tx,"count":count,"namespaces":entries}))
}

/// Prove audience membership from protected local configuration.
pub(crate) fn destination_audience<'a>(
    store: &'a Store,
    destination: &'a crate::share_scrub::ShareDestination,
) -> Result<&'a str> {
    use crate::share_scrub::ShareDestination;
    match destination {
        ShareDestination::Audience(binding) => {
            if binding.audience == "public"
                || !store.namespace_settings.destinations.contains(binding)
            {
                return Err(Error::PolicyDenied(
                    "namespace audience destination is not owner-bound".into(),
                ));
            }
            Ok(&binding.audience)
        }
        _ => Ok("public"),
    }
}

#[cfg(test)]
#[path = "entity_namespaces_tests.rs"]
mod tests;
