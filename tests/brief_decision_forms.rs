#![cfg(feature = "shacl")]

use quipu::{store::Store, tool_shapes, validate_shapes};
use serde_json::json;

const SHAPES: &str = include_str!("../shapes/brief-decision-forms.ttl");
const PREFIX: &str = r#"
@prefix a: <http://aegis.gastown.local/ontology/> .
@prefix schema: <https://schema.org/> .
"#;

fn form(kind: &str) -> String {
    format!(
        r#"{PREFIX}
<urn:example:form> a a:DecisionForm ; a:formKind "{kind}" ;
 schema:name "A question" ; schema:description "Choose explicitly" ;
 schema:version 1 ; schema:subjectOf <urn:example:source> .
"#
    )
}

#[test]
fn all_seven_form_kinds_are_governed_without_becoming_signed_decisions() {
    for kind in [
        "approve-reject",
        "choose-one",
        "ranked-choice",
        "free-text",
        "date-window",
        "step-confirm",
        "acknowledge",
    ] {
        assert!(validate_shapes(SHAPES, &form(kind)).unwrap().conforms);
    }
    let store = Store::open_in_memory().unwrap();
    store
        .load_shapes("brief-decision-forms", SHAPES, "2026-01-01T00:00:00Z")
        .unwrap();
    let vocabulary = tool_shapes(&store, &json!({"action": "vocabulary"})).unwrap();
    let classes = vocabulary["classes"].as_array().unwrap();
    assert_eq!(classes.len(), 2);
    for name in ["DecisionForm", "DecisionFormOption"] {
        assert!(classes.iter().any(|c| {
            c.as_str() == Some(format!("http://aegis.gastown.local/ontology/{name}").as_str())
        }));
    }
}

#[test]
fn missing_or_unknown_kind_revision_and_source_are_refused() {
    let valid = form("free-text");
    for invalid in [
        valid.replace("\"free-text\"", "\"unknown-kind\""),
        valid.replace("schema:version 1", "schema:version 0"),
        valid.replace("a:formKind \"free-text\" ;", ""),
        valid.replace(
            "schema:subjectOf <urn:example:source>",
            "schema:name \"duplicate\"",
        ),
    ] {
        assert!(!validate_shapes(SHAPES, &invalid).unwrap().conforms);
    }
}

#[test]
fn options_require_bounded_order_consequences_and_evidence() {
    let data = format!(
        r#"{}
<urn:example:option> a a:DecisionFormOption ; a:optionValue "hold" ;
 a:optionOrder 0 ; schema:name "Hold" ; schema:description "Work remains paused" ;
 schema:subjectOf <urn:example:evidence> .
"#,
        form("choose-one")
    );
    assert!(validate_shapes(SHAPES, &data).unwrap().conforms);
    for invalid in [
        data.replace("a:optionOrder 0", "a:optionOrder 12"),
        data.replace("schema:description \"Work remains paused\" ;", ""),
        data.replace(
            "schema:subjectOf <urn:example:evidence>",
            "schema:name \"duplicate\"",
        ),
    ] {
        assert!(!validate_shapes(SHAPES, &invalid).unwrap().conforms);
    }
}
