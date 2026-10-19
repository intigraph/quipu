//! Read-only ROOT fact history for the receiving browser store.
use crate::{Explorer, err_js};
use quipu::{Store, Value};
use serde_json::{Value as Json, json};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl Explorer {
    /// Receiver-local assertions and retractions, with valid and transaction time.
    /// Producer history absent from the imported RDF is never reconstructed.
    ///
    /// # Errors
    /// A dictionary, fact or transaction read fails.
    #[wasm_bindgen(js_name = factHistory)]
    pub fn fact_history(&self, entity: &str) -> Result<String, JsValue> {
        read_history(&self.store, entity)
            .map(|v| v.to_string())
            .map_err(err_js)
    }
}

fn read_history(store: &Store, entity: &str) -> quipu::Result<Json> {
    let Some(id) = store.lookup(entity)? else {
        return Ok(json!({"facts": [], "truncated": false, "scope": "receiver-root"}));
    };
    let facts = store.entity_history(id)?;
    let truncated = facts.len() > 2000;
    let mut rows = Vec::new();
    let mut transactions = std::collections::BTreeMap::new();
    for f in facts.into_iter().take(2000) {
        if let std::collections::btree_map::Entry::Vacant(entry) = transactions.entry(f.tx) {
            entry.insert(store.get_transaction(f.tx)?);
        }
        let tx = transactions.get(&f.tx).and_then(Option::as_ref);
        let object = match &f.value {
            Value::Ref(id) => json!({"kind":"iri", "value":store.resolve(*id)?}),
            value => json!({"kind":"literal", "value":value.as_lexical().map_or_else(
                || format!("{value:?}"), str::to_owned), "datatype":value.datatype(), "language":value.language()}),
        };
        rows.push(json!({"predicate":store.resolve(f.attribute)?, "object":object,
            "tx":f.tx, "valid_from":f.valid_from, "valid_to":f.valid_to,
            "operation":format!("{:?}",f.op), "transaction_time":tx.map(|t| &t.timestamp),
            "source":tx.and_then(|t| t.source.as_ref()), "actor":tx.and_then(|t| t.actor.as_ref())}));
    }
    Ok(json!({"facts":rows,"truncated":truncated,"scope":"receiver-root"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quipu::{Datum, Op};

    #[test]
    fn exact_receiver_history_keeps_retractions_and_provenance() {
        let mut store = Store::open_in_memory().unwrap();
        let e = store.intern("urn:test:subject").unwrap();
        let a = store.intern("urn:test:predicate").unwrap();
        let o = store.intern("urn:test:object").unwrap();
        for (op, time, source) in [
            (Op::Assert, "2026-10-19T00:00:00Z", "receiver-assert"),
            (Op::Retract, "2026-10-19T01:00:00Z", "receiver-retract"),
        ] {
            store
                .transact(
                    &[Datum {
                        entity: e,
                        attribute: a,
                        value: Value::Ref(o),
                        valid_from: "2026-10-01T00:00:00Z".into(),
                        valid_to: None,
                        op,
                    }],
                    time,
                    Some("tester"),
                    Some(source),
                )
                .unwrap();
        }
        let out = read_history(&store, "urn:test:subject").unwrap();
        let rows = out["facts"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["object"]["value"], "urn:test:object");
        assert_eq!(rows[0]["valid_from"], "2026-10-01T00:00:00Z");
        assert_eq!(rows[0]["transaction_time"], "2026-10-19T00:00:00Z");
        assert_eq!(rows[1]["operation"], "Retract");
        assert_eq!(rows[1]["source"], "receiver-retract");
        assert_eq!(out["scope"], "receiver-root");
        assert_eq!(
            read_history(&store, "urn:test:missing").unwrap()["facts"],
            json!([])
        );
    }
}
