//! One shape must not be able to brick the write path.
//!
//! The post-state cardinality plan is computed over every loaded shape, so a
//! single shape it could not plan used to make EVERY later write refuse with
//! "cannot verify", including writes that never touched that shape.

use crate::Store;

fn knot(store: &mut Store, ttl: &str) -> crate::Result<serde_json::Value> {
    crate::tool_knot(
        store,
        &serde_json::json!({"turtle": format!("@prefix ex: <http://ex.test/> . {ttl}"),
                            "timestamp": "2026-11-04T00:00:00Z"}),
    )
}

const ALTERNATIVE: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <http://ex.test/> .
ex:LabelledShape a sh:NodeShape ; sh:targetClass ex:Labelled ;
    sh:property [ sh:path [ sh:alternativePath ( ex:label ex:altLabel ) ] ; sh:maxCount 1 ] .";

const SEQUENCE: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <http://ex.test/> .
ex:ChainShape a sh:NodeShape ; sh:targetClass ex:Chained ;
    sh:property [ sh:path ( ex:parent ex:name ) ; sh:maxCount 1 ] .";

#[test]
fn an_alternative_path_cardinality_shape_loads_and_leaves_unrelated_writes_working() {
    let mut store = Store::open_in_memory().unwrap();
    store
        .load_shapes("alt", ALTERNATIVE, "2026-11-04T00:00:00Z")
        .expect("an alternative of predicates is plannable");
    // Before the fix every write below refused with CannotVerify.
    knot(&mut store, "ex:a ex:unrelated \"one\" .").expect("unrelated write, new graph");
    knot(&mut store, "ex:b ex:unrelated \"two\" .").expect("unrelated write, incremental");
    knot(&mut store, "ex:l a ex:Labelled ; ex:label \"only one\" .").expect("conforming write");
}

#[test]
fn an_alternative_path_cardinality_shape_is_still_enforced() {
    let mut store = Store::open_in_memory().unwrap();
    store
        .load_shapes("alt", ALTERNATIVE, "2026-11-04T00:00:00Z")
        .unwrap();
    knot(&mut store, "ex:seed ex:unrelated \"x\" .").unwrap();
    let out = knot(
        &mut store,
        "ex:l a ex:Labelled ; ex:label \"first\" ; ex:altLabel \"second\" .",
    )
    .expect("a violation is reported, not a refusal to verify");
    assert_eq!(out["conforms"], false, "{out}");
    let issue = &out["issues"][0];
    assert_eq!(
        issue["component"], "http://www.w3.org/ns/shacl#MaxCountConstraintComponent",
        "{out}"
    );
    assert_eq!(issue["focus_node"], "http://ex.test/l", "{out}");
    // Refused writes commit nothing.
    let rows = crate::sparql::query(
        &store,
        "SELECT (COUNT(*) AS ?n) WHERE { <http://ex.test/l> ?p ?o }",
    )
    .unwrap();
    assert_eq!(
        rows.rows()[0].get("n"),
        Some(&crate::Value::Int(0)),
        "the violating facts were not committed"
    );
}

#[test]
fn an_unplannable_path_is_refused_at_load_and_writes_keep_working() {
    let mut store = Store::open_in_memory().unwrap();
    let err = store
        .load_shapes("seq", SEQUENCE, "2026-11-04T00:00:00Z")
        .expect_err("a sequence path cannot be checked incrementally");
    let message = err.to_string();
    assert!(message.contains("shapes refused"), "{message}");
    assert!(message.contains("every write"), "{message}");
    // Refused, so it never reached the store: writes are unaffected.
    knot(&mut store, "ex:a ex:unrelated \"one\" .").expect("write after a refused load");
    knot(&mut store, "ex:b ex:unrelated \"two\" .").expect("second write after a refused load");
}
