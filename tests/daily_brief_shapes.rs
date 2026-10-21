#![cfg(feature = "shacl")]

const SHAPES: &str = include_str!("../shapes/aegis-ontology.shapes.ttl");
const PREFIXES: &str = r#"
@prefix a: <http://aegis.gastown.local/ontology/> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
"#;
const FIELDS: &[(&str, &str)] = &[
    ("rdfs:label", r#""Example priority""#),
    (
        "a:reference",
        r#""https://example.invalid/brief#priorities/one""#,
    ),
    ("a:briefArtifact", "<https://example.invalid/brief>"),
    ("a:briefCollection", r#""priorities""#),
    ("a:briefDocumentId", r#""one""#),
    (
        "a:briefPayloadJson",
        r#""{\"title\":\"Curated\",\"rank\":1}""#,
    ),
    ("a:briefRevision", r#""1"^^xsd:integer"#),
    ("a:briefCurator", r#""sattler""#),
    ("a:briefSourceVersion", r#""4"^^xsd:integer"#),
];

fn fixture(omit: Option<&str>, replacement: Option<(&str, &str)>) -> String {
    let mut data = format!("{PREFIXES}\na:example a a:DailyBriefRow .\n");
    for (predicate, value) in FIELDS {
        if omit == Some(*predicate) {
            continue;
        }
        let value = replacement
            .filter(|(p, _)| p == predicate)
            .map_or(*value, |(_, v)| v);
        data.push_str(&format!("a:example {predicate} {value} .\n"));
    }
    data
}

#[test]
fn complete_canonical_row_conforms() {
    let result = quipu::validate_shapes(SHAPES, &fixture(None, None)).unwrap();
    assert!(result.conforms, "{:?}", result.results);
}

#[test]
fn each_required_field_is_enforced() {
    for (field, _) in FIELDS {
        let result = quipu::validate_shapes(SHAPES, &fixture(Some(field), None)).unwrap();
        assert!(!result.conforms, "missing {field} accepted");
        assert!(
            result
                .results
                .iter()
                .any(|r| r.focus_node.ends_with("example"))
        );
    }
}

#[test]
fn wrong_kind_bounds_and_duplicate_values_refuse() {
    for (field, value) in [
        ("a:briefArtifact", r#""literal instead of IRI""#),
        ("a:briefCollection", r#""""#),
        ("a:briefDocumentId", r#""""#),
        ("a:briefRevision", r#""0"^^xsd:integer"#),
        ("a:briefSourceVersion", r#""-1"^^xsd:integer"#),
        ("a:briefSourceVersion", r#""4""#),
        ("a:briefPayloadJson", r#""x""#),
        ("a:briefCurator", r#""""#),
        ("a:briefCollection", r#""asks", "priorities""#),
    ] {
        let result = quipu::validate_shapes(SHAPES, &fixture(None, Some((field, value)))).unwrap();
        assert!(!result.conforms, "bad {field} accepted: {value}");
    }
    for (field, limit) in [
        ("a:briefCollection", 64),
        ("a:briefDocumentId", 256),
        ("a:briefPayloadJson", 16384),
        ("a:briefCurator", 128),
    ] {
        let long = format!("\"{}\"", "x".repeat(limit + 1));
        assert!(
            !quipu::validate_shapes(SHAPES, &fixture(None, Some((field, &long))))
                .unwrap()
                .conforms
        );
    }
}

#[test]
fn before_after_missing_source_version() {
    let base = SHAPES
        .split_once("# Canonical curated Daily Brief rows (aegis-hqz, schema proposal 32).")
        .expect("the appended shape has an exact before/after boundary")
        .0;
    let missing = fixture(Some("a:briefSourceVersion"), None);
    let before = quipu::validate_shapes(base, &missing).unwrap();
    let after = quipu::validate_shapes(SHAPES, &missing).unwrap();
    let positive = quipu::validate_shapes(SHAPES, &fixture(None, None)).unwrap();
    println!(
        "source fixture: missing source version BEFORE conforms={}, AFTER conforms={}; complete positive control conforms={}",
        before.conforms, after.conforms, positive.conforms
    );
    assert!(before.conforms, "base has no DailyBriefRow contract");
    assert!(!after.conforms, "new shape must reject the incomplete row");
    assert!(positive.conforms);
}
