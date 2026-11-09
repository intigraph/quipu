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

const ALTERNATIVE_WITH_INVERSE: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <http://ex.test/> .
ex:LabelledShape a sh:NodeShape ; sh:targetClass ex:Labelled ;
    sh:property [ sh:path [ sh:alternativePath ( ex:p [ sh:inversePath ex:q ] ) ] ;
                  sh:maxCount 1 ] .";

#[test]
fn an_alternative_with_an_inverse_member_counts_existing_incoming_values() {
    let mut store = Store::open_in_memory().unwrap();
    store
        .load_shapes(
            "alt-inverse",
            ALTERNATIVE_WITH_INVERSE,
            "2026-11-04T00:00:00Z",
        )
        .unwrap();
    // The inverse edge is already stored, not present in the candidate delta.
    knot(&mut store, "ex:incoming ex:q ex:l .").unwrap();
    let err = knot(&mut store, "ex:l a ex:Labelled ; ex:p ex:outgoing .")
        .expect_err("the incoming and outgoing values violate maxCount 1");
    match err {
        crate::Error::ValidationFailed {
            violations,
            messages,
        } => {
            assert!(violations > 0);
            assert!(
                messages.iter().any(|message| {
                    message.contains("http://www.w3.org/ns/shacl#MaxCountConstraintComponent")
                        && message.contains("http://ex.test/l")
                }),
                "{messages:?}"
            );
        }
        other => panic!("expected a cardinality violation, got {other}"),
    }
    let rows = crate::sparql::query(
        &store,
        "SELECT (COUNT(*) AS ?n) WHERE { <http://ex.test/l> ?p ?o }",
    )
    .unwrap();
    assert_eq!(rows.rows()[0].get("n"), Some(&crate::Value::Int(0)));
    let rows = crate::sparql::query(
        &store,
        "SELECT (COUNT(*) AS ?n) WHERE { <http://ex.test/incoming> <http://ex.test/q> <http://ex.test/l> }",
    )
    .unwrap();
    assert_eq!(rows.rows()[0].get("n"), Some(&crate::Value::Int(1)));
}

#[test]
fn an_alternative_with_an_inverse_member_unions_equal_values_once() {
    let mut store = Store::open_in_memory().unwrap();
    store
        .load_shapes(
            "alt-inverse",
            ALTERNATIVE_WITH_INVERSE,
            "2026-11-04T00:00:00Z",
        )
        .unwrap();
    knot(&mut store, "ex:only ex:q ex:l .").unwrap();
    // Both arms reach the same RDF term: maxCount 1 must still conform.
    let out = knot(&mut store, "ex:l a ex:Labelled ; ex:p ex:only .").unwrap();
    assert_eq!(out["conforms"], true, "{out}");
    let rows = crate::sparql::query(
        &store,
        "SELECT (COUNT(*) AS ?n) WHERE { <http://ex.test/l> <http://ex.test/p> <http://ex.test/only> }",
    )
    .unwrap();
    assert_eq!(rows.rows()[0].get("n"), Some(&crate::Value::Int(1)));
}

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

// kaj: payload conformance cannot certify a path completed by stored facts.
const KAJ_COMPLEX: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <http://ex.test/> .
ex:s a sh:PropertyShape ; sh:targetNode ex:i ;
    sh:path ( [ sh:inversePath ex:p ] [ sh:inversePath ex:p ] ) ; sh:class ex:C .";

#[test]
fn kaj_complex_path_incremental_violation_rolls_back() {
    let mut store = Store::open_in_memory().unwrap();
    store
        .load_shapes("kaj", KAJ_COMPLEX, "2026-11-09T00:00:00Z")
        .unwrap();
    knot(&mut store, "ex:j ex:p ex:i .").unwrap();
    let error = knot(&mut store, "ex:k ex:p ex:j .")
        .expect_err("a payload PASS cannot certify an affected complex path");
    assert!(
        matches!(error, crate::Error::ValidationFailed { .. }),
        "{error}"
    );
    let rows =
        crate::sparql::query(&store, "SELECT ?s WHERE { ?s <http://ex.test/p> ?o }").unwrap();
    assert_eq!(rows.rows().len(), 1, "invalid write must roll back");
}

#[test]
fn kaj_nested_property_incremental_violation_rolls_back() {
    let mut store = Store::open_in_memory().unwrap();
    let shapes = "@prefix sh: <http://www.w3.org/ns/shacl#> .
        @prefix ex: <http://ex.test/> .
        ex:s a sh:NodeShape ; sh:targetNode ex:i ;
            sh:property [ sh:path ex:p ; sh:property [ sh:path ex:r ; sh:class ex:C ] ] .";
    store
        .load_shapes("kaj", shapes, "2026-11-09T00:00:00Z")
        .unwrap();
    knot(&mut store, "ex:i ex:p ex:j .").unwrap();
    let error = knot(&mut store, "ex:j ex:r ex:k .")
        .expect_err("a changed child requires its stored parent's path context");
    assert!(
        matches!(error, crate::Error::ValidationFailed { .. }),
        "{error}"
    );
    let rows =
        crate::sparql::query(&store, "SELECT ?s WHERE { ?s <http://ex.test/r> ?o }").unwrap();
    assert!(rows.rows().is_empty());
}

