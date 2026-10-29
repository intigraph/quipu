use crate::{
    Store,
    rdf::ingest_rdf_to_graph,
    sparql::{Bindings, query},
};
use oxrdfio::RdfFormat;
use std::cell::Cell;

thread_local! {
    static DISABLED: Cell<bool> = const { Cell::new(false) };
    static HITS: Cell<usize> = const { Cell::new(0) };
}
pub(super) fn hit() {
    HITS.set(HITS.get() + 1);
}
pub(super) fn disabled() -> bool {
    DISABLED.get()
}
fn reference<T>(run: impl FnOnce() -> T) -> T {
    struct Reset(bool);
    impl Drop for Reset {
        fn drop(&mut self) {
            DISABLED.set(self.0);
        }
    }
    let _reset = Reset(DISABLED.replace(true));
    run()
}
fn bag(rows: &[Bindings]) -> Vec<String> {
    let mut values: Vec<_> = rows
        .iter()
        .map(|row| {
            let mut pairs: Vec<_> = row.iter().collect();
            pairs.sort_by_key(|(k, _)| *k);
            format!("{pairs:?}")
        })
        .collect();
    values.sort();
    values
}
fn put(s: &mut Store, g: &str, data: &str) {
    let g = s.graph_create(g).unwrap();
    ingest_rdf_to_graph(
        s,
        data.as_bytes(),
        RdfFormat::NTriples,
        None,
        "2026-01-01T00:00:00Z",
        None,
        None,
        g,
    )
    .unwrap();
}
fn fixture_with_closed(count: usize) -> Store {
    let mut s = Store::open_in_memory().unwrap();
    let mut text = String::new();
    for (name, values) in [
        ("known", vec!["\"open\""]),
        ("closed", vec!["\"closed\""]),
        ("unknown", vec!["\"weird\""]),
        ("language", vec!["\"open\"@en"]),
        (
            "typed",
            vec!["\"7\"^^<http://www.w3.org/2001/XMLSchema#integer>"],
        ),
        ("mixed", vec!["\"closed\"", "\"weird\"", "\"another\""]),
        ("missing", vec![]),
        ("ref", vec!["<http://example.org/weird>"]),
    ] {
        text += &format!("<http://example.org/{name}> a <http://example.org/T> .\n")
            .replace(" a ", " <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> ");
        for owner in ["<http://example.org/owner>", "\"owner\""] {
            text += &format!("<http://example.org/{name}> <http://example.org/owner> {owner} .\n");
        }
        for value in values {
            text += &format!("<http://example.org/{name}> <http://example.org/status> {value} .\n");
        }
    }
    // A dense closed owner proves the optimizer's cost admission. These rows
    // must not leak into the open/default compatibility result.
    for i in 0..count {
        text += &format!(
            "<http://example.org/closed{i}> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://example.org/T> .\n"
        );
        text += &format!(
            "<http://example.org/closed{i}> <http://example.org/owner> <http://example.org/owner> .\n"
        );
        text += &format!("<http://example.org/closed{i}> <http://example.org/owner> \"owner\" .\n");
        text +=
            &format!("<http://example.org/closed{i}> <http://example.org/status> \"closed\" .\n");
    }
    put(&mut s, "http://example.org/g", &text);
    put(
        &mut s,
        "http://example.org/other",
        "<http://example.org/known> <http://example.org/status> \"weird\" .",
    );
    s
}
fn text(owner: &str) -> String {
    format!(
        "SELECT ?s ?v WHERE {{ GRAPH <http://example.org/g> {{ ?s <http://example.org/owner> {owner} ; a <http://example.org/T> ; <http://example.org/status> ?v . FILTER(isLiteral(?v) && STR(?v) NOT IN (\"open\",\"closed\")) }} }}"
    )
}
#[test]
fn unknown_status_preserves_rdf_terms_owner_kinds_graphs_and_mixed_value_bags() {
    let s = fixture();
    for owner in ["<http://example.org/owner>", "\"owner\""] {
        let q = text(owner);
        HITS.set(0);
        let actual = query(&s, &q).unwrap();
        assert!(HITS.get() > 0, "candidate path was never exercised");
        let original = reference(|| query(&s, &q)).unwrap();
        assert_eq!(bag(actual.rows()), bag(original.rows()));
        assert_eq!(actual.rows().len(), 4); // unknown, numeric, two mixed values
    }
}
#[test]
fn unknown_status_fallback_preserves_large_candidate_sets_and_limit_ordering() {
    let mut s = fixture();
    let mut text = String::new();
    for i in 0..140 {
        text += &format!(
            "<http://example.org/item{i}> <http://example.org/owner> \"owner\" ; a <http://example.org/T> ; <http://example.org/status> \"weird\" .\n"
        );
    }
    // Turtle is intentionally parsed as Turtle for the compact fixture.
    let g = s.lookup("http://example.org/g").unwrap().unwrap();
    ingest_rdf_to_graph(
        &mut s,
        text.as_bytes(),
        RdfFormat::Turtle,
        None,
        "2026-01-01T00:00:00Z",
        None,
        None,
        g,
    )
    .unwrap();
    let q = format!("{} ORDER BY STR(?s) LIMIT 7", text_query());
    let actual = query(&s, &q).unwrap();
    let original = reference(|| query(&s, &q)).unwrap();
    assert_eq!(bag(actual.rows()), bag(original.rows()));
    assert_eq!(actual.rows().len(), 7);
}
fn text_query() -> String {
    text("\"owner\"")
}

#[test]
fn compatibility_union_keeps_missing_unknown_and_legacy_literal_owners() {
    let s = fixture();
    for owner in ["<http://example.org/owner>", "\"owner\""] {
        let q = format!(
            r#"SELECT ?s WHERE {{ GRAPH <http://example.org/g> {{
            {{ ?s <http://example.org/owner> {owner}; a <http://example.org/T>; <http://example.org/status> "open" }}
            UNION {{ ?s <http://example.org/owner> {owner}; a <http://example.org/T>; <http://example.org/status> ?v
                FILTER(isLiteral(?v) && STR(?v) NOT IN ("open","closed")) }}
            UNION {{ ?s <http://example.org/owner> {owner}; a <http://example.org/T>
                FILTER NOT EXISTS {{ ?s <http://example.org/status> ?v FILTER(isLiteral(?v) && sameTerm(?v, STR(?v))) }} }}
        }} }}"#
        );
        let actual = query(&s, &q).unwrap();
        let original = reference(|| query(&s, &q)).unwrap();
        assert_eq!(bag(actual.rows()), bag(original.rows()));
        for name in [
            "known", "unknown", "missing", "ref", "language", "typed", "mixed",
        ] {
            let id = s
                .lookup(&format!("http://example.org/{name}"))
                .unwrap()
                .unwrap();
            assert!(
                actual
                    .rows()
                    .iter()
                    .any(|r| r.get("s") == Some(&crate::Value::Ref(id))),
                "lost {name}"
            );
        }
    }
}

fn fixture() -> Store {
    fixture_with_closed(4100)
}

#[test]
fn selective_owners_do_not_pay_for_a_global_property_scan() {
    let s = fixture_with_closed(0);
    let q = text("\"owner\"");
    HITS.set(0);
    let actual = query(&s, &q).unwrap();
    assert_eq!(
        HITS.get(),
        0,
        "small owner incorrectly admitted to global scan"
    );
    let original = reference(|| query(&s, &q)).unwrap();
    assert_eq!(bag(actual.rows()), bag(original.rows()));
    assert_eq!(actual.rows().len(), 4);
}
