//! Work and answer controls for named-graph presence subqueries.
use crate::{Store, rdf::ingest_rdf_to_graph};
use oxrdfio::RdfFormat;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn named_graph_presence_preflight_work() {
    let main = std::env::var("QUIPU_LIMIT_MAIN_GRAPH")
        .unwrap_or_else(|_| "http://example.org/main".into());
    let eph = std::env::var("QUIPU_LIMIT_EPH_GRAPH")
        .unwrap_or_else(|_| "http://example.org/ephemeral".into());
    let text = match std::env::var("QUIPU_LIMIT_QUERY") {
        Ok(path) => std::fs::read_to_string(path).unwrap(),
        Err(_) => format!(
            "SELECT ?probe WHERE {{ {{ BIND(\"complete\" AS ?probe) }} UNION {{ SELECT (\"main_legacy\" AS ?probe) WHERE {{ GRAPH <{main}> {{ ?s <http://example.org/revision> ?r }} }} LIMIT 1 }} UNION {{ SELECT (\"eph_legacy\" AS ?probe) WHERE {{ GRAPH <{eph}> {{ ?s <http://example.org/revision> ?r }} }} LIMIT 1 }} UNION {{ SELECT (\"eph_present\" AS ?probe) WHERE {{ GRAPH <{eph}> {{ ?s ?p ?o }} }} LIMIT 1 }} }} LIMIT 5"
        ),
    };
    let mut store = Store::open_in_memory().unwrap();
    store.overlay_create(&main, 0).unwrap();
    let graph = store.overlay_create(&eph, 0).unwrap();
    let data: String = (0..3000)
        .map(|i| format!("<http://example.org/s{i}> <http://example.org/p> {i} .\n"))
        .collect();
    ingest_rdf_to_graph(
        &mut store,
        data.as_bytes(),
        RdfFormat::Turtle,
        None,
        "2026-01-01T00:00:00Z",
        None,
        None,
        graph,
    )
    .unwrap();
    let parsed = crate::sparql::sparql_parser().parse_query(&text).unwrap();
    let ticks = Arc::new(AtomicUsize::new(0));
    let seen = ticks.clone();
    store
        .conn
        .progress_handler(
            1,
            Some(move || {
                seen.fetch_add(1, Ordering::Relaxed);
                false
            }),
        )
        .unwrap();
    super::super::limited_scan::take_entries();
    let started = std::time::Instant::now();
    let result = crate::sparql::eval_parsed(&store, parsed, &Default::default()).unwrap();
    let elapsed = started.elapsed();
    let entries = super::super::limited_scan::take_entries();
    store
        .conn
        .progress_handler(0, None::<fn() -> bool>)
        .unwrap();
    let mut markers: Vec<_> = result
        .rows()
        .iter()
        .map(|row| row["probe"].clone())
        .collect();
    markers.sort_by_key(|v| format!("{v:?}"));
    assert_eq!(
        markers,
        vec![
            crate::Value::Str("complete".into()),
            crate::Value::Str("eph_present".into())
        ]
    );
    assert_eq!(result.variables(), &["probe"]);
    eprintln!(
        "NAMED_PREFLIGHT entries={entries} vm_steps={} elapsed_ms={:.3} rows={}",
        ticks.load(Ordering::Relaxed),
        elapsed.as_secs_f64() * 1000.0,
        result.rows().len()
    );
    assert!(
        entries > 0,
        "fixed GRAPH presence must enter the limited path"
    );
}

#[test]
fn named_presence_eligibility_is_narrow() {
    use crate::sparql::{Bindings, TemporalContext, limited_scan};
    use spargebra::{Query, algebra::GraphPattern};
    let ctx = TemporalContext::default();
    for (query, expected) in [
        (
            "SELECT ('yes' AS ?marker) WHERE {GRAPH <http://example.org/g> {?s ?p ?o}} LIMIT 1",
            true,
        ),
        (
            "SELECT ('yes' AS ?marker) WHERE {GRAPH <http://example.org/g> {?s <http://example.org/p> ?o}} LIMIT 1",
            true,
        ),
        (
            "SELECT ?s WHERE {GRAPH <http://example.org/g> {?s ?p ?o}} LIMIT 1",
            false,
        ),
        (
            "SELECT ('yes' AS ?marker) WHERE {GRAPH ?g {?s ?p ?o}} LIMIT 1",
            false,
        ),
        (
            "SELECT ('yes' AS ?marker) WHERE {GRAPH <http://example.org/g> {?s ?p ?o FILTER(?o=1)}} LIMIT 1",
            false,
        ),
        (
            "SELECT DISTINCT ('yes' AS ?marker) WHERE {GRAPH <http://example.org/g> {?s ?p ?o}} LIMIT 1",
            false,
        ),
        (
            "SELECT ('yes' AS ?marker) WHERE {GRAPH <http://example.org/g> {?s ?p ?o}} ORDER BY ?s LIMIT 1",
            false,
        ),
        (
            "SELECT ('yes' AS ?marker) WHERE {GRAPH <http://example.org/g> {?s a ?o}} LIMIT 1",
            false,
        ),
        (
            "SELECT ('yes' AS ?marker) WHERE {GRAPH <http://example.org/g> {?s ?p ?s}} LIMIT 1",
            false,
        ),
        (
            "SELECT ('yes' AS ?marker) WHERE {GRAPH <http://example.org/g> {?s ?p ?o . ?s ?q ?v}} LIMIT 1",
            false,
        ),
        (
            "SELECT (RAND() AS ?marker) WHERE {GRAPH <http://example.org/g> {?s ?p ?o}} LIMIT 1",
            false,
        ),
    ] {
        let Query::Select {
            pattern: GraphPattern::Slice { inner, .. },
            ..
        } = crate::sparql::sparql_parser().parse_query(query).unwrap()
        else {
            panic!("slice")
        };
        assert_eq!(
            limited_scan::named_presence(&inner, &ctx, &Bindings::new()),
            expected,
            "{query}"
        );
        let historical = TemporalContext {
            as_of_tx: Some(1),
            ..ctx.clone()
        };
        assert!(!limited_scan::named_presence(
            &inner,
            &historical,
            &Bindings::new()
        ));
        let historical = TemporalContext {
            valid_at: Some("2026-01-01".into()),
            ..ctx.clone()
        };
        assert!(!limited_scan::named_presence(
            &inner,
            &historical,
            &Bindings::new()
        ));
        let inferred = TemporalContext {
            entails_rdfs: true,
            ..ctx.clone()
        };
        assert!(!limited_scan::named_presence(
            &inner,
            &inferred,
            &Bindings::new()
        ));
        let bound = Bindings::from([("outside".into(), crate::Value::Int(1))]);
        assert!(!limited_scan::named_presence(&inner, &ctx, &bound));
    }
}

