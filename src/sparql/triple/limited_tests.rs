//! Bounded work controls for explicitly limited generic leaves.
use crate::rdf::ingest_rdf;
use crate::store::Store;
use oxrdfio::RdfFormat;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn explicit_limit_one_avoids_grouping_the_entire_graph() {
    let mut store = Store::open_in_memory().unwrap();
    let mut data = String::new();
    for i in 0..10_000 {
        use std::fmt::Write;
        writeln!(
            data,
            "<http://example.org/s{i}> <http://example.org/p> {i} ."
        )
        .unwrap();
    }
    ingest_rdf(
        &mut store,
        data.as_bytes(),
        RdfFormat::Turtle,
        None,
        "2026-01-01T00:00:00Z",
        None,
        None,
    )
    .unwrap();
    // The public query entrypoint installs its own deadline progress handler.
    // Count the actual native evaluator directly so it cannot replace ours.
    let parsed = crate::sparql::sparql_parser()
        .parse_query("SELECT ?s WHERE { ?s ?p ?o } LIMIT 1")
        .unwrap();
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
    let result =
        crate::sparql::eval_parsed(&store, parsed, &crate::sparql::TemporalContext::default());
    store
        .conn
        .progress_handler(0, None::<fn() -> bool>)
        .unwrap();
    let result = result.unwrap();
    assert_eq!(result.rows().len(), 1, "positive result control");
    assert!(result.rows()[0].contains_key("s"));
    let instructions = ticks.load(Ordering::Relaxed);
    assert!(instructions > 0, "instrument must observe SQLite work");
    eprintln!("LIMIT_ONE_VM_INSTRUCTIONS={instructions}");
    assert!(
        instructions < 10_000,
        "LIMIT 1 performed {instructions} VM instructions"
    );
}

fn keys(result: &crate::sparql::QueryResult) -> Vec<Vec<(String, Vec<u8>)>> {
    let mut rows: Vec<_> = result
        .rows()
        .iter()
        .map(|row| {
            let mut fields: Vec<_> = row
                .iter()
                .map(|(name, value)| (name.clone(), value.term_key()))
                .collect();
            fields.sort();
            fields
        })
        .collect();
    rows.sort();
    rows
}

#[test]
fn limited_canonical_rows_keep_duplicate_numeric_and_graph_semantics() {
    let mut store = Store::open_in_memory().unwrap();
    let data = br#"@prefix e: <http://example.org/> .
        e:s e:p 1, 2 . e:t e:p e:s ."#;
    ingest_rdf(
        &mut store,
        data.as_slice(),
        RdfFormat::Turtle,
        None,
        "2026-01-01T00:00:00Z",
        None,
        None,
    )
    .unwrap();
    for (name, values) in [("g1", "1, 3"), ("g2", "1, 4")] {
        let graph = store
            .overlay_create(&format!("http://example.org/{name}"), 0)
            .unwrap();
        let ttl = format!("<http://example.org/s> <http://example.org/p> {values} .");
        crate::rdf::ingest_rdf_to_graph(
            &mut store,
            ttl.as_bytes(),
            RdfFormat::Turtle,
            None,
            "2026-01-01T00:00:00Z",
            None,
            None,
            graph,
        )
        .unwrap();
    }
    store
        .conn
        .execute_batch(
            "INSERT INTO transactions(id,timestamp) VALUES(9000,'2026-01-02');
        INSERT INTO facts(e,a,v,g,tx,valid_from,op)
        SELECT e,a,v,g,9000,'2026-01-02',1 FROM facts WHERE g=0;",
        )
        .unwrap();
    let s = store.lookup("http://example.org/s").unwrap().unwrap();
    let p = store.lookup("http://example.org/p").unwrap().unwrap();
    store
        .conn
        .execute(
            "INSERT INTO facts(e,a,v,g,tx,valid_from,op) VALUES(?1,?2,?3,0,9000,'2026-01-02',1)",
            rusqlite::params![s, p, crate::Value::Float(1.0).to_bytes()],
        )
        .unwrap();
    for (from, expected) in [
        ("", 4),
        (
            "FROM <http://example.org/g1> FROM <http://example.org/g2>",
            3,
        ),
        ("FROM <http://example.org/missing>", 0),
    ] {
        let text = format!("SELECT ?s ?p ?o {from} WHERE {{ ?s ?p ?o }}");
        let full = crate::sparql::query(&store, &text).unwrap();
        assert_eq!(full.rows().len(), expected, "dataset positive control");
        let limited = crate::sparql::query(&store, &format!("{text} LIMIT 100")).unwrap();
        assert_eq!(keys(&limited), keys(&full));
        assert_eq!(limited.variables(), full.variables());
    }
    let limited = crate::sparql::query(&store, "SELECT ?s ?p ?o WHERE {?s ?p ?o} LIMIT 3").unwrap();
    assert_eq!(
        limited.rows().len(),
        3,
        "physical duplicates do not consume LIMIT"
    );
    let mut unique = keys(&limited);
    unique.dedup();
    assert_eq!(unique.len(), 3, "each limited canonical triple is distinct");
}

