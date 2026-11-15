use super::*;
use crate::Value;
fn triple(lexical: &str, datatype: &str) -> TripleValue {
    TripleValue::new(
        "urn:subject".into(),
        "urn:predicate".into(),
        TripleObject::Literal {
            lexical: lexical.into(),
            datatype: datatype.into(),
            language: None,
        },
    )
    .unwrap()
}
#[test]
fn tagged_triple_roundtrip_preserves_literal_identity_and_never_aliases_a_string() {
    let values = [
        triple("01", crate::namespace::XSD_INTEGER),
        triple("1", crate::namespace::XSD_INTEGER),
        triple("01", crate::namespace::XSD_STRING),
    ];
    for t in &values {
        let v = Value::Triple(Box::new(t.clone()));
        assert_eq!(Value::from_bytes(&v.to_bytes()).unwrap(), v);
        assert_ne!(v, Value::Str(String::from_utf8(t.encode()).unwrap()));
    }
    assert_ne!(values[0], values[1]);
    assert_ne!(values[0], values[2]);
    let iri = TripleValue::new(
        "urn:s".into(),
        "urn:p".into(),
        TripleObject::Iri {
            iri: "urn:value".into(),
        },
    )
    .unwrap();
    assert_eq!(TripleValue::decode(&iri.encode()).unwrap(), iri);
}
#[test]
fn malformed_oversized_and_forged_deserialization_refuse() {
    assert!(
        TripleValue::new(
            "relative".into(),
            "urn:p".into(),
            TripleObject::Iri {
                iri: "urn:o".into()
            }
        )
        .is_err()
    );
    let t = triple("ok", crate::namespace::XSD_STRING);
    let mut wire: serde_json::Value = serde_json::from_slice(&t.encode()).unwrap();
    wire["subject"] = serde_json::json!("relative");
    let bytes = serde_json::to_vec(&wire).unwrap();
    assert!(TripleValue::decode(&bytes).is_err());
    assert!(serde_json::from_slice::<TripleValue>(&bytes).is_err());
    assert!(TripleValue::decode(&vec![b' '; MAX_BYTES + 1]).is_err());
    assert!(TripleValue::decode(b"{}").is_err());
    assert!(Value::from_bytes(&[8, b'{']).is_err());
}
#[test]
fn language_identity_and_required_datatype_are_checked() {
    let object = TripleObject::Literal {
        lexical: "hello@en".into(),
        datatype: oxrdf::vocab::rdf::LANG_STRING.as_str().into(),
        language: Some("en".into()),
    };
    let t = TripleValue::new("urn:s".into(), "urn:p".into(), object).unwrap();
    assert_eq!(TripleValue::decode(&t.encode()).unwrap(), t);
    assert!(
        TripleValue::new(
            "urn:s".into(),
            "urn:p".into(),
            TripleObject::Literal {
                lexical: "x".into(),
                datatype: crate::namespace::XSD_STRING.into(),
                language: Some("en".into())
            }
        )
        .is_err()
    );
}