#[test]
fn named_presence_keeps_graph_and_marker_answers() {
    let mut store = Store::open_in_memory().unwrap();
    let graph = store.overlay_create("http://example.org/g", 0).unwrap();
    let ttl = b"<http://example.org/s> <http://example.org/p> 1, 1.0, \"one\"@en .";
    ingest_rdf_to_graph(
        &mut store,
        ttl.as_slice(),
        RdfFormat::Turtle,
        None,
        "2026-01-01T00:00:00Z",
        None,
        None,
        graph,
    )
    .unwrap();
    store.overlay_create("http://example.org/empty", 0).unwrap();
    for (scope, predicate, expected) in [
        ("g", "?p", 1),
        ("g", "<http://example.org/p>", 1),
        ("g", "<http://example.org/missing>", 0),
        ("empty", "?p", 0),
        ("absent", "?p", 0),
    ] {
        let q = format!(
            "SELECT ('present' AS ?marker) WHERE {{ GRAPH <http://example.org/{scope}> {{ ?s {predicate} ?o }} }}"
        );
        let full = crate::sparql::query(&store, &q).unwrap();
        let limited = crate::sparql::query(&store, &format!("{q} LIMIT 1")).unwrap();
        assert_eq!(limited.variables(), full.variables());
        assert_eq!(limited.rows().len(), expected);
        assert_eq!(limited.rows(), &full.rows()[..expected]);
    }
    let excluded = "SELECT ('present' AS ?marker) FROM NAMED <http://example.org/empty> WHERE {GRAPH <http://example.org/g> {?s ?p ?o}} LIMIT 1";
    assert!(
        crate::sparql::query(&store, excluded)
            .unwrap()
            .rows()
            .is_empty()
    );
}

#[test]
fn named_presence_keeps_composed_and_attached_only_graphs() {
    let scratch = tempfile::tempdir().unwrap();
    let main = scratch.path().join("main.db");
    let source = scratch.path().join("source.db");
    let layer = scratch.path().join("layer.db");
    for (path, graph_iri, data) in [
        (
            &main,
            "http://example.org/main",
            "<http://example.org/shared> <http://example.org/p> 1 .",
        ),
        (
            &source,
            "http://example.org/layer",
            "<http://example.org/shared> <http://example.org/p> 1 . <http://example.org/only-layer> <http://example.org/p> 2 .",
        ),
    ] {
        let mut store = Store::open(path.to_str().unwrap()).unwrap();
        store.intern("http://example.org/layer").unwrap();
        let graph = store.overlay_create(graph_iri, 0).unwrap();
        ingest_rdf_to_graph(
            &mut store,
            data.as_bytes(),
            RdfFormat::Turtle,
            None,
            "2026-01-01T00:00:00Z",
            None,
            None,
            graph,
        )
        .unwrap();
    }
    crate::store::respace::respace_file(&source, &layer, 31).unwrap();
    let store = Store::open_with_attachments(
        main.to_str().unwrap(),
        &[crate::store::attach::Attachment::read_only(
            "layer",
            layer.to_str().unwrap(),
        )],
    )
    .unwrap();
    let aliases: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM term_alias", [], |r| r.get(0))
        .unwrap();
    assert!(aliases >= 3, "real graph, subject and predicate aliases");
    for graph in ["main", "layer"] {
        let text = format!(
            "SELECT ('present' AS ?marker) WHERE {{GRAPH <http://example.org/{graph}> {{?s ?p ?o}}}}"
        );
        let full = crate::sparql::query(&store, &text).unwrap();
        let limited = crate::sparql::query(&store, &format!("{text} LIMIT 1")).unwrap();
        assert_eq!(limited.rows(), &full.rows()[..1]);
        assert_eq!(limited.variables(), full.variables());
    }
    let text = "SELECT ('present' AS ?marker) WHERE {GRAPH <http://example.org/layer> {?s <http://example.org/p> ?o}}";
    let full = crate::sparql::query(&store, text).unwrap();
    assert_eq!(
        full.rows().len(),
        2,
        "attached-only subject remains visible"
    );
    let limited = crate::sparql::query(&store, &format!("{text} OFFSET 1 LIMIT 1")).unwrap();
    assert_eq!(limited.rows(), &full.rows()[1..2]);
}
