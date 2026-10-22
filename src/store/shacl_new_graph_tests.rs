use crate::{Error, Store};

const SHAPES: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <http://ex.test/> .
ex:ThingShape a sh:NodeShape ; sh:targetClass ex:Thing ;
    sh:property [ sh:path ex:name ; sh:maxCount 1 ] .";

fn knot(store: &mut Store, ttl: &str) -> crate::Result<serde_json::Value> {
    crate::tool_knot(
        store,
        &serde_json::json!({"turtle": format!("@prefix ex: <http://ex.test/> . {ttl}"),
                            "timestamp": "2026-10-22T00:00:00Z"}),
    )
}

fn loaded(shapes: &str) -> Store {
    let store = Store::open_in_memory().unwrap();
    store
        .load_shapes("t16", shapes, "2026-10-22T00:00:00Z")
        .unwrap();
    store
}

#[test]
fn a_new_graph_larger_than_the_incremental_cap_is_admitted_in_batch() {
    // aegis-t16: 104,000 facts in one write to a graph with no history, under a
    // maxCount shape. The incremental path refused this with CannotVerify
    // ("post-state SHACL context exceeds 100000 facts").
    let mut store = loaded(SHAPES);
    let mut ttl = String::new();
    for i in 0..26_000 {
        ttl.push_str(&format!(
            "ex:t{i} a ex:Thing ; ex:name \"n{i}\" ; ex:p {i} ; ex:q \"v\" .\n"
        ));
    }
    knot(&mut store, &ttl).expect("a conforming new graph is admitted");
    let rows = crate::sparql::query(
        &store,
        "SELECT (COUNT(*) AS ?n) WHERE { ?s a <http://ex.test/Thing> }",
    )
    .unwrap();
    assert_eq!(rows.rows()[0].get("n"), Some(&crate::Value::Int(26_000)));
}

#[test]
fn a_violation_in_a_new_graph_is_refused_using_root_types() {
    // The batch path must see what the payload alone cannot: the focus's type
    // lives in ROOT (an earlier write), the two names in a new named graph.
    // Payload validation has no target here; post-state includes ROOT types.
    let mut store = loaded(SHAPES);
    knot(&mut store, "ex:t a ex:Thing .").unwrap();
    store.graph_create("urn:t16:new").unwrap();
    let g = store.lookup("urn:t16:new").unwrap().unwrap();
    assert!(store.graph_is_new(g, i64::MAX).unwrap());
    let refused = crate::tool_knot(
        &mut store,
        &serde_json::json!({"turtle": r#"@prefix ex: <http://ex.test/> . ex:t ex:name "a" , "b" ."#,
                            "graph": "urn:t16:new", "timestamp": "2026-10-22T00:00:00Z"}),
    );
    assert!(
        matches!(&refused, Err(Error::ValidationFailed { violations: 1, .. })),
        "{refused:?}"
    );
    assert!(
        store.graph_is_new(g, i64::MAX).unwrap(),
        "nothing was written"
    );
}

#[test]
fn a_warning_in_a_new_graph_does_not_refuse() {
    let mut store =
        loaded(&SHAPES.replace("sh:maxCount 1", "sh:maxCount 1 ; sh:severity sh:Warning"));
    knot(&mut store, r#"ex:t a ex:Thing ; ex:name "a" , "b" ."#).expect("warnings report only");
}

#[test]
fn a_graph_with_any_history_is_not_new() {
    let mut store = loaded(SHAPES);
    assert!(store.graph_is_new(0, i64::MAX).unwrap());
    knot(&mut store, r#"ex:t a ex:Thing ; ex:name "a" ."#).unwrap();
    // Another transaction's view: the graph now has history, so it stays on the
    // incremental path (and its cap).
    assert!(!store.graph_is_new(0, i64::MAX).unwrap());
}