#[test]
fn limited_attached_rows_preserve_subject_predicate_and_object_aliases() {
    let scratch = tempfile::tempdir().unwrap();
    let main = scratch.path().join("main.db");
    let source = scratch.path().join("source.db");
    let layer = scratch.path().join("layer.db");
    for (path, graph_iri) in [
        (&main, "http://example.org/main-g"),
        (&source, "http://example.org/layer-g"),
    ] {
        let mut store = Store::open(path.to_str().unwrap()).unwrap();
        // Main knows the layer graph's name without claiming its provider.
        store.intern("http://example.org/layer-g").unwrap();
        let graph = store.overlay_create(graph_iri, 0).unwrap();
        crate::rdf::ingest_rdf_to_graph(
            &mut store,
            b"<http://example.org/s> <http://example.org/p> <http://example.org/o> .".as_slice(),
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
    let count: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM term_alias", [], |row| row.get(0))
        .unwrap();
    assert!(
        count >= 4,
        "graph, subject, predicate and object aliases are real"
    );
    let predicates = store.lookup_all("http://example.org/p").unwrap();
    assert_eq!(
        predicates.len(),
        2,
        "both dictionaries contribute a predicate ID"
    );
    let physical: i64 = store
        .conn
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM {} WHERE a IN (?1,?2) AND op=1 AND valid_to IS NULL",
                store.facts_source()
            ),
            rusqlite::params![predicates[0], predicates[1]],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        physical, 2,
        "both providers really contribute the duplicate"
    );
    let text = "SELECT ?s ?p ?o FROM <http://example.org/main-g> FROM <http://example.org/layer-g> WHERE {?s ?p ?o}";
    let full = crate::sparql::query(&store, text).unwrap();
    let limited = crate::sparql::query(&store, &format!("{text} LIMIT 100")).unwrap();
    assert_eq!(full.rows().len(), 1);
    assert_eq!(keys(&limited), keys(&full));
}

#[test]
fn limited_scope_refuses_unsafe_shapes_and_restores_after_failure() {
    use super::super::{Bindings, TemporalContext, limited_scan};
    use spargebra::algebra::GraphPattern;
    let context = TemporalContext::default();
    for (inner, accepted) in [
        ("?s ?p ?o", true),
        ("?s ?p ?s", false),
        ("?s <http://example.org/p> ?o", false),
        ("?s ?p ?o . ?s ?q ?v", false),
        ("?s ?p ?o FILTER(?o=1)", false),
        ("GRAPH ?g {?s ?p ?o}", false),
    ] {
        let parsed = crate::sparql::sparql_parser()
            .parse_query(&format!("SELECT * WHERE {{{inner}}} LIMIT 1"))
            .unwrap();
        let spargebra::Query::Select { pattern, .. } = parsed else {
            panic!("SELECT")
        };
        let GraphPattern::Slice { inner, .. } = pattern else {
            panic!("Slice")
        };
        assert_eq!(
            limited_scan::eligible(&inner, &context, &Bindings::new()),
            accepted
        );
        let historical = TemporalContext {
            as_of_tx: Some(1),
            ..context.clone()
        };
        assert!(!limited_scan::eligible(
            &inner,
            &historical,
            &Bindings::new()
        ));
    }
    assert!(!limited_scan::active());
    let result = std::panic::catch_unwind(|| {
        limited_scan::scoped(true, || {
            assert!(limited_scan::active());
            limited_scan::scoped(false, || assert!(!limited_scan::active()));
            assert!(limited_scan::active());
            panic!("owned unwind control");
        })
    });
    assert!(result.is_err());
    assert!(!limited_scan::active());
}