#[test]
fn kaj_late_subclass_target_is_not_silently_accepted() {
    let mut store = Store::open_in_memory().unwrap();
    let shapes = "@prefix sh: <http://www.w3.org/ns/shacl#> .
        @prefix ex: <http://ex.test/> .
        ex:s a sh:NodeShape ; sh:targetClass ex:Super ; sh:in (ex:valid) .
        ex:SubShape a sh:NodeShape ; sh:targetClass ex:Sub .";
    store
        .load_shapes("kaj", shapes, "2026-11-09T00:00:00Z")
        .unwrap();
    knot(&mut store, "ex:invalid a ex:Sub .").unwrap();
    let error = knot(
        &mut store,
        "ex:Sub <http://www.w3.org/2000/01/rdf-schema#subClassOf> ex:Super .",
    )
    .expect_err("a new target dependency must not bypass stored instance validation");
    assert!(
        matches!(error, crate::Error::ValidationFailed { .. }),
        "{error}"
    );
}

#[test]
fn kaj_valid_whole_document_and_unrelated_writes_still_work() {
    let mut store = Store::open_in_memory().unwrap();
    store
        .load_shapes("kaj", KAJ_COMPLEX, "2026-11-09T00:00:00Z")
        .unwrap();
    let out = knot(&mut store, "ex:j ex:p ex:i . ex:k ex:p ex:j ; a ex:C .").unwrap();
    assert_eq!(out["conforms"], true);
    let out = knot(&mut store, "ex:unrelated ex:label \"ok\" .").unwrap();
    assert_eq!(out["conforms"], true);
}

#[test]
fn kaj_affected_path_budget_is_unknown_not_a_silent_pass() {
    let mut store = Store::open_in_memory().unwrap();
    store
        .load_shapes("kaj", KAJ_COMPLEX, "2026-11-09T00:00:00Z")
        .unwrap();
    // Whole-graph validation can prove this fresh graph. The later affected
    // component exceeds the incremental bound, so partial validation cannot pass.
    let mut ttl = String::from("ex:n0 ex:p ex:i . ex:n1 a ex:C .\n");
    for n in 1..1025 {
        ttl.push_str(&format!("ex:n{n} ex:p ex:n{} .\n", n - 1));
    }
    assert_eq!(knot(&mut store, &ttl).unwrap()["conforms"], true);
    assert_eq!(
        knot(&mut store, "ex:other ex:label \"ok\" .").unwrap()["conforms"],
        true
    );
    let error = knot(&mut store, "ex:extra ex:p ex:n1024 .").expect_err("bound must refuse");
    assert!(matches!(error, crate::Error::CannotVerify(_)), "{error}");
    let rows = crate::sparql::query(
        &store,
        "SELECT ?o WHERE { <http://ex.test/extra> <http://ex.test/p> ?o }",
    )
    .unwrap();
    assert!(rows.rows().is_empty(), "unknown write must roll back");
}

#[test]
fn kaj_late_subclass_target_accepts_valid_stored_instances() {
    let mut store = Store::open_in_memory().unwrap();
    let shapes = "@prefix sh: <http://www.w3.org/ns/shacl#> .
        @prefix ex: <http://ex.test/> .
        ex:s a sh:NodeShape ; sh:targetClass ex:Super ; sh:in (ex:valid) .
        ex:SubShape a sh:NodeShape ; sh:targetClass ex:Sub .";
    store
        .load_shapes("kaj", shapes, "2026-11-09T00:00:00Z")
        .unwrap();
    assert_eq!(
        knot(&mut store, "ex:valid a ex:Sub .").unwrap()["conforms"],
        true
    );
    assert_eq!(
        knot(
            &mut store,
            "ex:Sub <http://www.w3.org/2000/01/rdf-schema#subClassOf> ex:Super ."
        )
        .unwrap()["conforms"],
        true
    );
}

#[test]
fn kaj_discovered_parent_uses_only_selected_reference_types() {
    for destination_is_root in [false, true] {
        for include_types in [true, false] {
            let mut store = Store::open_in_memory().unwrap();
            let shapes = "@prefix sh: <http://www.w3.org/ns/shacl#> .
                @prefix ex: <http://ex.test/> .
                ex:s a sh:NodeShape ; sh:targetClass ex:Parent ;
                    sh:property [ sh:path ex:p ; sh:property [ sh:path ex:r ; sh:class ex:C ] ] .";
            store
                .load_shapes("kaj", shapes, "2026-11-09T00:00:00Z")
                .unwrap();
            let plane = store.graph_create("http://ex.test/plane").unwrap();
            let (g, type_graph) = if destination_is_root {
                (0, plane)
            } else {
                (plane, 0)
            };
            let graph_write = |store: &mut Store, graph: Option<&str>, ttl: &str| {
                crate::tool_knot(
                    store,
                    &serde_json::json!({"graph":graph,
                    "turtle":format!("@prefix ex: <http://ex.test/> . {ttl}"),
                    "timestamp":"2026-11-09T00:00:00Z"}),
                )
            };
            let destination = (!destination_is_root).then_some("http://ex.test/plane");
            let types = destination_is_root.then_some("http://ex.test/plane");
            graph_write(&mut store, types, "ex:i a ex:Parent .").unwrap();
            graph_write(&mut store, destination, "ex:i ex:p ex:j .").unwrap();
            let graphs = if include_types {
                vec![g, type_graph]
            } else {
                vec![g]
            };
            let result = store.with_write_reference_graphs(g, &graphs, |s| {
                graph_write(s, destination, "ex:j ex:r ex:k .")
            });
            if include_types {
                assert!(
                    matches!(result, Err(crate::Error::ValidationFailed { .. })),
                    "selected typed parent must be checked: {result:?}"
                );
            } else {
                assert_eq!(
                    result.unwrap()["conforms"],
                    true,
                    "excluded types must not create a target"
                );
            }
        }
    }
}
