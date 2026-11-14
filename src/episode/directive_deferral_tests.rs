//! rdj: warnings are advisory; traceability requirements remain blocking.
use super::*;

fn episode() -> Episode {
    serde_json::from_value(serde_json::json!({
        "name": "directive-admission", "nodes": [
            {"name": "retained-decision", "type": "Directive"}
        ], "edges": []
    }))
    .unwrap()
}

#[test]
fn warning_only_episode_is_admitted_but_violation_is_refused() {
    for (severity, accepted) in [("Warning", true), ("Info", true), ("Violation", false)] {
        let mut store = Store::open_in_memory().unwrap();
        let shape = format!(
            r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix a: <http://aegis.gastown.local/ontology/> .
a:TraceShape a sh:NodeShape ; sh:targetClass a:Directive ;
 sh:severity sh:{severity} ;
 sh:property [ sh:path a:missing ; sh:minCount 1 ] .
"#
        );
        store
            .load_shapes("traceability", &shape, "2026-11-14")
            .unwrap();
        let ep = episode();
        let result = ingest_episode(
            &mut store,
            &ep,
            "2026-11-14T08:00:00Z",
            namespace::DEFAULT_BASE_NS,
        );
        assert_eq!(result.is_ok(), accepted, "{severity}: {result:?}");
        if !accepted {
            assert!(matches!(
                result,
                Err(Error::ValidationFailed { violations: 1, .. })
            ));
        }
    }
}

#[test]
fn published_directive_shape_accepts_typed_deadline_and_rejects_neither() {
    let shapes = include_str!("../../shapes/aegis-ontology.shapes.ttl");
    let prefix = r#"
@prefix a: <http://aegis.gastown.local/ontology/> .
@prefix q: <https://w3id.org/intigraph/quechua/ns#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
a:decision a a:Directive ; rdfs:label "Keep this decision" "#;
    for (suffix, expected) in [
        (
            "; q:traceabilityDueAt \"2030-11-28T08:00:00Z\"^^xsd:dateTime .",
            true,
        ),
        (
            "; q:traceabilityDueAt \"2020-11-28T08:00:00Z\"^^xsd:dateTime .",
            true,
        ),
        (".", false),
        ("; q:traceabilityDueAt \"later\" .", false),
        (
            "; q:traceabilityDueAt \"2030-11-28T08:00:00Z\"^^xsd:dateTime, \"2031-11-28T08:00:00Z\"^^xsd:dateTime .",
            false,
        ),
    ] {
        let feedback = crate::shacl::validate_shapes(shapes, &format!("{prefix}{suffix}")).unwrap();
        assert_eq!(!feedback.blocks(), expected, "{suffix}: {feedback:?}");
        if !expected {
            assert!(feedback.violations > 0);
        }
    }
}

#[test]
fn three_directive_episode_accepts_explicit_typed_dates_and_refuses_neither() {
    for with_date in [true, false] {
        let mut store = Store::open_in_memory().unwrap();
        store
            .load_shapes(
                "aegis-ontology",
                include_str!("../../shapes/aegis-ontology.shapes.ttl"),
                "2026-11-14",
            )
            .unwrap();
        let nodes: Vec<_> = (0..3).map(|i| {
            let mut node = serde_json::json!({"name": format!("decision-{i}"), "type": "Directive"});
            if with_date {
                node["properties"] = serde_json::json!({
                    "https://w3id.org/intigraph/quechua/ns#traceabilityDueAt": {
                        "value": "2030-11-28T08:00:00Z", "datatype": "http://www.w3.org/2001/XMLSchema#dateTime"
                    }
                });
            }
            node
        }).collect();
        let episode: Episode = serde_json::from_value(
            serde_json::json!({"name": "three-decisions", "nodes": nodes, "edges": []}),
        )
        .unwrap();
        let result = ingest_episode(
            &mut store,
            &episode,
            "2026-11-14T08:00:00Z",
            namespace::DEFAULT_BASE_NS,
        );
        assert_eq!(result.is_ok(), with_date, "{result:?}");
        if !with_date {
            match result.unwrap_err() {
                Error::ValidationFailed {
                    violations,
                    messages,
                } => {
                    assert!(violations >= 3);
                    assert!(
                        messages
                            .iter()
                            .any(|m| m.contains("OR one traceabilityDueAt"))
                    );
                }
                other => panic!("expected directive refusal, got {other}"),
            }
        }
    }
}
