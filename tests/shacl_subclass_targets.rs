//! Data-graph subclass targets and pre-commit rejection without inferred types.
#![cfg(feature = "shacl")]

use quipu::{Validator, store::Store, tool_knot};
use serde_json::json;

const PREFIX: &str = "@prefix ex: <https://example.test/> .\n@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n";
const SHAPES: &str = r#"
@prefix ex: <https://example.test/> .
@prefix sh: <http://www.w3.org/ns/shacl#> .
ex:ParentShape a sh:NodeShape ; sh:targetClass ex:Parent ;
    sh:property [ sh:path ex:required ; sh:minCount 1 ] .
ex:ChildShape a sh:NodeShape ; sh:targetClass ex:Child .
"#;

#[test]
fn target_class_traverses_only_data_subclasses_without_materializing_types() {
    let validator = Validator::from_turtle(SHAPES).unwrap();
    for hierarchy in [
        "ex:Child rdfs:subClassOf ex:Parent .",
        "ex:Child rdfs:subClassOf ex:Middle . ex:Middle rdfs:subClassOf ex:Parent .",
        "ex:Child rdfs:subClassOf ex:Middle . ex:Middle rdfs:subClassOf ex:Child, ex:Parent .",
        "ex:Child rdfs:subClassOf _:middle . _:middle rdfs:subClassOf ex:Parent .",
        "ex:Child rdfs:subClassOf ex:Left, ex:Right . ex:Left rdfs:subClassOf ex:Parent . ex:Right rdfs:subClassOf ex:Parent .",
    ] {
        let data = format!("{PREFIX}{hierarchy} ex:item a ex:Child .");
        let feedback = validator.validate(data.as_bytes()).unwrap();
        assert!(
            !feedback.conforms,
            "missing inherited requirement passed: {hierarchy}"
        );
        assert_eq!(
            feedback.violations, 1,
            "duplicate focus nodes: {feedback:?}"
        );
        let valid = format!("{data} ex:item ex:required \"ok\" .");
        assert!(validator.validate(valid.as_bytes()).unwrap().conforms);
    }
    // Reusing the compiled shapes must not retain a previous data hierarchy.
    assert!(
        validator
            .validate(format!("{PREFIX}ex:item a ex:Child .").as_bytes())
            .unwrap()
            .conforms
    );
    assert!(
        !validator
            .validate(format!("{PREFIX}ex:item a ex:Parent .").as_bytes())
            .unwrap()
            .conforms
    );
    assert!(
        validator
            .validate(
                format!("{PREFIX}ex:Other rdfs:subClassOf ex:Parent . ex:item a ex:Child .")
                    .as_bytes()
            )
            .unwrap()
            .conforms
    );

    // Target selection must not manufacture rdf:type values that satisfy hasValue.
    let explicit = format!(
        r#"{SHAPES}
ex:ChildShape <http://www.w3.org/ns/shacl#property> [
  <http://www.w3.org/ns/shacl#path> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> ;
  <http://www.w3.org/ns/shacl#hasValue> ex:Parent ] ."#
    );
    let data = format!(
        "{PREFIX}ex:Child rdfs:subClassOf ex:Parent . ex:item a ex:Child ; ex:required \"ok\" ."
    );
    assert!(
        !Validator::from_turtle(&explicit)
            .unwrap()
            .validate(data.as_bytes())
            .unwrap()
            .conforms
    );
}

#[test]
fn knot_rejects_inherited_violation_before_committing_or_owl_materialization() {
    let mut store = Store::open_in_memory().unwrap();
    store
        .load_shapes("fixture", SHAPES, "2026-01-01T00:00:00Z")
        .unwrap();
    let data = format!("{PREFIX}ex:Child rdfs:subClassOf ex:Parent . ex:item a ex:Child .");
    let before = store.current_facts().unwrap().len();
    let rejected = tool_knot(&mut store, &json!({"turtle": data})).unwrap();
    assert_eq!(rejected["conforms"], false, "{rejected}");
    assert_eq!(
        store.current_facts().unwrap().len(),
        before,
        "rejected data committed"
    );
    let accepted = tool_knot(
        &mut store,
        &json!({"turtle": format!("{data} ex:item ex:required \"ok\" .")}),
    )
    .unwrap();
    assert!(accepted["count"].as_u64().unwrap() > 0, "{accepted}");
    assert!(store.current_facts().unwrap().len() > before);
}
