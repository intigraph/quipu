use super::*;

const TS: &str = "2026-10-20T00:00:00Z";

#[test]
fn native_validation_details_survive_the_transport_error() {
    let native = crate::error::Error::ValidationFailed {
        violations: 1,
        messages: vec!["required title missing".into()],
    };
    let error = BindingError::native("quipu_knot", native);
    let json = serde_json::to_value(&error).unwrap();
    assert_eq!(json["details"]["violations"], 1);
    assert_eq!(json["details"]["messages"][0], "required title missing");
    assert_eq!(json["commit_state"], "unknown");
    assert!(std::error::Error::source(&error).is_some());
}

#[test]
fn manifest_and_dispatch_share_exact_names() {
    let mut engine = Engine::open_in_memory().unwrap();
    let caps = engine.capabilities();
    assert_eq!(caps["full_parity"], false);
    let names: std::collections::BTreeSet<_> = OPERATIONS.iter().map(|op| op.name).collect();
    assert_eq!(names.len(), OPERATIONS.len());
    for op in OPERATIONS {
        let result = engine.invoke(op.name, &json!({}));
        if !op.requires_shacl || cfg!(feature = "shacl") {
            assert_ne!(
                result.as_ref().err().map(|e| e.code),
                Some("unsupported_capability")
            );
        } else {
            assert_eq!(result.unwrap_err().commit_state, "not_started");
        }
    }
    assert_eq!(caps["operations"].as_array().unwrap().len(), names.len());
}

#[test]
fn unsupported_and_malformed_calls_never_enter_a_handler() {
    let mut engine = Engine::open_in_memory().unwrap();
    let query = json!({"query": "SELECT ?s WHERE { ?s ?p ?o }"});
    let before = engine.invoke("quipu_query", &query).unwrap();
    let unsupported = engine
        .invoke("quipu_update", &json!({"entity": "urn:never"}))
        .unwrap_err();
    assert_eq!(unsupported.code, "unsupported_capability");
    assert_eq!(unsupported.commit_state, "not_started");
    assert!(!unsupported.retryable);
    let malformed = engine.invoke_json("quipu_episode", "{").unwrap_err();
    assert_eq!(malformed.code, "invalid_json");
    assert_eq!(malformed.commit_state, "not_started");
    assert_eq!(before, engine.invoke("quipu_query", &query).unwrap());
    assert!(engine.store.lookup("urn:never").unwrap().is_none());
}

#[test]
fn native_errors_do_not_claim_rollback_or_retry() {
    let mut engine = Engine::open_in_memory().unwrap();
    let error = engine
        .invoke("quipu_entity_history", &json!({"iri": "urn:absent"}))
        .unwrap_err();
    assert_eq!(error.code, "engine_error");
    assert_eq!(error.commit_state, "unknown");
    assert!(!error.retryable);
    assert!(engine.store.lookup("urn:absent").unwrap().is_none());
}

#[cfg(not(feature = "shacl"))]
#[test]
fn unlinked_validator_cannot_be_silently_bypassed() {
    let mut engine = Engine::open_in_memory().unwrap();
    let error = engine
        .invoke("quipu_knot", &json!({"turtle": "<urn:a> <urn:b> <urn:c>."}))
        .unwrap_err();
    assert_eq!(error.code, "unsupported_capability");
    assert_eq!(error.commit_state, "not_started");
    assert!(engine.store.lookup("urn:a").unwrap().is_none());
    assert_eq!(engine.capabilities()["features"]["shacl"], false);
}

#[cfg(feature = "shacl")]
fn load_shape(engine: &mut Engine) {
    engine.invoke("quipu_shapes", &json!({
        "action": "load", "name": "binding-control", "timestamp": TS,
        "turtle": "@prefix sh: <http://www.w3.org/ns/shacl#> . <urn:RecordShape> a sh:NodeShape; sh:targetClass <urn:Record>; sh:property [sh:path <urn:title>; sh:minCount 1] ."
    })).unwrap();
}

