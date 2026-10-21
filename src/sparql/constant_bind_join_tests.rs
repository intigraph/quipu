//! Subject bind join through `BIND(<constant> AS ?v)` (aegis-xacr).
//!
//! A dependency panel unions one branch per edge predicate and tags each with a
//! constant: `{ ?item q:blockedOn ?t . BIND("blocks" AS ?relation) } UNION ...`.
//! Each branch is an Extend, so the bind join declined and every branch scanned
//! its predicate across the whole graph. A constant reads no binding, so the
//! seeded evaluation equals the join, except where a left row already binds
//! the BIND variable: seeded Extend overwrites it, the join drops the row.

use oxrdfio::RdfFormat;
use spargebra::algebra::GraphPattern;
use spargebra::{Query, SparqlParser};

use super::bind_join::try_subject_bind_join;
use super::pattern::eval_pattern_seeded;
use super::pattern_util::join_rows;
use super::{Bindings, TemporalContext, query_temporal};
use crate::rdf::ingest_rdf;
use crate::store::Store;

fn store() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    let data = r#"@prefix ex: <http://example.org/> .
ex:alice ex:name "Alice" ; ex:age 30 .
ex:bob ex:name "Bob" ; ex:age 25 .
ex:carol ex:name "Carol" .
ex:dave ex:name "Dave" ; ex:age 40 .
"#;
    ingest_rdf(
        &mut store,
        data.as_bytes(),
        RdfFormat::Turtle,
        None,
        "2026-10-21T00:00:00Z",
        None,
        None,
    )
    .unwrap();
    store
}

fn join_operands(sparql: &str) -> (GraphPattern, GraphPattern) {
    let Query::Select { pattern, .. } = SparqlParser::new().parse_query(sparql).unwrap() else {
        panic!("not a SELECT");
    };
    let mut p = pattern;
    loop {
        p = match p {
            GraphPattern::Project { inner, .. } | GraphPattern::Distinct { inner } => *inner,
            GraphPattern::Join { left, right } => return (*left, *right),
            other => panic!("expected a Join, got {other:?}"),
        };
    }
}

fn bag(rows: &[Bindings]) -> Vec<String> {
    let mut v: Vec<String> = rows
        .iter()
        .map(|r| {
            let mut kv: Vec<String> = r.iter().map(|(k, v)| format!("{k}={v:?}")).collect();
            kv.sort();
            kv.join(",")
        })
        .collect();
    v.sort();
    v
}

/// The bind join's rows and header, or None when it declines.
fn bind(store: &Store, sparql: &str) -> Option<(Vec<String>, Vec<String>, Vec<String>)> {
    let (left, right) = join_operands(sparql);
    let ctx = TemporalContext::default();
    let seed = Bindings::new();
    let (left_rows, left_vars) = eval_pattern_seeded(store, &left, &ctx, &seed).unwrap();
    let (bound, header) =
        try_subject_bind_join(store, &right, &ctx, &seed, &left_rows, &left_vars).unwrap()?;
    let (right_rows, right_vars) = eval_pattern_seeded(store, &right, &ctx, &seed).unwrap();
    let hash = join_rows(&left_rows, &right_rows, &ctx).unwrap();
    assert_eq!(header, right_vars, "header must match the hash-join arm's");
    Some((bag(&bound), bag(&hash), header))
}

const EDGES: &str = r#"PREFIX ex: <http://example.org/>
    SELECT * WHERE {
      VALUES ?s { ex:alice ex:bob ex:carol }
      { ?s ex:age ?o BIND("age" AS ?rel) } UNION { ?s ex:name ?o BIND("name" AS ?rel) }
    }"#;

#[test]
fn engages_through_constant_binds_in_a_union_and_matches_the_hash_join() {
    let (bound, hash, header) = bind(&store(), EDGES).expect("must engage for this shape");
    assert_eq!(bound, hash);
    // alice age+name, bob age+name, carol name only; dave is not selected.
    assert_eq!(bound.len(), 5);
    assert!(header.contains(&"rel".to_string()));
}

#[test]
fn the_full_query_returns_the_tagged_edges() {
    let rows = query_temporal(&store(), EDGES, &TemporalContext::default()).unwrap();
    assert_eq!(rows.rows().len(), 5);
}

#[test]
fn declines_when_a_left_row_already_binds_the_constant_variable() {
    let store = store();
    let sparql = r#"PREFIX ex: <http://example.org/>
        SELECT * WHERE {
          VALUES (?s ?rel) { (ex:alice "name") (ex:bob "age") }
          { ?s ex:age ?o BIND("age" AS ?rel) }
        }"#;
    assert!(
        bind(&store, sparql).is_none(),
        "seeded Extend would overwrite ?rel"
    );
    // Bottom-up, alice's "name" conflicts with the branch's "age" and is
    // dropped; bob's "age" agrees and joins.
    let rows = query_temporal(&store, sparql, &TemporalContext::default()).unwrap();
    assert_eq!(rows.rows().len(), 1);
}

#[test]
fn declines_a_bind_that_reads_a_binding() {
    let store = store();
    let sparql = r#"PREFIX ex: <http://example.org/>
        SELECT * WHERE {
          VALUES ?s { ex:alice ex:bob }
          { ?s ex:age ?o BIND(STR(?s) AS ?rel) }
        }"#;
    assert!(bind(&store, sparql).is_none());
}
