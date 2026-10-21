//! ORDER BY + LIMIT below OPTIONAL hydration (aegis-xacr) against the full sort.
//!
//! The oracle is the same query without its LIMIT, which never rewrites, cut to
//! the same window. A test that only compares answers passes whether or not the
//! rewrite engaged, so engagement is pinned separately on the parsed algebra.

use oxrdfio::RdfFormat;
use spargebra::algebra::GraphPattern;
use spargebra::{Query, SparqlParser};

use super::order_pushdown::rewrite;
use super::{Bindings, TemporalContext, query_temporal};
use crate::rdf::ingest_rdf;
use crate::store::Store;

fn store() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    let mut ttl = String::from("@prefix ex: <http://example.org/> .\n");
    for i in 0..40 {
        // Ten distinct end times, so most ranks are four-way ties.
        ttl.push_str(&format!(
            "ex:i{i:02} ex:outcome \"{}\" ; ex:end {} .\n",
            if i % 7 == 0 { "dropped" } else { "done" },
            i % 10
        ));
        if i % 3 != 0 {
            ttl.push_str(&format!("ex:i{i:02} ex:name \"n{i}\" .\n"));
        }
        if i % 4 == 0 {
            // Multi-valued: one left row expands to two joined rows.
            ttl.push_str(&format!("ex:i{i:02} ex:name \"alt{i}\" .\n"));
        }
        if i % 5 == 0 {
            ttl.push_str(&format!("ex:i{i:02} ex:result \"r{i}\" .\n"));
        }
    }
    ingest_rdf(
        &mut store,
        ttl.as_bytes(),
        RdfFormat::Turtle,
        None,
        "2026-10-21T00:00:00Z",
        None,
        None,
    )
    .unwrap();
    store
}

fn ordered(rows: &[Bindings]) -> Vec<String> {
    rows.iter()
        .map(|r| {
            let mut kv: Vec<String> = r.iter().map(|(k, v)| format!("{k}={v:?}")).collect();
            kv.sort();
            kv.join(",")
        })
        .collect()
}

const WHERE: &str = r#"WHERE { ?item ex:outcome "done" ; ex:end ?end .
    FILTER(?end >= 3)
    OPTIONAL { ?item ex:name ?name }
    OPTIONAL { ?item ex:result ?result } }"#;

/// `ORDER BY <order> LIMIT k OFFSET o` equals the unlimited answer's window.
fn same_window(order: &str, offset: usize, k: usize) {
    let store = store();
    let ctx = TemporalContext::default();
    let head = "PREFIX ex: <http://example.org/> SELECT ?item ?end ?name ?result";
    let full = query_temporal(&store, &format!("{head} {WHERE} ORDER BY {order}"), &ctx).unwrap();
    let limited = query_temporal(
        &store,
        &format!("{head} {WHERE} ORDER BY {order} LIMIT {k} OFFSET {offset}"),
        &ctx,
    )
    .unwrap();
    let expected: Vec<String> = ordered(full.rows())
        .into_iter()
        .skip(offset)
        .take(k)
        .collect();
    assert_eq!(
        ordered(limited.rows()),
        expected,
        "ORDER BY {order} LIMIT {k} OFFSET {offset}"
    );
    assert!(!expected.is_empty());
}

#[test]
fn limited_answer_is_the_full_sort_window_including_ties_and_offsets() {
    for order in [
        "DESC(?end) ?item",
        "DESC(?end)",
        "?end",
        "?item",
        // Reads an OPTIONAL variable: must keep the full evaluation.
        "?name ?item",
    ] {
        for (offset, k) in [(0, 1), (0, 5), (0, 7), (3, 4), (6, 9), (0, 200)] {
            same_window(order, offset, k);
        }
    }
}