#[cfg(feature = "shacl")]
#[test]
fn adopting_shapes_is_explicit_and_observable() {
    let mut engine = Engine::open_in_memory().unwrap();
    let input =
        json!({"timestamp": TS, "turtle": "<urn:good> a <urn:Record>; <urn:title> \"Before\" ."});
    let refusal = engine.invoke("quipu_knot", &input).unwrap_err();
    assert_eq!(refusal.code, "schema_not_adopted");
    assert_eq!(refusal.commit_state, "not_started");
    assert!(engine.store.lookup("urn:good").unwrap().is_none());
    let before = engine.capabilities()["shape_catalog"].clone();
    load_shape(&mut engine);
    assert_ne!(before, engine.capabilities()["shape_catalog"]);
    assert!(
        engine.invoke("quipu_knot", &input).unwrap()["tx_id"]
            .as_i64()
            .unwrap()
            > 0
    );
    assert_eq!(
        engine
            .invoke(
                "quipu_query",
                &json!({"query": "SELECT ?s WHERE { ?s a <urn:Record> }"})
            )
            .unwrap()["count"],
        1
    );
    engine
        .invoke(
            "quipu_shapes",
            &json!({"action": "remove", "name": "binding-control"}),
        )
        .unwrap();
    assert_eq!(engine.capabilities()["schema_state"], "not_adopted");
    let change =
        json!({"entity": "urn:good", "predicate": "urn:title", "value": {"str": "Unvalidated"}});
    assert_eq!(
        engine
            .invoke("quipu_set", &change)
            .unwrap_err()
            .commit_state,
        "not_started"
    );
}

