use super::*;
use std::collections::BTreeSet;

const TS: &str = "2026-01-01T00:00:00Z";
const PREFIX: &str = "@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> . \
    @prefix owl: <http://www.w3.org/2002/07/owl#> .";

fn store(axioms: &str) -> Store {
    let source = Store::open_in_memory().unwrap();
    source
        .load_ontology("test", &format!("{PREFIX} {axioms}"), TS)
        .unwrap();
    source
}

fn fact(source: &mut Store, e: &str, a: &str, v: &str, graph: i64) {
    let datum = Datum {
        entity: source.intern(e).unwrap(),
        attribute: source.intern(a).unwrap(),
        value: Value::Ref(source.intern(v).unwrap()),
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    };
    // The companion fixture represents an existing engine-derived premise.
    // Use the materializer's normal provenance; external writes stay refused.
    let derived = graph != crate::schema::ROOT_GRAPH;
    source
        .transact_to_graph(
            &[datum],
            TS,
            derived.then_some("owl"),
            derived.then_some("owl:materialize"),
            graph,
        )
        .unwrap();
}

fn capture(source: &Store, budget: usize) -> Result<Snapshot> {
    Snapshot::capture_with_limit(source, Store::open_in_memory().unwrap(), TS, budget)
}

// Compare logical proposals, not independently allocated scratch term ids.
fn proposals(source: &Store, before: i64) -> BTreeSet<(String, String, String)> {
    let mut stmt = source
        .conn
        .prepare(
            "SELECT f.e,f.a,f.v FROM facts f JOIN transactions t ON t.id=f.tx \
         WHERE f.tx>?1 AND t.source='owl:materialize' AND f.op=1 AND f.valid_to IS NULL",
        )
        .unwrap();
    stmt.query_map([before], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, Vec<u8>>(2)?,
        ))
    })
    .unwrap()
    .map(|row| {
        let (e, a, v) = row.unwrap();
        let value = match Value::from_bytes(&v).unwrap() {
            Value::Ref(id) => source.resolve(id).unwrap(),
            other => panic!("unexpected fixture value: {other:?}"),
        };
        (
            source.resolve(e).unwrap(),
            source.resolve(a).unwrap(),
            value,
        )
    })
    .collect()
}

fn assert_full_parity(source: &mut Store, budget: usize) -> BTreeSet<(String, String, String)> {
    let mut plan = capture(source, budget).unwrap();
    let scratch_head = plan.scratch.transaction_head().unwrap();
    plan.derive().unwrap();
    let selected = proposals(&plan.scratch, scratch_head);
    source.ensure_owl_cache().unwrap();
    let ontology = source.owl_cache.as_deref().unwrap().clone();
    let full_head = source.transaction_head().unwrap();
    ontology
        .materialize_limited(source, TS, MAX_PROPOSALS)
        .unwrap();
    assert_eq!(selected, proposals(source, full_head));
    selected
}

