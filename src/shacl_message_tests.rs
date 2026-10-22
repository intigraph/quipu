//! Message attribution follows the exact compiled source shape, not document text.
use super::*;

const PREFIX: &str = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <https://example.org/message/> .
"#;
const DATA: &str = "<https://example.org/message/o> a <https://example.org/message/Observation> .";

#[test]
fn unrelated_shape_message_is_never_borrowed() {
    for property in ["ex:About", "[ sh:path ex:about ; sh:minCount 1 ]"] {
        let shapes = format!(
            r#"{PREFIX}
ex:Directive a sh:NodeShape ; sh:targetClass ex:DirectiveClass ;
    sh:hasValue ex:required ; sh:message "unrelated directive" .
ex:ObservationShape a sh:NodeShape ; sh:targetClass ex:Observation ; sh:property {property} .
ex:About a sh:PropertyShape ; sh:path ex:about ; sh:minCount 1 .
"#
        );
        let validator = Validator::from_turtle(&shapes).unwrap();
        for _ in 0..2 {
            let feedback = validator.validate(DATA.as_bytes()).unwrap();
            assert_eq!(feedback.violations, 1);
            let issue = &feedback.results[0];
            assert_eq!(
                issue.path.as_deref(),
                Some("https://example.org/message/about")
            );
            assert!(issue.message.as_deref().unwrap().contains("MinCount"));
            assert!(!issue.message.as_deref().unwrap().contains("unrelated"));
            assert!(issue.source_shape.is_some());
        }
        let valid = format!(
            "{DATA}\n<https://example.org/message/o> <https://example.org/message/about> <https://example.org/message/topic> ."
        );
        assert!(validator.validate(valid.as_bytes()).unwrap().conforms);
    }
}

#[test]
fn each_named_property_uses_its_own_message() {
    let shapes = format!(
        r#"{PREFIX}
ex:ObservationShape a sh:NodeShape ; sh:targetClass ex:Observation ;
    sh:message "parent only" ; sh:property ex:About, ex:Source .
ex:About a sh:PropertyShape ; sh:path ex:about ; sh:minCount 1 ; sh:message "about required" .
ex:Source a sh:PropertyShape ; sh:path ex:source ; sh:minCount 1 ; sh:message "source required" .
"#
    );
    let feedback = validate_shapes(&shapes, DATA).unwrap();
    assert_eq!(feedback.violations, 2);
    for issue in &feedback.results {
        let (source, message) = match issue.path.as_deref().unwrap() {
            "https://example.org/message/about" => ("About", "about required"),
            "https://example.org/message/source" => ("Source", "source required"),
            path => panic!("unexpected path {path}"),
        };
        assert_eq!(
            issue.source_shape.as_deref(),
            Some(format!("https://example.org/message/{source}").as_str())
        );
        assert_eq!(issue.message.as_deref(), Some(message));
    }
}

#[test]
fn inline_messages_use_parsed_identity_and_turtle_literals() {
    for message in [
        "sh:message \"escaped \\\"about\\\"\\nrequired\"",
        "<http://www.w3.org/ns/shacl#message> '''escaped \"about\"\nrequired'''",
    ] {
        let shapes = format!(
            r#"{PREFIX}
# sh:message "decoy in a comment"
ex:ObservationShape a sh:NodeShape ; sh:targetClass ex:Observation ;
    sh:property [ sh:path ex:about ; sh:minCount 1 ; {message} ] ;
    sh:property [ sh:path ex:source ; sh:minCount 1 ] .
"#
        );
        let validator = Validator::from_turtle(&shapes).unwrap();
        for _ in 0..2 {
            let feedback = validator.validate(DATA.as_bytes()).unwrap();
            assert_eq!(feedback.violations, 2);
            for issue in &feedback.results {
                if issue.path.as_deref() == Some("https://example.org/message/about") {
                    assert_eq!(
                        issue.message.as_deref(),
                        Some("escaped \"about\"\nrequired")
                    );
                } else {
                    assert_eq!(
                        issue.path.as_deref(),
                        Some("https://example.org/message/source")
                    );
                    assert!(issue.message.as_deref().unwrap().contains("MinCount"));
                }
            }
        }
    }
}

#[test]
fn emitted_event_keeps_its_property_shapes_message() {
    use crate::episode::{Episode, Node, ingest_episode};
    use crate::namespace::DEFAULT_BASE_NS;
    let shapes = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix a: <http://aegis.gastown.local/ontology/> .
@prefix q: <http://quipu.dev/ns#> .
a:Unrelated a sh:NodeShape ; sh:targetClass a:Directive ;
    sh:hasValue a:required ; sh:message "unrelated directive" ; q:onViolation "emit" .
a:WidgetShape a sh:NodeShape ; sh:targetClass a:Widget ; q:onViolation "emit" ;
    sh:property [ sh:path a:size ; sh:minCount 1 ; sh:message "size required" ] .
"#;
    for custom in [false, true] {
        let shapes = if custom {
            shapes.to_string()
        } else {
            shapes.replace(" ; sh:message \"size required\"", "")
        };
        let mut store = crate::Store::open_in_memory().unwrap();
        store
            .load_shapes("messages", &shapes, "2026-10-22T00:00:00Z")
            .unwrap();
        store.shacl_config_mut().validate_on_write = true;
        let episode = Episode {
            name: "message-control".into(),
            episode_body: None,
            source: Some("test".into()),
            group_id: None,
            edges: vec![],
            graph: None,
            shapes: None,
            replace_snapshot: false,
            nodes: vec![Node {
                name: "widget".into(),
                node_type: Some("Widget".into()),
                description: Some("fixture".into()),
                properties: None,
                distinct_from: vec![],
            }],
        };
        let (tx, _) = ingest_episode(
            &mut store,
            &episode,
            "2026-10-22T00:00:01Z",
            DEFAULT_BASE_NS,
        )
        .unwrap();
        let events: Vec<_> = store
            .events_after(0, 100, None, None)
            .unwrap()
            .into_iter()
            .filter(|event| event.event_type == "shacl.violation")
            .collect();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].tx_id, tx);
        let payload: serde_json::Value = serde_json::from_str(&events[0].payload).unwrap();
        if custom {
            assert_eq!(payload["message"], "size required");
        } else {
            assert!(payload["message"].as_str().unwrap().contains("MinCount"));
            assert!(!payload["message"].as_str().unwrap().contains("unrelated"));
        }
        assert_eq!(payload["path"], format!("{DEFAULT_BASE_NS}size"));
        assert_eq!(payload["mode"], "emit");
        assert!(payload["shape"].as_str().is_some());
    }
}