#[cfg(feature = "shacl")]
#[test]
fn shared_handlers_preserve_shacl_refusal_and_lossless_history() {
    let mut engine = Engine::open_in_memory().unwrap();
    load_shape(&mut engine);
    let good = engine.invoke("quipu_knot", &json!({
        "timestamp": TS,
        "turtle": "<urn:good> a <urn:Record>; <urn:title> \"Before\"; <urn:n> 9007199254740993 ."
    })).unwrap();
    assert!(good["tx_id"].as_i64().unwrap() > 0);
    let bad = engine.invoke(
        "quipu_knot",
        &json!({"timestamp": TS, "turtle": "<urn:bad> a <urn:Record> ."}),
    );
    assert!(bad.is_err() || bad.unwrap()["conforms"] == false);
    let rows = engine
        .invoke(
            "quipu_query",
            &json!({"query": "SELECT ?s WHERE { ?s a <urn:Record> }"}),
        )
        .unwrap();
    assert_eq!(rows["count"], 1); // populated positive control and refused-write zero
    engine.invoke("quipu_set", &json!({"entity": "urn:good", "predicate": "urn:title", "value": {"str": "After"}, "timestamp": "2026-10-20T00:01:00Z"})).unwrap();
    let raw = engine
        .invoke_json("quipu_entity_history", r#"{"iri":"urn:good"}"#)
        .unwrap();
    assert!(raw.contains("9007199254740993"));
    assert!(!raw.contains("9007199254740992"));
    let history: Value = serde_json::from_str(&raw).unwrap();
    assert!(
        history["history"]
            .as_array()
            .unwrap()
            .iter()
            .any(|fact| fact["value"] == "Before")
    );
    assert!(
        history["history"]
            .as_array()
            .unwrap()
            .iter()
            .any(|fact| fact["value"] == "After")
    );
}

#[cfg(feature = "shacl")]
#[test]
fn binding_and_direct_native_operations_return_the_same_results() {
    let mut facade = Engine::open_in_memory().unwrap();
    let mut direct = Engine::open_in_memory().unwrap();
    load_shape(&mut facade);
    load_shape(&mut direct);
    let input =
        json!({"timestamp": TS, "turtle": "<urn:good> a <urn:Record>; <urn:title> \"Before\" ."});
    assert_eq!(
        facade.invoke("quipu_knot", &input).unwrap(),
        crate::tool_knot(&mut direct.store, &input).unwrap()
    );
    let input = json!({"entity": "urn:good", "predicate": "urn:title", "value": {"str": "After"}, "timestamp": "2026-10-20T00:01:00Z"});
    assert_eq!(
        facade.invoke("quipu_set", &input).unwrap(),
        crate::tool_set(&mut direct.store, &input).unwrap()
    );
    let input = json!({"iri": "urn:good"});
    assert_eq!(
        facade.invoke("quipu_entity_history", &input).unwrap(),
        crate::tool_entity_history(&direct.store, &input).unwrap()
    );
    let input = json!({"query": "SELECT ?p ?v WHERE { <urn:good> ?p ?v }"});
    assert_eq!(
        facade.invoke("quipu_query", &input).unwrap(),
        crate::tool_query(&direct.store, &input).unwrap()
    );
}

/// The source's feed, one transaction per page, as frame text.
fn feed_of(source: &Store) -> Vec<String> {
    let mut pages = Vec::new();
    let mut cursor = 0;
    loop {
        let page = source
            .changes_after(
                cursor,
                1,
                crate::store::changes::Capture::OldAndNewValues,
                None,
            )
            .unwrap();
        if page.records.is_empty() {
            return pages;
        }
        cursor = page.next_tx;
        pages.push(page.to_json().to_string());
    }
}

fn record_source() -> Store {
    let mut source = Store::open_in_memory().unwrap();
    let typ = source
        .intern("http://www.w3.org/1999/02/22-rdf-syntax-ns#type")
        .unwrap();
    let r = source.intern("urn:r1").unwrap();
    let class = source.intern("urn:Record").unwrap();
    let datum = crate::store::Datum {
        entity: r,
        attribute: typ,
        value: crate::types::Value::Ref(class),
        valid_from: TS.into(),
        valid_to: None,
        op: crate::types::Op::Assert,
    };
    source.transact(&[datum], TS, None, None).unwrap();
    source
}

#[test]
fn a_replica_applies_its_source_feed_and_refuses_local_writes() {
    let source = record_source();
    let mut engine = Engine::open_in_memory().unwrap();
    assert_eq!(engine.replica_progress(), Value::Null);
    for page in feed_of(&source) {
        let outcome = engine
            .replica_apply_frame("quipu.changes", &page, 10)
            .unwrap();
        assert_eq!(outcome["kind"], "applied", "{outcome}");
    }
    assert_eq!(
        engine.replica_progress()["source_tx"],
        json!(source.latest_tx_id().unwrap())
    );
    assert_eq!(engine.replica_freshness(15)["last_frame_age_ms"], json!(5));
    assert_eq!(engine.replica_freshness(15)["bounded"], json!(false));
    // Reads still work.
    let read = engine
        .invoke(
            "quipu_query",
            &json!({"query": "SELECT ?s WHERE { ?s a <urn:Record> }"}),
        )
        .unwrap();
    assert!(read.to_string().contains("urn:r1"), "{read}");
    // Local writes are refused before dispatch: only the feed writes here.
    let refused = engine
        .invoke(
            "quipu_set",
            &json!({"entity": "urn:r1", "attribute": "urn:title", "value": "x"}),
        )
        .unwrap_err();
    assert_eq!(refused.code, "read_only_replica");
    assert_eq!(refused.commit_state, "not_started");
}

/// The source already judged what it accepted: a replica that re-judged it
/// against its own shapes could refuse it and silently fork.
#[cfg(feature = "shacl")]
#[test]
fn a_replica_applies_source_data_its_local_shapes_would_refuse() {
    // Control: the same data written locally IS refused by the adopted shape.
    let mut local = Engine::open_in_memory().unwrap();
    load_shape(&mut local);
    let control = local.invoke(
        "quipu_knot",
        &json!({"turtle": "<urn:r1> a <urn:Record> .", "timestamp": TS}),
    );
    let refused = match control {
        Err(_) => true,
        Ok(v) => v.get("conforms") == Some(&json!(false)),
    };
    assert!(
        refused,
        "control: the shape must refuse an untitled record locally"
    );

    let source = record_source();
    let mut replica = Engine::open_in_memory().unwrap();
    load_shape(&mut replica);
    for page in feed_of(&source) {
        let outcome = replica
            .replica_apply_frame("quipu.changes", &page, 1)
            .unwrap();
        assert_eq!(outcome["kind"], "applied", "{outcome}");
    }
    assert!(replica.store.lookup("urn:r1").unwrap().is_some());
    // Validation is restored for anything that is not the applier.
    assert!(replica.store.shacl_config_mut().validate_on_write);
}