fn order_by(sparql: &str) -> (GraphPattern, Vec<spargebra::algebra::OrderExpression>) {
    let Query::Select { pattern, .. } = SparqlParser::new().parse_query(sparql).unwrap() else {
        panic!("not a SELECT");
    };
    let mut p = pattern;
    loop {
        p = match p {
            GraphPattern::Slice { inner, .. }
            | GraphPattern::Project { inner, .. }
            | GraphPattern::Distinct { inner } => *inner,
            GraphPattern::OrderBy { inner, expression } => return (*inner, expression),
            other => panic!("no ORDER BY in {other:?}"),
        };
    }
}

fn engages(sparql: &str) -> bool {
    let (inner, keys) = order_by(sparql);
    rewrite(&inner, &keys, 10).is_some()
}

#[test]
fn engages_for_the_panel_shape_inside_and_outside_graph() {
    let p = "PREFIX ex: <http://example.org/> SELECT *";
    assert!(engages(&format!(
        "{p} {WHERE} ORDER BY DESC(?end) LIMIT 10"
    )));
    assert!(engages(&format!(
        "{p} WHERE {{ GRAPH <http://example.org/g> {{ ?item ex:end ?end OPTIONAL {{ ?item ex:name ?n }} }} }} ORDER BY ?end LIMIT 10"
    )));
}

#[test]
fn declines_what_an_optional_or_the_context_could_change() {
    let p = "PREFIX ex: <http://example.org/> SELECT *";
    for (why, body, order) in [
        ("key reads an OPTIONAL variable", WHERE, "?name"),
        (
            "filter reads an OPTIONAL variable",
            r#"WHERE { ?item ex:end ?end OPTIONAL { ?item ex:name ?name } FILTER(BOUND(?name)) }"#,
            "?end",
        ),
        (
            "filter is EXISTS",
            r#"WHERE { ?item ex:end ?end OPTIONAL { ?item ex:name ?name } FILTER EXISTS { ?item ex:result ?r } }"#,
            "?end",
        ),
        ("key is volatile", WHERE, "RAND()"),
        (
            "no OPTIONAL to skip",
            r#"WHERE { ?item ex:end ?end }"#,
            "?end",
        ),
        (
            "left side is not a BGP",
            r#"WHERE { { ?item ex:end ?end } UNION { ?item ex:end ?end } OPTIONAL { ?item ex:name ?name } }"#,
            "?end",
        ),
    ] {
        assert!(
            !engages(&format!("{p} {body} ORDER BY {order} LIMIT 10")),
            "{why}"
        );
    }
}

/// Keys whose values mix comparison kinds (franklin's #700 counterexample:
/// ranks 10, "x", 1 with unmatched OPTIONALs). `compare_option_values` gives
/// `10 = "x" = 1` but `1 < 10`, so the order is not transitive and the rewrite
/// must fall back to the full evaluation.
fn mixed_store() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    let mut ttl = String::from(
        "@prefix ex: <http://example.org/> .\n@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n",
    );
    let ranks = [
        "10", "\"x\"", "1", "\"y\"", "5", "\"a\"", "3", "\"10\"", "7", "\"b\"",
    ];
    let tags = ["ex:t1", "\"m\"", "ex:t2", "\"a\"", "ex:t0"];
    let weights = ["\"NaN\"^^xsd:double", "2.5", "\"INF\"^^xsd:double", "1.0"];
    for i in 0..30 {
        ttl.push_str(&format!(
            "ex:i{i:02} ex:rank {} ; ex:tag {} ; ex:weight {} .\n",
            ranks[i % ranks.len()],
            tags[i % tags.len()],
            weights[i % weights.len()]
        ));
        if i % 3 == 0 {
            ttl.push_str(&format!("ex:i{i:02} ex:name \"n{i}\" .\n"));
        }
    }
    ingest_rdf(
        &mut store,
        ttl.as_bytes(),
        RdfFormat::Turtle,
        None,
        "2026-10-21T00:00:00Z",
        None,
        None,
    )
    .unwrap();
    store
}

