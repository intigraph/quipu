//! Explicit addresses must use the same identity, validation and hash path.
use super::*;
use crate::{tool_episode, tool_query, tool_shapes};
use serde_json::{Value, json};

const ENTITY: &str = "https://example.org/site/worker";
const CLASS: &str = "https://example.org/vocabulary#Host";

fn payload() -> Value {
    json!({"name":"explicit-address", "source":"fixture", "nodes":[
        {"name":"worker.alpha", "type":"Host", "iri":ENTITY}
    ]})
}

fn ask(store: &Store, query: &str) -> bool {
    tool_query(store, &json!({"query":query})).unwrap()["result"] == true
}

fn governed() -> Store {
    let mut store = Store::open_in_memory_test_ns().unwrap();
    tool_shapes(&mut store, &json!({"action":"load", "name":"explicit-types",
        "turtle":format!("@prefix sh: <http://www.w3.org/ns/shacl#> . \
             <https://example.org/vocabulary#HostShape> a sh:NodeShape; sh:targetClass <{CLASS}> .")
    })).unwrap();
    store
}

#[test]
fn explicit_address_and_loaded_class_are_independent_of_the_legacy_base() {
    let mut store = governed();
    let mut input = payload();
    input["nodes"][0]["type"] = json!(CLASS);
    tool_episode(&mut store, &input).unwrap();
    assert!(ask(
        &store,
        &format!("ASK {{ <{ENTITY}> a ?t . FILTER(?t=<{CLASS}>) }}")
    ));
    assert!(!ask(
        &store,
        &format!(
            "ASK {{ <{}worker.alpha> ?p ?o }}",
            namespace::DEFAULT_BASE_NS
        )
    ));
}

#[test]
fn edges_and_confidence_reification_use_the_explicit_node_addresses() {
    let mut store = Store::open_in_memory_test_ns().unwrap();
    let mut input = payload();
    input["nodes"].as_array_mut().unwrap().push(json!({
        "name":"worker.beta", "type":"Service", "iri":"https://example.org/site/service"}));
    input["edges"] = json!([{"source":"worker.alpha", "target":"worker.beta",
        "relation":"runs_on", "confidence":"EXTRACTED"}]);
    tool_episode(&mut store, &input).unwrap();
    assert!(ask(
        &store,
        &format!(
            "ASK {{ <{ENTITY}> <{}runs_on> <https://example.org/site/service> }}",
            namespace::DEFAULT_BASE_NS
        )
    ));
    assert!(ask(
        &store,
        &format!(
            "ASK {{ ?s <{}subject> <{ENTITY}>; <{}object> <https://example.org/site/service> }}",
            namespace::RDF,
            namespace::RDF
        )
    ));
}

#[test]
fn malformed_explicit_addresses_refuse_before_any_transaction() {
    for iri in [
        "",
        "relative/path",
        " https://example.org/site/worker",
        "https://example.org/a b",
        "https://example.org/a> . <https://example.org/injected>",
    ] {
        let mut store = Store::open_in_memory_test_ns().unwrap();
        let before = store.latest_tx_id().unwrap();
        let mut input = payload();
        input["nodes"][0]["iri"] = json!(iri);
        assert!(
            tool_episode(&mut store, &input).is_err(),
            "accepted malformed address {iri:?}"
        );
        assert_eq!(before, store.latest_tx_id().unwrap());
    }
}

#[test]
fn absolute_http_and_urn_addresses_are_preserved_without_normalization() {
    for iri in [
        ENTITY,
        "urn:example:worker",
        "HTTPS://example.org/site/worker",
    ] {
        let mut store = Store::open_in_memory_test_ns().unwrap();
        let mut input = payload();
        input["nodes"][0]["iri"] = json!(iri);
        tool_episode(&mut store, &input).unwrap();
        assert!(ask(&store, &format!("ASK {{ <{iri}> ?p ?o }}")));
    }
}

#[test]
fn a_loaded_absolute_class_alone_preserves_legacy_entity_addressing() {
    let mut store = governed();
    let mut input = payload();
    input["nodes"][0].as_object_mut().unwrap().remove("iri");
    input["nodes"][0]["type"] = json!(CLASS);
    tool_episode(&mut store, &input).unwrap();
    assert!(ask(
        &store,
        &format!(
            "ASK {{ <{}worker.alpha> a ?t . FILTER(?t=<{CLASS}>) }}",
            namespace::DEFAULT_BASE_NS
        )
    ));
    assert!(!ask(&store, &format!("ASK {{ <{ENTITY}> ?p ?o }}")));
}

#[test]
fn two_names_cannot_claim_one_explicit_address_in_a_batch() {
    let mut store = Store::open_in_memory_test_ns().unwrap();
    let mut input = payload();
    input["nodes"].as_array_mut().unwrap().push(json!({
        "name":"different label", "type":"Host", "iri":ENTITY}));
    let before = store.latest_tx_id().unwrap();
    assert!(tool_episode(&mut store, &input).is_err());
    assert_eq!(before, store.latest_tx_id().unwrap());
}

