//! aegis-g51c condition 7: `split_shapes_by_policy` must route EXACTLY the
//! emit-annotated shapes to emit, on the real loaded shape files.
//!
//! The oracle is an independent RDF parse of the same document: the expected
//! emit set is the subjects carrying the triple `quipu:onViolation "emit"`, the
//! expected reject set is every other `sh:NodeShape`. The splitter's two
//! documents must contain exactly those shapes (count and names).

use std::collections::BTreeSet;

use oxrdfio::{RdfFormat, RdfParser};

use super::split_shapes_by_policy;

const SH_NODE_SHAPE: &str = "http://www.w3.org/ns/shacl#NodeShape";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const ON_VIOLATION: &str = "http://quipu.dev/ontology/onViolation";

/// camayoc shapes/core.shapes.ttl at 731272ea, the ref production loads as
/// `camayoc-core`. Its `sh:pattern` strings contain `#`.
const CAMAYOC_CORE: &str = include_str!("../tests/fixtures/shapes/camayoc-core-731272ea.ttl");
const CAMAYOC_CORE_SHA256: &str =
    "8bbdf741ff746f7bdf59b1eed75a2f7f812d361f2a823a2e5dc438a0732812c2";
/// The condition-2 candidate: camayoc-core 731272ea with `CamayocObservationShape`'s
/// four minCounts moved into the emit-annotated debt shape.
const CAMAYOC_CORE_CANDIDATE: &str =
    include_str!("../tests/fixtures/shapes/camayoc-core-g51c-candidate.ttl");
const AEGIS_ONTOLOGY: &str = include_str!("../shapes/aegis-ontology.shapes.ttl");
const CODE_ENTITIES: &str = include_str!("../shapes/code-entities.ttl");

/// The condition-2 debt shapes: one per set that sorts AFTER camayoc-core's
/// first `#`-in-literal line, which is where the old splitter lost track.
const CAMAYOC_DEBT: &str = r#"
# Write-validation debt (aegis-g51c).
aegis:CamayocObservationProvenanceDebtShape a sh:NodeShape ;
    quipu:onViolation "emit" ;
    sh:severity sh:Warning ;
    sh:targetClass aegis:Observation ;
    sh:property [ sh:path aegis:sourceKind ; sh:minCount 1 ] ;
    sh:property [ sh:path aegis:about ; sh:minCount 1 ] ;
    sh:property [ sh:path aegis:observedAt ; sh:minCount 1 ] ;
    sh:property [ sh:path aegis:observedValue ; sh:minCount 1 ] .
"#;
const CODE_DEBT: &str = r#"
@prefix quipu: <http://quipu.dev/ontology/> .
bobbin:ChunkLabelMaxCountDebtShape a sh:NodeShape ;
    quipu:onViolation "emit" ;
    sh:severity sh:Warning ;
    sh:targetClass bobbin:Chunk ;
    sh:property [ sh:path rdfs:label ; sh:maxCount 1 ] .
"#;

/// The production combine: loaded sets in name order, joined by a blank line.
fn combined() -> String {
    let camayoc = format!("{CAMAYOC_CORE}\n{CAMAYOC_DEBT}");
    let code = format!("{CODE_ENTITIES}\n{CODE_DEBT}");
    [AEGIS_ONTOLOGY, camayoc.as_str(), code.as_str()].join("\n\n")
}

/// `sh:NodeShape` subjects of a Turtle document, and those annotated emit.
fn shapes_of(doc: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let (mut all, mut emit) = (BTreeSet::new(), BTreeSet::new());
    for quad in RdfParser::from_format(RdfFormat::Turtle).for_reader(doc.as_bytes()) {
        let quad = quad.expect("document parses as Turtle");
        let (s, p, o) = (
            quad.subject.to_string(),
            quad.predicate.as_str().to_owned(),
            quad.object.to_string(),
        );
        if p == RDF_TYPE && o == format!("<{SH_NODE_SHAPE}>") {
            all.insert(s.clone());
        }
        if p == ON_VIOLATION && o == "\"emit\"" {
            emit.insert(s);
        }
    }
    (all, emit)
}

