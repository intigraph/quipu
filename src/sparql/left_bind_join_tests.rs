//! OPTIONAL probed per left row (aegis-xacr) against the bottom-up keyed probe.
//!
//! Two kinds, as for the Join bind join: the probe must ENGAGE for the shapes it
//! is meant for (a test that only compares answers passes whether or not it
//! engaged), and every engaged shape must give the bottom-up rows, in the same
//! order, with the same header.

use oxrdfio::RdfFormat;
use spargebra::algebra::{Expression, GraphPattern};
use spargebra::{Query, SparqlParser};

use super::bind_join::{LEFT_BIND_MAX_ROWS, try_subject_bind_left_join};
use super::left_join::left_join;
use super::pattern::eval_pattern_seeded;
use super::{Bindings, GraphScope, TemporalContext};
use crate::rdf::{ingest_rdf, ingest_rdf_to_graph};
use crate::store::Store;

const G: &str = "http://example.org/g";

fn store() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    let root = r#"@prefix ex: <http://example.org/> .
ex:alice ex:name "Alice" ; ex:age 30 ; ex:age 31 .
ex:bob ex:name "Bob" ; ex:age 25 .
ex:carol ex:name "Carol" .
"#;
    ingest_rdf(
        &mut store,
        root.as_bytes(),
        RdfFormat::Turtle,
        None,
        "2026-10-21T00:00:00Z",
        None,
        None,
    )
    .unwrap();
    let g = store.graph_create(G).unwrap();
    let named = r#"@prefix ex: <http://example.org/> .