#[test]
fn explicit_and_legacy_entries_cannot_collide_on_one_subject() {
    let mut store = Store::open_in_memory_test_ns().unwrap();
    let mut input = payload();
    input["nodes"][0]["iri"] = json!(format!("{}worker_beta", namespace::DEFAULT_BASE_NS));
    input["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"worker_beta", "type":"Host"}));
    let before = store.latest_tx_id().unwrap();
    assert!(tool_episode(&mut store, &input).is_err());
    assert_eq!(before, store.latest_tx_id().unwrap());
}

#[test]
fn mixed_class_spellings_do_not_hide_in_batch_aliases() {
    let mut store = Store::open_in_memory_test_ns().unwrap();
    let mut input = payload();
    input["nodes"].as_array_mut().unwrap().push(json!({
        "name":"worker_alpha", "iri":"https://example.org/site/alias",
        "type":format!("{}Host", namespace::DEFAULT_BASE_NS)}));
    let episode: Episode = serde_json::from_value(input).unwrap();
    let opts = IngestResolutionOpts {
        enabled: true,
        strict_mode: true,
        threshold: 0.9,
        top_k: 5,
    };
    let before = store.latest_tx_id().unwrap();
    assert!(
        ingest_episode_with_resolution(
            &mut store,
            &episode,
            "2026-11-08T00:00:00Z",
            namespace::DEFAULT_BASE_NS,
            Some(&opts)
        )
        .is_err()
    );
    assert_eq!(before, store.latest_tx_id().unwrap());
}

#[test]
fn an_absolute_type_still_needs_the_loaded_vocabulary() {
    let mut store = governed();
    let mut input = payload();
    let unknown = "https://example.org/vocabulary#Unloaded";
    input["nodes"][0]["type"] = json!(unknown);
    let before = store.latest_tx_id().unwrap();
    let error = tool_episode(&mut store, &input).unwrap_err().to_string();
    assert!(error.contains(unknown), "{error}");
    assert_eq!(before, store.latest_tx_id().unwrap());
}

#[test]
fn explicit_address_changes_participate_in_the_idempotency_hash() {
    let first: Episode = serde_json::from_value(payload()).unwrap();
    let mut input = payload();
    input["nodes"][0]["iri"] = json!("https://example.org/site/another-worker");
    let second: Episode = serde_json::from_value(input).unwrap();
    assert_ne!(episode_content_hash(&first), episode_content_hash(&second));
}

#[test]
fn explicit_addressing_preserves_strict_alias_refusal_and_exact_reuse() {
    let mut store = Store::open_in_memory_test_ns().unwrap();
    let opts = IngestResolutionOpts {
        enabled: true,
        strict_mode: true,
        threshold: 0.9,
        top_k: 5,
    };
    let first: Episode = serde_json::from_value(payload()).unwrap();
    ingest_episode_with_resolution(
        &mut store,
        &first,
        "2026-11-08T00:00:00Z",
        namespace::DEFAULT_BASE_NS,
        Some(&opts),
    )
    .unwrap();
    assert!(ask(&store, &format!("ASK {{ <{ENTITY}> ?p ?o }}")));
    let mut input = payload();
    input["name"] = json!("explicit-alias");
    input["nodes"][0]["name"] = json!("worker_alpha");
    input["nodes"][0]["iri"] = json!("https://example.org/site/alias");
    let alias: Episode = serde_json::from_value(input).unwrap();
    let before = store.latest_tx_id().unwrap();
    assert!(
        ingest_episode_with_resolution(
            &mut store,
            &alias,
            "2026-11-08T00:00:01Z",
            namespace::DEFAULT_BASE_NS,
            Some(&opts)
        )
        .is_err()
    );
    assert_eq!(before, store.latest_tx_id().unwrap());
    ingest_episode_with_resolution(
        &mut store,
        &first,
        "2026-11-08T00:00:02Z",
        namespace::DEFAULT_BASE_NS,
        Some(&opts),
    )
    .unwrap();
}

#[test]
fn legacy_names_are_not_reinterpreted_as_absolute_addresses() {
    let mut store = Store::open_in_memory_test_ns().unwrap();
    let input =
        json!({"name":"legacy-address", "nodes":[{"name":"code/demo/source.rs", "type":"Host"}]});
    tool_episode(&mut store, &input).unwrap();
    assert!(ask(
        &store,
        &format!(
            "ASK {{ <{}code/demo/source.rs> ?p ?o }}",
            namespace::DEFAULT_BASE_NS
        )
    ));
    let mut bad = input;
    bad["nodes"][0]["name"] = json!(ENTITY);
    assert!(tool_episode(&mut store, &bad).is_err());
}