#[test]
fn mixed_kind_keys_fall_back_to_the_full_sort_window() {
    let store = mixed_store();
    let ctx = TemporalContext::default();
    let head = "PREFIX ex: <http://example.org/> SELECT ?item ?rank ?tag ?weight ?name";
    let body = "WHERE { ?item ex:rank ?rank ; ex:tag ?tag ; ex:weight ?weight \
                OPTIONAL { ?item ex:name ?name } }";
    for order in [
        "?rank",
        "DESC(?rank) ?item",
        "?tag ?item",
        "?weight ?item",
        "?item",
    ] {
        let full =
            query_temporal(&store, &format!("{head} {body} ORDER BY {order}"), &ctx).unwrap();
        for (offset, k) in [(0, 1), (0, 3), (2, 5), (0, 9), (4, 12), (0, 40)] {
            let limited = query_temporal(
                &store,
                &format!("{head} {body} ORDER BY {order} LIMIT {k} OFFSET {offset}"),
                &ctx,
            )
            .unwrap();
            let expected: Vec<String> = ordered(full.rows())
                .into_iter()
                .skip(offset)
                .take(k)
                .collect();
            assert_eq!(
                ordered(limited.rows()),
                expected,
                "ORDER BY {order} LIMIT {k} OFFSET {offset}"
            );
        }
    }
}

/// franklin's numeric-domain counterexamples on #700: each value compares
/// equal to itself, yet exact, float and double comparisons do not compose
/// into one order. Every insertion order must give the oracle's windows.
fn numeric_domain_windows(ranks: [&str; 3]) {
    let subjects = ["a", "b", "c"];
    let orders: [[usize; 3]; 6] = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    for perm in orders {
        let mut store = Store::open_in_memory().unwrap();
        let mut ttl = String::from("@prefix ex: <http://example.test/> .\n");
        for &i in &perm {
            ttl.push_str(&format!("ex:{} ex:rank {} .\n", subjects[i], ranks[i]));
        }
        ingest_rdf(
            &mut store,
            ttl.as_bytes(),
            RdfFormat::Turtle,
            None,
            "2026-10-21T00:00:00Z",
            None,
            None,
        )
        .unwrap();
        let ctx = TemporalContext::default();
        let head = "PREFIX ex: <http://example.test/> SELECT ?s ?rank ?tag";
        let body = "WHERE { ?s ex:rank ?rank OPTIONAL { ?s ex:missing ?tag } }";
        for order in ["?rank ?s", "?rank", "DESC(?rank) ?s"] {
            let full =
                query_temporal(&store, &format!("{head} {body} ORDER BY {order}"), &ctx).unwrap();
            for (offset, k) in [(0, 1), (1, 2), (0, 2), (0, 20)] {
                let limited = query_temporal(
                    &store,
                    &format!("{head} {body} ORDER BY {order} LIMIT {k} OFFSET {offset}"),
                    &ctx,
                )
                .unwrap();
                let expected: Vec<String> = ordered(full.rows())
                    .into_iter()
                    .skip(offset)
                    .take(k)
                    .collect();
                assert_eq!(
                    ordered(limited.rows()),
                    expected,
                    "{ranks:?} order {perm:?} ORDER BY {order} LIMIT {k} OFFSET {offset}"
                );
            }
        }
    }
}

#[test]
fn exact_and_double_ranks_fall_back() {
    numeric_domain_windows([
        "9007199254740993",
        "9.007199254740992E15",
        "9007199254740992",
    ]);
}

#[test]
fn exact_and_float_ranks_fall_back() {
    numeric_domain_windows([
        "16777217",
        "\"16777216\"^^<http://www.w3.org/2001/XMLSchema#float>",
        "16777216",
    ]);
}

const META_GRAPH: &str = "urn:franklin:metadata";

fn graph_store(ttl: &str) -> Store {
    let mut store = Store::open_in_memory().unwrap();
    let g = store.graph_create(META_GRAPH).unwrap();
    crate::rdf::ingest_rdf_to_graph(
        &mut store,
        ttl.as_bytes(),
        RdfFormat::Turtle,
        None,
        "2026-10-21T00:00:00Z",
        None,
        None,
        g,
    )
    .unwrap();
    store
}

