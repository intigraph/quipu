//! CONSTRUCT result dedupe: exact `Triple` identity, first-seen order, linear.

use crate::Store;
use crate::sparql::{QueryResult, query};

fn graph(sparql: &str) -> Vec<crate::sparql::Triple> {
    let store = Store::open_in_memory().unwrap();
    match query(&store, sparql).unwrap() {
        QueryResult::Graph(triples) => triples,
        other => panic!("expected a graph, got {other:?}"),
    }
}

#[test]
fn duplicates_collapse_in_first_seen_order() {
    let triples = graph(
        r#"CONSTRUCT { ?s <http://e/p> ?o } WHERE {
             VALUES (?s ?o) {
               (<http://e/b> "x") (<http://e/a> 1) (<http://e/b> "x")
               (<http://e/a> "1"^^<http://www.w3.org/2001/XMLSchema#integer>)
               (<http://e/a> "01"^^<http://www.w3.org/2001/XMLSchema#integer>)
               (<http://e/a> "x"@en) (<http://e/a> "x")
             } }"#,
    );
    let rendered: Vec<String> = triples
        .iter()
        .map(|t| format!("{} {:?}", t.subject, t.object))
        .collect();
    // 1 and "1"^^xsd:integer are one term; "01" and the tagged/plain strings
    // are distinct terms.
    assert_eq!(triples.len(), 5, "{rendered:#?}");
    assert_eq!(triples[0].subject, "http://e/b");
    assert_eq!(triples[1].subject, "http://e/a");
}

#[test]
fn large_construct_is_not_quadratic() {
    // 300 x 400 = 120k solutions, and the template states each triple twice,
    // so 240k candidates collapse to 120k. The Vec::contains dedupe was
    // quadratic here (a real 160k-triple CONSTRUCT took 18.7 s).
    let a: String = (0..300).map(|i| format!("<http://e/s{i}> ")).collect();
    let b: String = (0..400).map(|i| format!("{i} ")).collect();
    let started = std::time::Instant::now();
    let triples = graph(&format!(
        "CONSTRUCT {{ ?a <http://e/p> ?b . ?a <http://e/p> ?b }} \
         WHERE {{ VALUES ?a {{ {a} }} VALUES ?b {{ {b} }} }}"
    ));
    assert_eq!(triples.len(), 120_000);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "CONSTRUCT took {:?}",
        started.elapsed()
    );
}
