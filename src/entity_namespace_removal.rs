//! Deliberately narrow, owner-local inverse for unused namespace declarations.
use super::{Namespace, RECORD, REGISTRY_GRAPH, invalid, list, persisted};
use crate::{Datum, Error, Op, Result, Store, Value};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The reviewed predecessor and exact declaration, including all selectors.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RemovalRequest {
    pub expected_registry_hash: String,
    #[serde(deserialize_with = "exact_namespace")]
    pub namespace: Namespace,
    /// Explicit owner-reviewed inventory budget; exhaustion is UNKNOWN.
    #[serde(default = "default_inventory_limit")]
    pub max_reference_facts: u64,
}

fn default_inventory_limit() -> u64 {
    100_000
}

fn exact_namespace<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Namespace, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    let required = [
        "prefix",
        "base_iri",
        "visibility",
        "audience",
        "resolver_url",
        "minting",
        "graphs",
        "types",
    ];
    if value
        .as_object()
        .is_none_or(|object| required.iter().any(|key| !object.contains_key(*key)))
    {
        return Err(serde::de::Error::custom(
            "complete exact namespace declaration required",
        ));
    }
    serde_json::from_value(value).map_err(serde::de::Error::custom)
}

/// Store-bound, transaction-bound effective registry digest. An intervening
/// write, even an apply/remove ABA, invalidates a previously reviewed request.
pub fn registry_hash(store: &Store) -> Result<String> {
    let snapshot = store
        .conn
        .is_autocommit()
        .then(|| store.conn.unchecked_transaction())
        .transpose()?;
    let bytes = serde_json::to_vec(&(
        "namespace-removal-v1",
        store.store_id()?,
        store.log_epoch()?,
        store.latest_tx_id()?,
        list(store)?,
        persisted(store)?,
    ))
    .map_err(|e| invalid(&e.to_string()))?;
    if let Some(snapshot) = snapshot {
        snapshot.commit()?;
    }
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

/// No HTTP/MCP or authenticated principal may enter this local-owner lane.
/// Remove config first; retained policy remains effective until this succeeds.
pub fn remove(store: &mut Store, request: &RemovalRequest) -> Result<serde_json::Value> {
    owner_change(store, |store| remove_locked(store, request))
}

/// Restore/apply only the exact reviewed declaration, under the same barrier.
/// Configuration must already contain it; this does not take policy from RDF.
pub fn apply_if(store: &mut Store, request: &RemovalRequest) -> Result<serde_json::Value> {
    owner_change(store, |store| {
        if registry_hash(store)? != request.expected_registry_hash {
            return Err(Error::ConditionalWriteConflict);
        }
        let prior = persisted(store)?;
        if !store
            .namespace_settings
            .namespaces
            .contains(&request.namespace)
            || store
                .namespace_settings
                .namespaces
                .iter()
                .any(|entry| entry != &request.namespace && prior.get(&entry.prefix) != Some(entry))
        {
            return Err(invalid(
                "conditional apply requires the exact configured declaration only",
            ));
        }
        super::apply(store)
    })
}

fn owner_change(
    store: &mut Store,
    work: impl FnOnce(&mut Store) -> Result<serde_json::Value>,
) -> Result<serde_json::Value> {
    if crate::transaction_auth::current_identity().is_some() || !store.conn.is_autocommit() {
        return Err(Error::PolicyDenied(
            "namespace removal is owner-local only".into(),
        ));
    }
    store.ensure_ownership_writable()?;
    store.conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = work(store).and_then(|value| {
        crate::store::release::settle(&store.conn, store.conn.execute_batch("COMMIT"))?;
        Ok(value)
    });
    if result.is_err() {
        let _ = store.conn.execute_batch("ROLLBACK");
        store.term_cache.borrow_mut().clear_persistent();
        store.read_model.borrow_mut().clear();
        store.clear_pending_write_events();
    }
    result
}

fn remove_locked(store: &mut Store, request: &RemovalRequest) -> Result<serde_json::Value> {
    if registry_hash(store)? != request.expected_registry_hash {
        return Err(Error::ConditionalWriteConflict);
    }
    let entry = &request.namespace;
    let unavailable: bool = store.conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM main.graphs WHERE source IS NOT NULL OR lifecycle='frozen')",
        [],
        |row| row.get(0),
    )?;
    if !store.attachments.is_empty() || unavailable {
        return Err(invalid("attached or frozen reference coverage is unknown"));
    }
    if persisted(store)?.get(&entry.prefix) != Some(entry)
        || store
            .namespace_settings
            .namespaces
            .iter()
            .any(|ns| ns.prefix == entry.prefix)
    {
        return Err(invalid(
            "remove requires exact retained policy absent from config",
        ));
    }
    // The dictionary is allocation history, including retracted identities.
    // substr uses literal matching: '_' and '%' in a base are never wildcards.
    let allocated: bool = store.conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM main.terms WHERE substr(iri,1,length(?1))=?1)",
        [&entry.base_iri],
        |row| row.get(0),
    )?;
    if allocated {
        return Err(invalid("namespace has allocated or referenced terms"));
    }
    // This first inverse intentionally refuses ANY local writer enrollment.
    // Missing tables/unknown schema are errors, never an empty grant inventory.
    for table in ["writer_grants", "client_changeset_grants"] {
        let enrolled: bool = store.conn.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM main.{table})"),
            [],
            |row| row.get(0),
        )?;
        if enrolled {
            return Err(invalid("local grants prevent namespace removal"));
        }
    }
    let graph = store
        .lookup(REGISTRY_GRAPH)?
        .ok_or_else(|| invalid("missing registry graph"))?;
    if store.graph_class(graph)?.as_deref() != Some("committed") {
        return Err(invalid("registry must be a local committed graph"));
    }
    let subject_iri = format!("urn:quipu:namespace:{}", entry.prefix);
    let subject = store
        .lookup(&subject_iri)?
        .ok_or_else(|| invalid("missing declaration subject"))?;
    let predicate = store
        .lookup(RECORD)?
        .ok_or_else(|| invalid("missing declaration predicate"))?;
    let record = serde_json::to_string(entry).map_err(|e| invalid(&e.to_string()))?;
    let facts: i64 = store
        .conn
        .query_row("SELECT count(*) FROM main.facts", [], |r| r.get(0))?;
    if u64::try_from(facts).map_err(|_| invalid("unknown reference count"))?
        > request.max_reference_facts
    {
        return Err(invalid(
            "reference inventory exceeds removal bound; coverage unknown",
        ));
    }
    // Historical foreign declarations and literal references are conservative
    // blockers too. Never silently ignore a foreign graph or undecodable value.
    let mut statement = store.conn.prepare("SELECT g,e,a,v FROM main.facts")?;
    for row in statement.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, Vec<u8>>(3)?,
        ))
    })? {
        let (g, e, a, bytes) = row?;
        let value = Value::from_bytes(&bytes)?;
        if g == graph && e == subject && a == predicate && value == Value::Str(record.clone()) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        let literal_reference = match &value {
            Value::Str(text)
            | Value::Lang { lexical: text, .. }
            | Value::Typed { lexical: text, .. } => references(text, entry),
            _ => false,
        };
        if e == subject
            || a == subject
            || value == Value::Ref(subject)
            || text.contains(&entry.base_iri)
            || text.contains(&subject_iri)
            || literal_reference
        {
            return Err(invalid("namespace has historical or foreign references"));
        }
    }
    drop(statement);
    if let Some(shapes) = store.get_combined_shapes()? {
        let parser = oxttl::TurtleParser::new()
            .with_base_iri(store.base_ns())
            .map_err(|_| invalid("unknown shapes base"))?;
        for triple in parser.for_reader(shapes.as_bytes()) {
            let triple = triple.map_err(|_| invalid("unknown shapes references"))?;
            if references(&triple.to_string(), entry) || triple.to_string().contains(&subject_iri) {
                return Err(invalid("namespace is referenced by loaded shapes"));
            }
        }
    }
    let datum = Datum {
        entity: subject,
        attribute: predicate,
        value: Value::Str(record),
        valid_from: crate::time::now_iso(),
        valid_to: None,
        op: Op::Retract,
    };
    let tx = store.transact_to_graph(
        &[datum],
        &crate::time::now_iso(),
        None,
        Some("namespace-owner-remove"),
        graph,
    )?;
    if persisted(store)?.contains_key(&entry.prefix) {
        return Err(invalid("declaration survived removal"));
    }
    Ok(serde_json::json!({"tx_id":tx,"removed":entry,"namespaces":list(store)?}))
}

fn references(text: &str, entry: &Namespace) -> bool {
    if text.contains(&entry.base_iri) || text.contains(&entry.prefix) {
        return true;
    }
    // Reference descriptors may escape their strings in JSON.
    fn json_reference(value: &serde_json::Value, entry: &Namespace) -> bool {
        match value {
            serde_json::Value::String(text) => {
                text.contains(&entry.base_iri) || text.contains(&entry.prefix)
            }
            serde_json::Value::Array(values) => values.iter().any(|v| json_reference(v, entry)),
            serde_json::Value::Object(values) => values
                .iter()
                .any(|(k, v)| k.contains(&entry.prefix) || json_reference(v, entry)),
            _ => false,
        }
    }
    serde_json::from_str(text).is_ok_and(|value| json_reference(&value, entry))
}

#[cfg(test)]
#[path = "entity_namespace_removal_tests.rs"]
mod tests;