/// Rows AND header of the limited query equal the unlimited query's window.
fn same_answer(store: &Store, query: &str, k: usize) {
    let ctx = TemporalContext::default();
    let full = query_temporal(store, query, &ctx).unwrap();
    let limited = query_temporal(store, &format!("{query} LIMIT {k}"), &ctx).unwrap();
    let expected: Vec<String> = ordered(full.rows()).into_iter().take(k).collect();
    assert_eq!(ordered(limited.rows()), expected, "rows: {query} LIMIT {k}");
    assert_eq!(
        limited.variables(),
        full.variables(),
        "header: {query} LIMIT {k}"
    );
}

/// franklin's #700 metadata counterexamples: `merge_bindings` lets an OPTIONAL
/// subquery overwrite `_provider`/`_trust`/`_freshness`, and `Project` adds a
/// metadata column when any hydrated row carries it.
#[test]
fn response_metadata_overwrite_and_header_keep_the_full_evaluation() {
    let overwrite = graph_store(
        r#"@prefix ex: <http://example.test/> .
ex:a ex:rank "1" ; ex:other "3" .
ex:b ex:rank "2" ; ex:other "0" .
"#,
    );
    let header = graph_store(
        r#"@prefix ex: <http://example.test/> .
ex:a ex:rank "1" .
ex:b ex:rank "2" ; ex:other "trusted" .
"#,
    );
    for meta in ["_provider", "_trust", "_freshness"] {
        for filter in [format!("FILTER(?{meta}=\"2\")"), String::new()] {
            let q = format!(
                "PREFIX ex: <http://example.test/> SELECT ?s ?{meta} WHERE {{ GRAPH <{META_GRAPH}> {{ \
                 ?s ex:rank ?{meta} OPTIONAL {{ {{ SELECT ?s ?{meta} WHERE {{ ?s ex:other ?{meta} }} }} }} {filter} }} }} \
                 ORDER BY ?{meta}"
            );
            for k in [1, 10] {
                same_answer(&overwrite, &q, k);
            }
        }
        let q = format!(
            "PREFIX ex: <http://example.test/> SELECT ?s ?rank WHERE {{ GRAPH <{META_GRAPH}> {{ \
             ?s ex:rank ?rank OPTIONAL {{ {{ SELECT ?s ?{meta} WHERE {{ ?s ex:other ?{meta} }} }} }} }} }} \
             ORDER BY ?rank"
        );
        same_answer(&header, &q, 1);
    }
}

#[test]
fn declines_metadata_names_and_non_plain_right_sides_but_keeps_plain_ones() {
    let p = "PREFIX ex: <http://example.test/> SELECT *";
    for (why, body) in [
        (
            "right side is a subquery",
            "WHERE { ?s ex:rank ?rank OPTIONAL { { SELECT ?s ?x WHERE { ?s ex:other ?x } } } }",
        ),
        (
            "right side is VALUES",
            "WHERE { ?s ex:rank ?rank OPTIONAL { VALUES ?x { 1 } } }",
        ),
        (
            "metadata name in the left BGP",
            "WHERE { ?s ex:rank ?rank ; ex:p ?_trust OPTIONAL { ?s ex:other ?x } }",
        ),
        (
            "metadata name in a plain right side",
            "WHERE { ?s ex:rank ?rank OPTIONAL { ?s ex:other ?_provider } }",
        ),
    ] {
        assert!(
            !engages(&format!("{p} {body} ORDER BY ?rank LIMIT 3")),
            "{why}"
        );
    }
    // Positive control: an ordinary name that merely starts like one still engages.
    assert!(engages(&format!(
        "{p} WHERE {{ ?s ex:rank ?rank OPTIONAL {{ ?s ex:other ?_providers }} }} ORDER BY ?rank LIMIT 3"
    )));
}
