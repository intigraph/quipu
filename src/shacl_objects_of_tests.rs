//! aegis-g51c C1: a `sh:targetObjectsOf` shape makes the OBJECT of the edge a
//! focus node. An edge-only write to a stored, valid object must be judged
//! with that object's stored required properties, not from the payload alone.

const SHAPES: &str = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix aegis: <http://aegis.gastown.local/ontology/> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
aegis:InDocumentObjectShape a sh:NodeShape ;
    sh:targetObjectsOf aegis:inDocument ;
    sh:property [ sh:path aegis:filePath ; sh:minCount 1 ] ;
    sh:property [ sh:path rdfs:label ; sh:minCount 1 ] .
"#;
const PREFIX: &str = "@prefix aegis: <http://aegis.gastown.local/ontology/> . \
                      @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> . ";

fn knot(store: &mut crate::Store, ttl: &str) -> crate::Result<serde_json::Value> {
    crate::tool_knot(
        store,
        &serde_json::json!({"turtle": format!("{PREFIX}{ttl}"), "timestamp": "2026-10-22T00:00:00Z"}),
    )
}

fn refused(result: &crate::Result<serde_json::Value>) -> bool {
    matches!(result, Ok(r) if r["conforms"] == serde_json::json!(false))
        || matches!(result, Err(crate::Error::ValidationFailed { .. }))
}

/// Store with the shape loaded; `seed` is written BEFORE validation is on.
fn store_with(seed: &str) -> crate::Store {
    let mut store = crate::Store::open_in_memory().unwrap();
    store
        .load_shapes("objects-of", SHAPES, "2026-10-22T00:00:00Z")
        .unwrap();
    knot(&mut store, seed).expect("seed write");
    store.shacl_config_mut().validate_on_write = true;
    store
}

#[test]
fn edge_only_link_to_a_stored_valid_object_commits() {
    let mut store = store_with(r#"aegis:doc1 aegis:filePath "docs/a.md" ; rdfs:label "a.md" ."#);
    let result = knot(&mut store, "aegis:sec1 aegis:inDocument aegis:doc1 .");
    assert!(
        !refused(&result),
        "the stored object satisfies the shape: {result:?}"
    );
}

#[test]
fn edge_only_link_to_a_stored_invalid_object_is_still_refused() {
    let mut store = store_with(r#"aegis:doc2 aegis:filePath "docs/b.md" ."#);
    let result = knot(&mut store, "aegis:sec2 aegis:inDocument aegis:doc2 .");
    assert!(
        refused(&result),
        "the stored object lacks rdfs:label: {result:?}"
    );
}

#[test]
fn edge_only_link_to_an_unknown_object_is_refused() {
    let mut store = store_with(r#"aegis:other rdfs:label "x" ."#);
    let result = knot(&mut store, "aegis:sec3 aegis:inDocument aegis:nowhere .");
    assert!(
        refused(&result),
        "nothing stored describes the object: {result:?}"
    );
}

#[test]
fn object_target_predicates_reads_only_targets_objects_of() {
    let got = super::object_target_predicates(SHAPES).unwrap();
    assert_eq!(
        got.into_iter().collect::<Vec<_>>(),
        ["http://aegis.gastown.local/ontology/inDocument"]
    );
}