#[test]
fn unrelated_rows_do_not_consume_the_semantic_premise_budget() {
    let mut source = store("<urn:p> rdfs:subPropertyOf <urn:q> .");
    fact(&mut source, "urn:a", "urn:p", "urn:b", 0);
    for n in 0..20 {
        fact(
            &mut source,
            &format!("urn:noise:{n}"),
            "urn:unrelated",
            "urn:value",
            0,
        );
    }
    assert_eq!(
        capture(&source, 1)
            .unwrap()
            .scratch
            .current_facts()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(assert_full_parity(&mut source, 1).len(), 1);
}

#[test]
fn relevant_rows_still_refuse_at_the_exact_budget() {
    let mut source = store("<urn:p> rdfs:subPropertyOf <urn:q> .");
    fact(&mut source, "urn:a", "urn:p", "urn:b", 0);
    fact(&mut source, "urn:c", "urn:p", "urn:d", 0);
    assert!(
        capture(&source, 1)
            .err()
            .unwrap()
            .to_string()
            .contains("premise budget exceeded")
    );
    assert!(capture(&source, 2).is_ok());
}

#[test]
fn identity_in_either_plane_keeps_arbitrary_subject_and_object_rewrites() {
    let mut source = store("<urn:Child> rdfs:subClassOf <urn:Parent> .");
    let companion = source.intern(ROOT_INFERRED_GRAPH_IRI).unwrap();
    fact(
        &mut source,
        "urn:a",
        crate::namespace::OWL_SAME_AS,
        "urn:b",
        0,
    );
    fact(
        &mut source,
        "urn:b",
        crate::namespace::OWL_SAME_AS,
        "urn:c",
        companion,
    );
    fact(&mut source, "urn:a", "urn:arbitrary", "urn:value", 0);
    fact(&mut source, "urn:outside", "urn:other", "urn:a", 0);
    let derived = assert_full_parity(&mut source, 4);
    assert!(derived.contains(&("urn:c".into(), "urn:arbitrary".into(), "urn:value".into())));
    assert!(derived.contains(&("urn:outside".into(), "urn:other".into(), "urn:c".into())));
}

#[test]
fn overlapping_attribute_subject_object_passes_count_each_row_once() {
    let mut source = store("<urn:p> rdfs:subPropertyOf <urn:q> .");
    fact(
        &mut source,
        "urn:a",
        crate::namespace::OWL_SAME_AS,
        "urn:b",
        0,
    );
    fact(&mut source, "urn:a", "urn:p", "urn:b", 0);
    assert_eq!(
        capture(&source, 2)
            .unwrap()
            .scratch
            .current_facts()
            .unwrap()
            .len(),
        2
    );
    assert_full_parity(&mut source, 2);
}

#[test]
fn property_generated_identity_falls_back_to_full_input_and_keeps_the_cap() {
    let mut source = store("<urn:p> rdfs:subPropertyOf owl:sameAs .");
    fact(&mut source, "urn:a", "urn:p", "urn:b", 0);
    fact(&mut source, "urn:a", "urn:arbitrary", "urn:value", 0);
    assert!(capture(&source, 1).is_err());
    let derived = assert_full_parity(&mut source, 2);
    assert!(derived.contains(&("urn:b".into(), "urn:arbitrary".into(), "urn:value".into())));
}

#[test]
fn property_and_class_rule_families_compose_with_the_same_full_closure() {
    let mut source = store(
        "<urn:p> rdfs:subPropertyOf <urn:q> . \
        <urn:q> owl:equivalentProperty <urn:r> ; owl:inverseOf <urn:inverse> ; \
        rdfs:domain <urn:Child> ; rdfs:range <urn:Child> . \
        <urn:Child> rdfs:subClassOf <urn:Parent> ; owl:equivalentClass <urn:Peer> . \
        <urn:r> a owl:SymmetricProperty, owl:TransitiveProperty .",
    );
    fact(&mut source, "urn:a", "urn:p", "urn:b", 0);
    fact(&mut source, "urn:b", "urn:r", "urn:c", 0);
    fact(&mut source, "urn:noise", "urn:unrelated", "urn:value", 0);
    let derived = assert_full_parity(&mut source, 2);
    assert!(derived.contains(&("urn:a".into(), "urn:r".into(), "urn:c".into())));
    assert!(derived.contains(&(
        "urn:c".into(),
        crate::namespace::RDF_TYPE.into(),
        "urn:Parent".into()
    )));
}

#[test]
fn attribute_selection_crosses_sql_chunks_without_truncation() {
    let axioms = (0..257)
        .map(|n| format!("<urn:p:{n}> rdfs:subPropertyOf <urn:q> ."))
        .collect::<Vec<_>>()
        .join("\n");
    let mut source = store(&axioms);
    for n in 0..257 {
        fact(
            &mut source,
            &format!("urn:e:{n}"),
            &format!("urn:p:{n}"),
            "urn:v",
            0,
        );
    }
    assert_eq!(
        capture(&source, 257)
            .unwrap()
            .scratch
            .current_facts()
            .unwrap()
            .len(),
        257
    );
    assert_eq!(assert_full_parity(&mut source, 257).len(), 257);
}

#[test]
fn identity_object_selection_crosses_sql_chunks_without_truncation() {
    let mut source = store("<urn:Child> rdfs:subClassOf <urn:Parent> .");
    for n in 0..130 {
        let a = format!("urn:a:{n}");
        let b = format!("urn:b:{n}");
        fact(&mut source, &a, crate::namespace::OWL_SAME_AS, &b, 0);
        fact(
            &mut source,
            &format!("urn:outside:{n}"),
            "urn:arbitrary",
            &a,
            0,
        );
    }
    let mut plan = capture(&source, 260).unwrap();
    assert_eq!(plan.scratch.current_facts().unwrap().len(), 260);
    plan.derive().unwrap();
    assert_eq!(plan.report.same_as_inferences, 260);
    assert_eq!(assert_full_parity(&mut source, 260).len(), 260);
}