#[cfg(feature = "shacl")]
#[test]
fn native_rdf_projection_survives_named_graph_disk_and_standard_reader() {
    use oxrdf::{Term, Triple};
    use oxrdfio::{RdfFormat, RdfSerializer};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("projection.db");
    let mut store = crate::Store::open(path.to_str().unwrap()).unwrap();
    store.load_shapes("projection-control", "@prefix sh: <http://www.w3.org/ns/shacl#> . \
        <urn:projection-shape> a sh:NodeShape; sh:targetNode <urn:projection>; \
        sh:property [ sh:path <http://www.w3.org/1999/02/22-rdf-syntax-ns#reifies>; sh:maxCount 1 ] .",
        "2026-11-08T00:00:00Z").unwrap();
    let graph = store.intern("urn:projection-graph").unwrap();
    let inner = triple("01", crate::namespace::XSD_INTEGER).to_ox().unwrap();
    let outer = Triple::new(
        NamedNode::new_unchecked("urn:projection"),
        NamedNode::new_unchecked("http://www.w3.org/1999/02/22-rdf-syntax-ns#reifies"),
        Term::Triple(Box::new(inner.clone())),
    );
    let mut serializer = RdfSerializer::from_format(RdfFormat::NTriples).for_writer(Vec::new());
    serializer.serialize_triple(&outer).unwrap();
    let document = serializer.finish().unwrap();
    crate::rdf::ingest_rdf_to_graph(
        &mut store,
        document.as_slice(),
        RdfFormat::NTriples,
        None,
        "2026-11-08T00:00:00Z",
        None,
        Some("private-projection-fixture"),
        graph,
    )
    .unwrap();
    let projection = store.lookup("urn:projection").unwrap().unwrap();
    assert!(
        store
            .entity_facts_in_graph(projection, 0)
            .unwrap()
            .is_empty()
    );
    let facts = store.entity_facts_in_graph(projection, graph).unwrap();
    assert_eq!(facts.len(), 1);
    assert!(matches!(facts[0].value, Value::Triple(_)));
    assert_eq!(
        crate::rdf::value_to_term(&store, &facts[0].value).unwrap(),
        outer.object
    );
    // Reifying a triple must never assert that triple as an ordinary slot.
    assert!(store.lookup("urn:subject").unwrap().is_none());
    let bytes = facts[0].value.to_bytes();
    drop(store);
    let store = crate::Store::open(path.to_str().unwrap()).unwrap();
    let facts = store.entity_facts_in_graph(projection, graph).unwrap();
    assert_eq!(facts[0].value.to_bytes(), bytes);
    let output = crate::rdf::serialize_facts(&store, &facts, RdfFormat::NTriples).unwrap();
    assert_eq!(output, document);
    let result = crate::sparql_query(&store,
        "SELECT ?o WHERE { GRAPH <urn:projection-graph> { <urn:projection> <http://www.w3.org/1999/02/22-rdf-syntax-ns#reifies> ?o } }").unwrap();
    assert_eq!(result.rows().len(), 1);
    assert_eq!(result.rows()[0]["o"], facts[0].value);
    let bespoke = crate::mcp::value_to_json(&store, &facts[0].value);
    assert!(bespoke["triple"].is_object());
    let check = crate::sparql_query(&store, "SELECT (isLiteral(?o) AS ?literal) (STR(?o) AS ?text) WHERE { GRAPH <urn:projection-graph> { <urn:projection> <http://www.w3.org/1999/02/22-rdf-syntax-ns#reifies> ?o } }").unwrap();
    assert_eq!(check.rows().len(), 1);
    assert_eq!(check.rows()[0]["literal"], Value::Bool(false));
    assert!(!check.rows()[0].contains_key("text"));

    let (_, json) = crate::w3c::serialize(&store, &result, crate::w3c::ResultFormat::SparqlJson)
        .unwrap()
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&json).unwrap();
    assert_eq!(json["results"]["bindings"][0]["o"]["type"], "triple");
}

#[cfg(feature = "shacl")]
#[test]
fn unsupported_nested_or_blank_triples_refuse_without_interning() {
    use oxrdf::{BlankNode, Term, Triple};
    let store = crate::Store::open_in_memory().unwrap();
    let before = store
        .conn
        .query_row("SELECT count(*) FROM terms", [], |r| r.get::<_, i64>(0))
        .unwrap();
    let blank = Triple::new(
        BlankNode::new_unchecked("private"),
        NamedNode::new_unchecked("urn:p"),
        NamedNode::new_unchecked("urn:o"),
    );
    assert!(crate::rdf::term_to_value(&store, &Term::Triple(Box::new(blank))).is_err());
    let inner = triple("x", crate::namespace::XSD_STRING).to_ox().unwrap();
    let nested = Triple::new(
        NamedNode::new_unchecked("urn:s"),
        NamedNode::new_unchecked("urn:p"),
        Term::Triple(Box::new(inner)),
    );
    assert!(crate::rdf::term_to_value(&store, &Term::Triple(Box::new(nested))).is_err());
    assert_eq!(
        store
            .conn
            .query_row("SELECT count(*) FROM terms", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        before
    );
}

#[test]
fn new_triple_kind_does_not_inflate_every_ordinary_value() {
    assert!(
        std::mem::size_of::<Value>() <= 64,
        "global Value footprint must stay compact"
    );
}
