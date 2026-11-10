//! A request can tighten new-entity admission without changing other writers.
use quipu::{
    Store,
    mcp::tools::{
        tool_episode as native_episode, tool_episode_prepare as native_prepare, tool_graph_create,
        tool_shapes,
    },
};
use serde_json::{Value, json};

const GRAPH: &str = "urn:test:governed";
const OTHER: &str = "urn:test:advisory";

fn identified<T>(work: impl FnOnce() -> T) -> T {
    quipu::transaction_auth::with_identity(
        Some(quipu::transaction_auth::Identity {
            principal: "urn:test:writer".into(),
            credential_id: Some("fixture".into()),
            auth_class: "named_bearer".into(),
            attestation: Default::default(),
        }),
        work,
    )
}

fn tool_episode(store: &mut Store, input: &Value) -> quipu::Result<Value> {
    identified(|| native_episode(store, input))
}

fn tool_episode_prepare(
    store: &Store,
    input: &Value,
) -> quipu::Result<Option<quipu::mcp::tools::PreparedEpisode>> {
    identified(|| native_prepare(store, input))
}

#[test]
fn caller_declared_identity_does_not_authenticate() {
    let mut store = store();
    let mut input = episode("anonymous", "document-private", true);
    input["actor"] = json!("urn:test:writer");
    input["principal"] = json!("urn:test:writer");
    let before = store.latest_tx_id().unwrap();
    assert!(native_prepare(&store, &input).is_err());
    assert!(native_episode(&mut store, &input).is_err());
    assert_eq!(before, store.latest_tx_id().unwrap());
}

fn store() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    store.set_base_ns(quipu::namespace::DEFAULT_BASE_NS.to_owned());
    let shapes = format!(
        "@prefix sh: <http://www.w3.org/ns/shacl#> . <urn:s> a sh:NodeShape ; sh:targetClass <{}Document> .",
        store.base_ns()
    );
    tool_shapes(
        &mut store,
        &json!({"action":"load","name":"documents","turtle":shapes}),
    )
    .unwrap();
    for graph in [GRAPH, OTHER] {
        tool_graph_create(&store, &json!({"graph":graph})).unwrap();
    }
    store
}

fn episode(name: &str, node: &str, strict: bool) -> Value {
    json!({"name":name,"graph":GRAPH,"strict_new_entities":strict,
        "nodes":[{"name":node,"type":"Document"}],"edges":[]})
}

#[test]
fn opt_in_refuses_before_transaction_and_does_not_leak() {
    let mut store = store();
    // Resolution is globally OFF, including after each request.
    tool_episode(&mut store, &episode("seed", "document-alpha", true)).unwrap();
    let mut duplicate = episode("duplicate", "document-alpha-variant", true);
    assert!(tool_episode_prepare(&store, &duplicate).unwrap().is_none());
    let before = store.latest_tx_id().unwrap();
    assert!(tool_episode(&mut store, &duplicate).is_err());
    assert_eq!(before, store.latest_tx_id().unwrap());
    assert!(!store.resolution_config().enabled);
    assert!(!store.resolution_config().strict_mode);
    duplicate["graph"] = json!(OTHER);
    duplicate["strict_new_entities"] = json!(false);
    tool_episode(&mut store, &duplicate).unwrap();
    assert!(!store.resolution_config().enabled);
}

#[test]
fn same_iri_and_reasoned_distinct_are_admitted() {
    let mut store = store();
    let first = episode("seed", "document-alpha", true);
    tool_episode(&mut store, &first).unwrap();
    let mut reuse = episode("reuse", "document-alpha-new-label", true);
    reuse["nodes"][0]["iri"] = json!(format!("{}document-alpha", store.base_ns()));
    tool_episode(&mut store, &reuse).unwrap();
    let mut distinct = episode("distinct", "document-alpha-variant", true);
    distinct["nodes"][0]["distinct_from"] = json!([format!("{}document-alpha", store.base_ns())]);
    let before = store.latest_tx_id().unwrap();
    assert!(
        tool_episode(&mut store, &distinct).is_err(),
        "unreasoned escape refused"
    );
    assert_eq!(before, store.latest_tx_id().unwrap());
    distinct["nodes"][0]["properties"] =
        json!({"distinctReason":"different document, manually adjudicated"});
    // The same-IRI relabel also remains a candidate for this exact subject.
    tool_episode(&mut store, &distinct).unwrap();
    for statement in [
        format!(
            "<{}document-alpha-variant> <https://quipu.dev/ontology/distinctFrom> <{}document-alpha>",
            store.base_ns(),
            store.base_ns()
        ),
        format!(
            "<{}document-alpha-variant> <{}distinctReason> \"different document, manually adjudicated\"",
            store.base_ns(),
            store.base_ns()
        ),
    ] {
        assert!(matches!(
            quipu::sparql::query(&store, &format!("ASK FROM <{GRAPH}> {{ {statement} }}")).unwrap(),
            quipu::sparql::QueryResult::Ask(true)
        ));
    }
    assert!(!store.resolution_config().strict_mode);
}

#[test]
fn false_or_omitted_flag_preserves_existing_policy() {
    let mut store = store();
    let mut seed = episode("seed", "document-theta", false);
    seed.as_object_mut().unwrap().remove("strict_new_entities");
    native_episode(&mut store, &seed).unwrap();
    store.resolution_config_mut().enabled = true;
    store.resolution_config_mut().strict_mode = true;
    let input = episode("duplicate", "document-theta-variant", false);
    let before = store.latest_tx_id().unwrap();
    assert!(native_episode(&mut store, &input).is_err());
    assert_eq!(before, store.latest_tx_id().unwrap());
    assert!(store.resolution_config().strict_mode);
}

#[test]
fn invalid_targets_and_non_boolean_flags_never_write() {
    let mut store = store();
    let original = episode("invalid", "different-document", true);
    for graph in [
        Value::Null,
        json!(""),
        json!(quipu::schema::ROOT_GRAPH_IRI),
        json!("urn:test:unregistered"),
        json!("not an IRI"),
    ] {
        let mut input = original.clone();
        input["graph"] = graph;
        let before = store.latest_tx_id().unwrap();
        assert!(tool_episode_prepare(&store, &input).is_err());
        assert!(tool_episode(&mut store, &input).is_err());
        assert_eq!(before, store.latest_tx_id().unwrap());
    }
    let mut input = original;
    input["strict_new_entities"] = json!("true");
    assert!(tool_episode(&mut store, &input).is_err());
}

#[test]
fn strict_writer_sees_a_candidate_committed_after_prepare() {
    let mut store = store();
    let input = episode("strict", "document-zeta-variant", true);
    assert!(tool_episode_prepare(&store, &input).unwrap().is_none());
    tool_episode(&mut store, &episode("between", "document-zeta", false)).unwrap();
    let before = store.latest_tx_id().unwrap();
    assert!(tool_episode(&mut store, &input).is_err());
    assert_eq!(before, store.latest_tx_id().unwrap());
}
