//! Transport-independent entity history, shared by native and language bindings.

use serde_json::{Value as JsonValue, json};

use crate::{Error, Op, Result, Store};

/// Read the existing ROOT-history contract for an exact IRI.
///
/// This is deliberately not a composed or all-named-graphs audit. Missing IRIs
/// are looked up rather than interned, so a failed read cannot create an entity.
pub fn tool_entity_history(store: &Store, input: &JsonValue) -> Result<JsonValue> {
    let iri = input
        .get("iri")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| Error::InvalidValue("missing 'iri' parameter".into()))?;
    let eid = store
        .lookup(iri)?
        .ok_or_else(|| Error::InvalidValue(format!("entity not found: {iri}")))?;
    let entries: Vec<JsonValue> = store
        .entity_history(eid)?
        .iter()
        .map(|fact| {
            json!({
                "op": if fact.op == Op::Assert { "assert" } else { "retract" },
                "predicate": store.resolve(fact.attribute).unwrap_or_default(),
                "value": crate::value_to_json(store, &fact.value),
                "valid_from": fact.valid_from,
                "valid_to": fact.valid_to,
                "tx": fact.tx,
            })
        })
        .collect();
    Ok(json!({ "iri": iri, "history": entries, "count": entries.len() }))
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
