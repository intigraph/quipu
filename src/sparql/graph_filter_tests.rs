//! Differential answers against an OR-false wrapper that retains unpruned scans.
use super::{graph_filter, query};
use crate::{
    Store, namespace,
    store::Datum,
    types::{Op, Value},
};
const TS: &str = "2026-01-01T00:00:00Z";
fn populate(store: &mut Store) {
    let a = store.intern(&format!("{}type", namespace::RDF)).unwrap();
    let t = store.intern("urn:Type").unwrap();
    for i in 0..26 {
        let name = if i < 7 {
            format!("urn:seeds:graph:{i}")
        } else {
            format!("urn:other:graph:{i}")
        };
        let g = store.graph_create(&name).unwrap();
        let datums: Vec<_> = (0..8)
            .map(|j| Datum {
                entity: store.intern(&format!("urn:entity:{i}:{j}")).unwrap(),
                attribute: a,
                value: Value::Ref(t),
                valid_from: TS.into(),
                valid_to: None,
                op: Op::Assert,
            })
            .collect();
        store.transact_to_graph(&datums, TS, None, None, g).unwrap();
    }
    for name in ["urn:seeds:graph:0", "urn:Unicode:İΩ"] {
        let g = store.graph_create(name).unwrap();
        let datum = Datum {
            entity: g,
            attribute: a,
            value: Value::Ref(t),
            valid_from: TS.into(),
            valid_to: None,
            op: Op::Assert,
        };
        store
            .transact_to_graph(&[datum], TS, None, None, g)
            .unwrap();
    }
}
fn fixture() -> Store {
    let mut s = Store::open_in_memory().unwrap();
    populate(&mut s);
    s
}
fn rows(store: &Store, q: &str) -> Vec<Vec<(String, String)>> {
    let r = query(store, q).unwrap();
    let mut rows: Vec<_> = r
        .rows()
        .iter()
        .map(|r| {
            let mut v: Vec<_> = r
                .iter()
                .map(|(k, v)| (k.clone(), format!("{v:?}")))
                .collect();
            v.sort();
            v
        })
        .collect();
    rows.sort();
    rows
}
fn compare(store: &Store, pattern: &str, expr: &str, prefix: &str) -> (usize, usize) {
    let q = |e: &str| format!("SELECT * {prefix} WHERE {{ {pattern} FILTER({e}) }}");
    graph_filter::scans(true);
    let actual = rows(store, &q(expr));
    let narrowed = graph_filter::scans(true);
    let expected = rows(store, &q(&format!("({expr}) || false")));
    let original = graph_filter::scans(true);
    assert_eq!(actual, expected, "{pattern} FILTER({expr})");
    (narrowed, original)
}
#[test]
fn graph_filter_reduces_actual_leaf_scans_and_preserves_count_groups() {
    let store = fixture();
    let (new, old) = compare(
        &store,
        "GRAPH ?g { ?s a ?t }",
        "CONTAINS(STR(?g),\"seeds\")",
        "",
    );
    println!("GRAPH_LEAF_SCANS original={old} narrowed={new}");
    assert_eq!(new, 7);
    assert!(old >= 26);
    assert!(new < old);
    let q = |e: &str| {
        format!(
            "SELECT ?g (COUNT(*) AS ?n) WHERE {{ GRAPH ?g {{ ?s a ?t }} FILTER({e}) }} GROUP BY ?g"
        )
    };
    assert_eq!(
        rows(&store, &q("CONTAINS(STR(?g),\"seeds\")")),
        rows(&store, &q("CONTAINS(STR(?g),\"seeds\") || false"))
    );
}
#[test]
fn single_string_tests_keep_matching_empty_and_no_match_answers() {
    let store = fixture();
    for expr in [
        "CONTAINS(STR(?g),\"seeds\")",
        "STRSTARTS(STR(?g),\"urn:seeds:\")",
        "STRENDS(STR(?g),\":0\")",
        "REGEX(STR(?g),\"SEEDS\",\"i\")",
        "CONTAINS(LCASE(STR(?g)),\"seeds\")",
        "CONTAINS(UCASE(STR(?g)),\"SEEDS\")",
        "CONTAINS(STR(?g),\"\")",
        "CONTAINS(STR(?g),\"unmatched\")",
        "CONTAINS(?g,\"seeds\")",
        "CONTAINS(LCASE(STR(?g)),\"i̇\")",
        "REGEX(STR(?g),\"Ω\")",
    ] {
        compare(&store, "GRAPH ?g { ?s a ?t }", expr, "");
    }
}
#[test]
fn from_named_intersection_and_graph_variable_in_bgp_are_preserved() {
    let store = fixture();
    assert_eq!(
        compare(
            &store,
            "GRAPH ?g { ?s a ?t }",
            "CONTAINS(STR(?g),\"seeds\")",
            "FROM NAMED <urn:seeds:graph:0> FROM NAMED <urn:other:graph:9>"
        )
        .0,
        1
    );
    assert_eq!(
        compare(
            &store,
            "GRAPH ?g { ?s a ?t }",
            "CONTAINS(STR(?g),\"seeds\")",
            "FROM NAMED <urn:other:graph:9>"
        )
        .0,
        0
    );
    compare(
        &store,
        "GRAPH ?g { ?g ?p ?o }",
        "CONTAINS(STR(?g),\"seeds\")",
        "",
    );
    compare(
        &store,
        "VALUES ?g { <urn:seeds:graph:0> <urn:other:graph:9> \"literal\" } GRAPH ?g { ?s a ?t }",
        "CONTAINS(STR(?g),\"seeds\")",
        "",
    );
}
#[test]
fn complex_and_nested_graph_patterns_retain_unpruned_scope() {
    let store = fixture();
    for pattern in [
        "GRAPH ?g { GRAPH ?h { ?s a ?t } }",
        "GRAPH ?g { { SELECT ?s ?t WHERE { GRAPH ?h { ?s a ?t } } } }",
        "GRAPH ?g { ?s a ?t OPTIONAL { ?s <urn:missing> ?o } }",
    ] {
        let (new, old) = compare(&store, pattern, "CONTAINS(STR(?g),\"seeds\")", "");
        assert_eq!(new, old);
    }
    for expr in [
        "CONTAINS(STR(?g),\"seeds\") || STRSTARTS(STR(?g),\"urn:other\")",
        "!CONTAINS(STR(?g),\"seeds\")",
        "CONTAINS(STR(?g),\"seeds\") && BOUND(?t)",
        "CONTAINS(STR(?s),\"entity\")",
    ] {
        let (new, old) = compare(&store, "GRAPH ?g { ?s a ?t }", expr, "");
        assert_eq!(new, old);
    }
}
#[test]
fn attached_graph_candidates_keep_composed_answers() {
    use crate::store::attach::Attachment;
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("source.db");
    let layer = d.path().join("layer.db");
    let main = d.path().join("main.db");
    {
        let mut s = Store::open(src.to_str().unwrap()).unwrap();
        populate(&mut s);
    }
    crate::store::respace::respace_file(&src, &layer, 19).unwrap();
    let s = Store::open_with_attachments(
        main.to_str().unwrap(),
        &[Attachment::read_only("layer", layer.to_str().unwrap())],
    )
    .unwrap();
    let (new, old) = compare(
        &s,
        "GRAPH ?g { ?s a ?t }",
        "CONTAINS(STR(?g),\"seeds\")",
        "",
    );
    assert_eq!(new, 7);
    assert!(old >= 26);
}