ex:alice ex:role "admin" ; ex:team ex:red .
ex:bob ex:role "user" .
"#;
    ingest_rdf_to_graph(
        &mut store,
        named.as_bytes(),
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

fn where_clause(sparql: &str) -> GraphPattern {
    let Query::Select { pattern, .. } = SparqlParser::new().parse_query(sparql).unwrap() else {
        panic!("not a SELECT");
    };
    let mut p = pattern;
    loop {
        p = match p {
            GraphPattern::Project { inner, .. } | GraphPattern::Distinct { inner } => *inner,
            GraphPattern::Graph { inner, .. } => *inner,
            other => return other,
        };
    }
}

fn operands(p: &GraphPattern) -> (&GraphPattern, &GraphPattern, Option<&Expression>) {
    match p {
        GraphPattern::LeftJoin {
            left,
            right,
            expression,
        } => (left, right, expression.as_ref()),
        other => panic!("expected a LeftJoin, got {other:?}"),
    }
}

/// Rows in ORDER, each row's bindings sorted, so the comparison is exact.
fn ordered(rows: &[Bindings]) -> Vec<String> {
    rows.iter()
        .map(|r| {
            let mut kv: Vec<String> = r.iter().map(|(k, v)| format!("{k}={v:?}")).collect();
            kv.sort();
            kv.join(",")
        })
        .collect()
}

/// The per-row probe and the bottom-up keyed probe for the same LeftJoin node.
fn both_ways(
    store: &Store,
    sparql: &str,
    ctx: &TemporalContext,
) -> (Vec<String>, Vec<String>, usize) {
    let p = where_clause(sparql);
    let (left, right, expression) = operands(&p);
    let seed = Bindings::new();
    let (left_rows, left_vars) = eval_pattern_seeded(store, left, ctx, &seed).unwrap();
    let (probed, header) =
        try_subject_bind_left_join(store, right, expression, ctx, &seed, &left_rows, &left_vars)
            .unwrap()
            .expect("the per-row OPTIONAL probe must engage for this shape");
    let (right_rows, right_vars) = eval_pattern_seeded(store, right, ctx, &seed).unwrap();
    let bottom_up = left_join(store, ctx, &left_rows, &right_rows, expression).unwrap();
    let mut expected_header = left_vars.clone();
    for v in right_vars {
        if !expected_header.contains(&v) {
            expected_header.push(v);
        }
    }
    assert_eq!(
        header, expected_header,
        "header must match the LeftJoin arm's"
    );
    (ordered(&probed), ordered(&bottom_up), probed.len())
}

#[test]
fn engages_and_keeps_matched_multi_valued_and_unmatched_rows() {
    let (probed, bottom_up, n) = both_ways(
        &store(),
        "PREFIX ex: <http://example.org/> SELECT * WHERE { ?s ex:name ?name OPTIONAL { ?s ex:age ?age } }",
        &TemporalContext::default(),
    );
    assert_eq!(probed, bottom_up);
    // alice twice (two ages), bob once, carol kept without ?age.
    assert_eq!(n, 4);
}

#[test]
fn the_optional_expression_is_applied_exactly_as_bottom_up() {
    let (probed, bottom_up, n) = both_ways(
        &store(),
        "PREFIX ex: <http://example.org/> SELECT * WHERE { ?s ex:name ?name OPTIONAL { ?s ex:age ?age FILTER(?age > 30) } }",
        &TemporalContext::default(),
    );
    assert_eq!(probed, bottom_up);
    // alice keeps only 31; bob's 25 fails the filter, so bob is kept unmatched.
    assert_eq!(n, 3);
}

#[test]
fn duplicate_left_rows_keep_their_multiplicity() {
    let (probed, bottom_up, n) = both_ways(
        &store(),
        "PREFIX ex: <http://example.org/> SELECT * WHERE { VALUES ?s { ex:bob ex:bob ex:carol } OPTIONAL { ?s ex:age ?age } }",
        &TemporalContext::default(),
    );
    assert_eq!(probed, bottom_up);
    assert_eq!(n, 3);
}

#[test]
fn named_graph_optional_engages_and_matches() {
    let store = store();
    let graph = store.lookup(G).unwrap().unwrap();
    let ctx = TemporalContext {
        graph: GraphScope::Named(vec![graph]),
        ..TemporalContext::default()
    };
    let (probed, bottom_up, n) = both_ways(
        &store,
        "PREFIX ex: <http://example.org/> SELECT * WHERE { GRAPH <http://example.org/g> { ?s ex:role ?role OPTIONAL { ?s ex:team ?team } } }",
        &ctx,
    );
    assert_eq!(probed, bottom_up);
    assert_eq!(n, 2);
}

fn declines(store: &Store, sparql: &str) {
    let p = where_clause(sparql);
    let (left, right, expression) = operands(&p);
    let ctx = TemporalContext::default();
    let seed = Bindings::new();
    let (left_rows, left_vars) = eval_pattern_seeded(store, left, &ctx, &seed).unwrap();
    assert!(
        try_subject_bind_left_join(
            store, right, expression, &ctx, &seed, &left_rows, &left_vars
        )
        .unwrap()
        .is_none(),
        "must keep bottom-up evaluation: {sparql}"
    );
}

#[test]
fn declines_a_right_side_that_could_observe_substitution() {
    let store = store();
    // A nested OPTIONAL in the right side keeps bottom-up scoping.
    declines(
        &store,
        "PREFIX ex: <http://example.org/> SELECT * WHERE { ?s ex:name ?n OPTIONAL { ?s ex:age ?a OPTIONAL { ?s ex:role ?r } } }",
    );
    // A subject bound to a literal is never substituted.
    declines(
        &store,
        r#"PREFIX ex: <http://example.org/> SELECT * WHERE { VALUES ?s { "alice" } OPTIONAL { ?s ex:age ?a } }"#,
    );
}

#[test]
fn declines_above_the_left_row_ceiling() {
    let store = store();
    let p = where_clause(
        "PREFIX ex: <http://example.org/> SELECT * WHERE { ?s ex:name ?n OPTIONAL { ?s ex:age ?a } }",
    );
    let (left, right, expression) = operands(&p);
    let ctx = TemporalContext::default();
    let seed = Bindings::new();
    let (rows, vars) = eval_pattern_seeded(&store, left, &ctx, &seed).unwrap();
    let wide = vec![rows[0].clone(); LEFT_BIND_MAX_ROWS + 1];
    assert!(
        try_subject_bind_left_join(&store, right, expression, &ctx, &seed, &wide, &vars)
            .unwrap()
            .is_none()
    );
    let at_cap = vec![rows[0].clone(); LEFT_BIND_MAX_ROWS];
    assert!(
        try_subject_bind_left_join(&store, right, expression, &ctx, &seed, &at_cap, &vars)
            .unwrap()
            .is_some()
    );
}
