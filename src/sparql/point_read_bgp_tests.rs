//! A BGP whose subjects are all fixed is a point read (aegis-bqq6).
//!
//! `<item> status ?s . <item> priority ?p` went to the read-model hash join,
//! which first builds the graph's whole model — a full scan of every current
//! fact — so the two-pattern read took 5-6 s on the seeds graph while either
//! pattern alone took 0.02 s. These tests assert the point read never builds
//! the model (never scans the predicate) and answers what the model would.

use oxrdfio::RdfFormat;
use serde_json::{Value, json};

use crate::rdf::ingest_rdf_to_graph;
use crate::store::Store;

const G: &str = "https://seeds.local/project/aegis";

fn fixture() -> (Store, i64) {
    let mut store = Store::open_in_memory().unwrap();
    let g = store.graph_create(G).unwrap();
    let mut data = String::from("@prefix q: <https://seeds.local/ontology/> .\n");
    for i in 0..200 {
        let pri = i % 4;
        data.push_str(&format!(
            "<https://seeds.local/item/i{i}> q:status \"open\" ; q:priority {pri} .\n"
        ));
    }
    data.push_str("<https://seeds.local/item/tvp> q:status \"open\" ; q:priority 2 .\n");
    ingest_rdf_to_graph(
        &mut store,
        data.as_bytes(),
        RdfFormat::Turtle,
        None,
        "2026-11-27T00:00:00Z",
        None,
        None,
        g,
    )
    .unwrap();
    assert!(store.read_model_enabled(), "the hash-join path must be live");
    (store, g)
}

fn profiled(store: &Store, body: &str) -> (Vec<Value>, Vec<String>) {
    let query = format!(
        "PREFIX q: <https://seeds.local/ontology/> SELECT ?s ?p WHERE {{ GRAPH <{G}> {{ {body} }} }}"
    );
    let mut out = crate::mcp::tool_query(store, &json!({"query": query, "execution_profile": true}))
        .unwrap();
    let profile = out.as_object_mut().unwrap().remove("execution_profile").unwrap();
    let paths = profile["paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["path"].as_str().unwrap().to_string())
        .collect();
    (out["rows"].as_array().unwrap().clone(), paths)
}

#[test]
fn constant_subject_two_pattern_bgp_does_not_scan_the_predicate() {
    let item = "<https://seeds.local/item/tvp>";
    let variants = [
        format!("{item} q:status ?s . {item} q:priority ?p"),
        format!("{item} q:priority ?p . {item} q:status ?s"),
        format!("VALUES ?item {{ {item} }} ?item q:priority ?p ; q:status ?s"),
    ];
    for body in &variants {
        let (store, g) = fixture();
        let (rows, paths) = profiled(&store, body);
        assert_eq!(rows, vec![json!({"p": 2, "s": "open"})], "{body}");
        assert!(
            !paths.iter().any(|p| p == "bgp_read_model_hash"),
            "{body}: point read took the whole-graph hash join: {paths:?}"
        );
        assert!(
            !store.read_model_is_resident_for(g),
            "{body}: a point read built the graph's whole model (a predicate scan)"
        );
    }
}

/// The control: the same store DOES build the model for a join whose subject
/// is free, so the assertion above can fail — it is not vacuous.
#[test]
fn free_subject_join_still_uses_the_read_model() {
    let (store, g) = fixture();
    let (rows, paths) = profiled(&store, "?item q:priority ?p . ?item q:status ?s . FILTER(?p = 9)");
    assert!(rows.is_empty());
    assert!(paths.iter().any(|p| p == "bgp_read_model_hash"), "{paths:?}");
    assert!(store.read_model_is_resident_for(g));
}