fn names(set: &BTreeSet<String>) -> BTreeSet<String> {
    set.iter()
        .map(|s| {
            s.trim_matches(['<', '>'])
                .rsplit('/')
                .next()
                .unwrap()
                .to_owned()
        })
        .collect()
}

#[test]
fn fixture_is_the_production_camayoc_core() {
    use sha2::{Digest, Sha256};
    assert_eq!(
        hex::encode(Sha256::digest(CAMAYOC_CORE.as_bytes())),
        CAMAYOC_CORE_SHA256
    );
}

#[test]
fn real_sets_route_only_the_annotated_shapes_to_emit() {
    let doc = combined();
    let (all, annotated) = shapes_of(&doc);
    let split = split_shapes_by_policy(&doc);
    let (reject_shapes, _) = shapes_of(&split.reject);
    let (emit_shapes, _) = shapes_of(&split.emit);

    assert_eq!(
        emit_shapes, annotated,
        "emit document must hold exactly the annotated shapes"
    );
    let expected_reject: BTreeSet<_> = all.difference(&annotated).cloned().collect();
    assert_eq!(
        reject_shapes, expected_reject,
        "every other shape must stay in REJECT"
    );
    assert_eq!(reject_shapes.len() + emit_shapes.len(), all.len());

    // The debt shapes are the only additions beyond aegis-ontology's own emit set.
    let (_, ontology_emit) = shapes_of(AEGIS_ONTOLOGY);
    let added: BTreeSet<_> = names(&emit_shapes)
        .difference(&names(&ontology_emit))
        .cloned()
        .collect();
    assert_eq!(
        added,
        [
            "CamayocObservationProvenanceDebtShape",
            "ChunkLabelMaxCountDebtShape"
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>()
    );
    assert!(names(&reject_shapes).contains("CamayocObservationShape"));
    assert!(names(&reject_shapes).contains("CamayocBlockerShape"));
}

#[test]
fn hash_inside_a_literal_does_not_end_the_statement() {
    let doc = r#"@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <http://example.org/> .
@prefix quipu: <http://quipu.dev/ontology/> .
ex:Patterned a sh:NodeShape ;
    sh:targetClass ex:Ref ;
    sh:property [ sh:path ex:pr ; sh:pattern "^[a-z]+/[a-z]+#[0-9]+$" ] ;
    sh:property [ sh:path ex:kind ; sh:in ( "a#b" 'c#d' ) ] .
ex:Soft a sh:NodeShape ;
    quipu:onViolation "emit" ;
    sh:targetClass ex:Widget ;
    sh:property [ sh:path ex:size ; sh:minCount 1 ] .
ex:Hard a sh:NodeShape ;
    sh:targetClass ex:Thing ;
    sh:property [ sh:path ex:label ; sh:maxCount 1 ] .
"#;
    let split = split_shapes_by_policy(doc);
    assert_eq!(
        names(&shapes_of(&split.emit).0),
        ["Soft".to_owned()].into_iter().collect()
    );
    assert_eq!(
        names(&shapes_of(&split.reject).0),
        ["Hard", "Patterned"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
}

#[test]
fn an_annotation_in_a_comment_does_not_emit() {
    let doc = r#"@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <http://example.org/> .
ex:Hard a sh:NodeShape ;
    # not this one: quipu:onViolation "emit"
    sh:targetClass ex:Thing ;
    sh:property [ sh:path ex:label ; sh:maxCount 1 ] .
"#;
    let split = split_shapes_by_policy(doc);
    assert!(!split.has_emit);
    assert!(split.reject.contains("ex:Hard"));
}

#[test]
fn unterminated_content_rejects_even_with_emit_text() {
    let doc = r#"@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <http://example.org/> .
@prefix quipu: <http://quipu.dev/ontology/> .
ex:Hard a sh:NodeShape ;
    sh:property [ sh:path ex:label ; sh:maxCount 1 ;
ex:Soft a sh:NodeShape ;
    quipu:onViolation "emit" ;
    sh:targetClass ex:Widget .
"#;
    let split = split_shapes_by_policy(doc);
    assert!(
        !split.has_emit,
        "an unclosed bracket must not let anything route to emit"
    );
    assert!(split.reject.contains("ex:Soft") && split.reject.contains("ex:Hard"));
}

/// End to end on the write path: a non-debt maxCount is REFUSED and a
/// debt-only violation COMMITS with a shacl.violation event.
#[test]
fn write_gate_refuses_non_debt_and_emits_debt() {
    let mut store = crate::Store::open_in_memory().unwrap();
    let code = format!("{CODE_ENTITIES}\n{CODE_DEBT}");
    for (name, ttl) in [
        ("aegis-ontology", AEGIS_ONTOLOGY),
        ("camayoc-core", CAMAYOC_CORE_CANDIDATE),
        ("code-entities", code.as_str()),
    ] {
        store
            .load_shapes(name, ttl, "2026-10-22T00:00:00Z")
            .unwrap();
    }
    store.shacl_config_mut().validate_on_write = true;
    let prefix = "@prefix aegis: <http://aegis.gastown.local/ontology/> . @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> . ";
    let knot = |store: &mut crate::Store, ttl: &str| {
        crate::tool_knot(
            store,
            &serde_json::json!({"turtle": format!("{prefix}{ttl}"), "timestamp": "2026-10-22T00:00:00Z"}),
        )
    };

    let refused = knot(
        &mut store,
        r#"aegis:obs2 a aegis:Observation ; rdfs:label "a", "b" ; aegis:sourceKind "observed" ; aegis:about aegis:x ; aegis:observedAt "t" ; aegis:observedValue "v" ."#,
    );
    let refused_ok = matches!(&refused, Ok(r) if r["conforms"] == serde_json::json!(false))
        || matches!(refused, Err(crate::Error::ValidationFailed { .. }));
    assert!(
        refused_ok,
        "a non-debt maxCount on Observation must be refused"
    );

    let admitted = knot(&mut store,
        r#"aegis:obs1 a aegis:Observation ; rdfs:label "o1" ; aegis:sourceKind "observed" ; aegis:observedAt "t" ; aegis:observedValue "v" ."#)
        .expect("a debt-only violation must commit");
    assert_ne!(admitted["conforms"], serde_json::json!(false), "{admitted}");
    // The event names the shape by its blank-node id, so match mode + path.
    let events = store.events_after(0, 10_000, None, None).unwrap();
    assert!(
        events.iter().any(|e| e.event_type == "shacl.violation"
            && e.payload.contains(r#""mode":"emit""#)
            && e.payload
                .contains(r#""path":"http://aegis.gastown.local/ontology/about""#)),
        "the debt violation must be emitted as an event: {:?}",
        events
            .iter()
            .map(|e| (&e.event_type, &e.payload))
            .collect::<Vec<_>>()
    );
}

/// Review of #726: the annotation's TEXT inside any literal form (a
/// `sh:message`, say) must not route a valid unannotated shape to emit.
#[test]
fn annotation_text_inside_a_literal_does_not_emit() {
    for message in [
        r#"'''see quipu:onViolation "emit" for advisory shapes'''"#,
        r#""""see quipu:onViolation "emit" for advisory shapes""""#,
        r#""see quipu:onViolation \"emit\" for advisory shapes""#,
        r#"'see quipu:onViolation "emit" for advisory shapes'"#,
    ] {
        let doc = format!(
            "@prefix sh: <http://www.w3.org/ns/shacl#> .\n\
             @prefix ex: <http://example.org/> .\n\
             @prefix quipu: <http://quipu.dev/ontology/> .\n\
             ex:Hard a sh:NodeShape ;\n    sh:targetClass ex:Thing ;\n    \
             sh:message {message} ;\n    \
             sh:property [ sh:path ex:label ; sh:maxCount 1 ] .\n"
        );
        let (all, annotated) = shapes_of(&doc);
        assert!(annotated.is_empty() && all.len() == 1, "oracle: {message}");
        let split = split_shapes_by_policy(&doc);
        assert!(!split.has_emit, "routed to emit by a literal: {message}");
        assert_eq!(
            names(&shapes_of(&split.reject).0),
            ["Hard".to_owned()].into_iter().collect()
        );
    }
}

/// wu's ruling on #726: emit is decided from PARSED RDF, the triple
/// `quipu:onViolation "emit"` on the block's top-level subject. Each case is
/// one where reading the text, masked or not, gives the other answer.
#[test]
fn emit_membership_is_read_from_parsed_rdf() {
    const HEAD: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .\n\
                        @prefix ex: <http://example.org/> .\n\
                        @prefix quipu: <http://quipu.dev/ontology/> .\n";
    let cases = [
        // ian's case: the annotation quoted in a long single-quoted literal.
        (
            "quoted in sh:message",
            "ex:S a sh:NodeShape ;\n    sh:message '''quipu:onViolation \"emit\"''' ;\n    sh:property [ sh:path ex:label ; sh:maxCount 1 ] .\n",
            false,
        ),
        // Only the top-level shape's policy routes the block; a nested
        // property shape carrying the annotation does not soften its parent.
        (
            "nested property shape",
            "ex:S a sh:NodeShape ;\n    sh:property [ sh:path ex:label ; sh:maxCount 1 ; quipu:onViolation \"emit\" ] .\n",
            false,
        ),
        (
            "full IRI",
            "ex:S a sh:NodeShape ;\n    <http://quipu.dev/ontology/onViolation> \"emit\" ;\n    sh:targetClass ex:W .\n",
            true,
        ),
        (
            "another prefix name",
            "@prefix q: <http://quipu.dev/ontology/> .\nex:S a sh:NodeShape ;\n    q:onViolation \"emit\" ;\n    sh:targetClass ex:W .\n",
            true,
        ),
        (
            "admission namespace",
            "ex:S a sh:NodeShape ;\n    <http://quipu.dev/ns#onViolation> \"emit\" ;\n    sh:targetClass ex:W .\n",
            true,
        ),
        (
            "unrelated quipu: namespace",
            "@prefix quipu: <https://quipu.dev/ontology/> .\nex:S a sh:NodeShape ;\n    quipu:onViolation \"emit\" ;\n    sh:targetClass ex:W .\n",
            false,
        ),
        (
            "language-tagged value",
            "ex:S a sh:NodeShape ;\n    quipu:onViolation \"emit\"@en ;\n    sh:targetClass ex:W .\n",
            false,
        ),
        (
            "conflicting policies",
            "ex:S a sh:NodeShape ;\n    quipu:onViolation \"emit\", \"reject\" ;\n    sh:targetClass ex:W .\n",
            false,
        ),
        // One block, two top-level subjects: an unannotated one keeps it REJECT.
        (
            "unannotated second root",
            "ex:H a sh:NodeShape . ex:S a sh:NodeShape ; quipu:onViolation \"emit\" ;\n    sh:targetClass ex:W .\n",
            false,
        ),
        (
            "does not parse",
            "ex:S a sh:NodeShape ;\n    quipu:onViolation \"emit\" ;\n    sh:targetClass ex:W ex:X .\n",
            false,
        ),
        (
            "plain annotation",
            "ex:S a sh:NodeShape ;\n    quipu:onViolation \"emit\" ;\n    sh:targetClass ex:W .\n",
            true,
        ),
    ];
    for (name, block, expected) in cases {
        let doc = format!("{HEAD}{block}");
        let split = split_shapes_by_policy(&doc);
        assert_eq!(split.has_emit, expected, "{name}");
        let (home, other) = if expected {
            (&split.emit, &split.reject)
        } else {
            (&split.reject, &split.emit)
        };
        assert!(home.contains("ex:S a sh:NodeShape"), "{name}");
        assert!(!other.contains("ex:S a sh:NodeShape"), "{name}");
    }
}
