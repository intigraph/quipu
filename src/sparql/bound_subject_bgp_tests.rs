//! aegis-bqq6: a constant-subject BGP never builds the whole-graph model.

use super::super::query;
use crate::store::Store;
use crate::types::{Op, Value};

const G: &str = "urn:example:seeds";

/// A named graph of `items` seeds-like items, each with a status and priority,
/// so a predicate scan would read every one of them.
fn store(items: usize) -> (Store, i64) {
    let mut store = Store::open_in_memory().unwrap();
    let graph = store.overlay_create(G, 0).unwrap();
    let status = store.intern("urn:example:status").unwrap();
    let priority = store.intern("urn:example:priority").unwrap();
    for n in 0..items {
        let subject = store.intern(&format!("urn:example:item:{n}")).unwrap();
        for (attribute, value) in [
            (
                status,
                Value::Str(if n % 2 == 0 { "open" } else { "closed" }.into()),
            ),
            (priority, Value::Int(i64::try_from(n % 4).unwrap())),
        ] {
            store
                .overlay_write(
                    graph,
                    Op::Assert,
                    subject,
                    attribute,
                    value,
                    "2026-11-27T00:00:00Z",
                )
                .unwrap();
        }
    }
    (store, graph)
}

fn point_read(store: &Store, graph: i64, body: &str) {
    let text = format!("SELECT * WHERE {{ GRAPH <{G}> {{ {body} }} }}");
    let result = query(store, &text).unwrap();
    assert_eq!(result.rows().len(), 1, "{body}");
    let row = &result.rows()[0];
    assert_eq!(row.get("s"), Some(&Value::Str("closed".into())), "{body}");
    assert_eq!(row.get("p"), Some(&Value::Int(3)), "{body}");
    assert!(
        !store.read_model_is_resident_for(graph),
        "a constant-subject BGP must not scan the graph into a read model: {body}"
    );
}

#[test]
fn constant_subject_two_pattern_bgp_does_not_scan_the_predicate() {
    let (store, graph) = store(400);
    let s = "<urn:example:item:7>";
    point_read(
        &store,
        graph,
        &format!("{s} <urn:example:status> ?s . {s} <urn:example:priority> ?p"),
    );
    point_read(
        &store,
        graph,
        &format!("{s} <urn:example:priority> ?p . {s} <urn:example:status> ?s"),
    );
}

#[test]
fn values_seeded_subject_two_pattern_bgp_does_not_scan_the_predicate() {
    let (store, graph) = store(400);
    point_read(
        &store,
        graph,
        "VALUES ?item { <urn:example:item:7> } \
         ?item <urn:example:priority> ?p ; <urn:example:status> ?s",
    );
}

#[test]
fn constant_subject_bgp_with_no_match_is_empty() {
    let (store, graph) = store(10);
    let text = format!(
        "SELECT * WHERE {{ GRAPH <{G}> {{ <urn:example:item:99> <urn:example:status> ?s . \
         <urn:example:item:99> <urn:example:priority> ?p }} }}"
    );
    assert!(query(&store, &text).unwrap().rows().is_empty());
    assert!(!store.read_model_is_resident_for(graph));
}

#[test]
fn free_subject_join_still_uses_the_read_model() {
    // Control: the guard above is meaningful only if a free-subject join in the
    // same graph does reach the model.
    let (store, graph) = store(10);
    let text = format!(
        "SELECT * WHERE {{ GRAPH <{G}> {{ ?i <urn:example:status> ?s . \
         ?i <urn:example:priority> ?p }} }}"
    );
    assert_eq!(query(&store, &text).unwrap().rows().len(), 10);
    assert!(store.read_model_is_resident_for(graph));
}
