use serde_json::json;

use crate::{Datum, Op, Store, Value, tool_entity_history};

#[test]
fn entity_history_preserves_assertion_retraction_and_native_value() {
    let mut store = Store::open_in_memory().unwrap();
    let entity = store.intern("urn:history:entity").unwrap();
    let predicate = store.intern("urn:history:value").unwrap();
    let mut fact = Datum {
        entity,
        attribute: predicate,
        value: Value::Int(9_007_199_254_740_993),
        valid_from: "2020-01-01T00:00:00Z".into(),
        valid_to: None,
        op: Op::Assert,
    };
    let first = store
        .transact(&[fact.clone()], "2026-01-01T00:00:00Z", None, None)
        .unwrap();
    fact.op = Op::Retract;
    let second = store
        .transact(&[fact], "2026-02-01T00:00:00Z", None, None)
        .unwrap();

    let result = tool_entity_history(&store, &json!({"iri": "urn:history:entity"})).unwrap();
    assert_eq!(result["count"], 2);
    assert_eq!(result["history"][0]["op"], "assert");
    assert_eq!(result["history"][1]["op"], "retract");
    assert_eq!(result["history"][0]["predicate"], "urn:history:value");
    assert_eq!(result["history"][0]["tx"], first);
    assert_eq!(result["history"][1]["tx"], second);
    assert_eq!(result["history"][0]["valid_from"], "2020-01-01T00:00:00Z");
    assert_eq!(
        result["history"][0]["value"],
        json!(9_007_199_254_740_993_i64)
    );
    assert!(store.entity_facts(entity).unwrap().is_empty());
}

#[test]
fn missing_history_refuses_without_interning_or_writing() {
    let store = Store::open_in_memory().unwrap();
    let before = store.latest_tx_id().unwrap();
    for input in [
        json!({}),
        json!({"iri": 7}),
        json!({"iri": "urn:history:absent"}),
    ] {
        assert!(tool_entity_history(&store, &input).is_err());
    }
    assert_eq!(store.lookup("urn:history:absent").unwrap(), None);
    assert_eq!(store.latest_tx_id().unwrap(), before);
    assert!(store.current_facts().unwrap().is_empty());
}

#[test]
fn an_existing_entity_without_facts_has_explicit_empty_history() {
    let store = Store::open_in_memory().unwrap();
    store.intern("urn:history:known").unwrap();
    let result = tool_entity_history(&store, &json!({"iri": "urn:history:known"})).unwrap();
    assert_eq!(
        result,
        json!({"iri": "urn:history:known", "count": 0, "history": []})
    );
}
