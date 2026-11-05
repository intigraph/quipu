//! Exact grouped-reference controls for speculative retained relations.
use super::*;
use crate::{Value, rdf::ingest_rdf};
use oxrdf::{NamedNode, Variable};
use oxrdfio::RdfFormat;

fn pattern(repeated: bool) -> TriplePattern {
    TriplePattern {
        subject: Variable::new("s").unwrap().into(),
        predicate: NamedNode::new("http://example.org/p").unwrap().into(),
        object: Variable::new(if repeated { "s" } else { "o" })
            .unwrap()
            .into(),
    }
}

fn bag(rows: &[Bindings]) -> Vec<Vec<(String, Vec<u8>)>> {
    let mut rows: Vec<_> = rows
        .iter()
        .map(|row| {
            let mut fields: Vec<_> = row.iter().map(|(k, v)| (k.clone(), v.term_key())).collect();
            fields.sort();
            fields
        })
        .collect();
    rows.sort();
    rows
}

fn fixture() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    ingest_rdf(
        &mut store,
        b"@prefix e: <http://example.org/> . e:s e:p 1, 2 . e:t e:p e:t .".as_slice(),
        RdfFormat::Turtle,
        None,
        "2026-01-01T00:00:00Z",
        None,
        None,
    )
    .unwrap();
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
            rusqlite::params![s, p, Value::Float(1.0).to_bytes()],
        )
        .unwrap();
    store
}

#[test]
fn complete_filter_relation_keeps_numeric_duplicates_seeds_and_rejections() {
    let store = fixture();
    let ctx = TemporalContext::default();
    for repeated in [false, true] {
        let tp = pattern(repeated);
        let reference = super::super::triple::eval_triple_pattern_limited(
            &store,
            &tp,
            &Bindings::default(),
            &ctx,
            None,
        )
        .unwrap();
        let actual = collect(&store, &tp, &Bindings::default(), &ctx, 100).unwrap();
        assert!(!actual.is_empty(), "nonempty positive control");
        assert_eq!(bag(&actual), bag(&reference));
        assert_eq!(actual.len(), if repeated { 1 } else { 4 });
        assert!(
            collect(&store, &tp, &Bindings::default(), &ctx, 0)
                .unwrap()
                .is_empty()
        );
        let limited = collect(&store, &tp, &Bindings::default(), &ctx, 1).unwrap();
        assert_eq!(
            limited.len(),
            1,
            "rejected/duplicate rows never consume cap"
        );
        assert!(!active());
    }
    let mut seed = Bindings::default();
    seed.insert("o".into(), Value::Int(2));
    let tp = pattern(false);
    let reference =
        super::super::triple::eval_triple_pattern_limited(&store, &tp, &seed, &ctx, None).unwrap();
    assert_eq!(reference.len(), 1);
    assert_eq!(
        bag(&collect(&store, &tp, &seed, &ctx, 100).unwrap()),
        bag(&reference)
    );
}

#[test]
fn complete_filter_relation_keeps_dataset_and_graph_variable_semantics() {
    use crate::sparql::GraphScope;
    let mut store = fixture();
    let mut graphs = Vec::new();
    for name in ["g1", "g2"] {
        let g = store
            .graph_create(&format!("http://example.org/{name}"))
            .unwrap();
        graphs.push(g);
        crate::rdf::ingest_rdf_to_graph(
            &mut store,
            b"<http://example.org/s> <http://example.org/p> 1 .".as_slice(),
            RdfFormat::Turtle,
            None,
            "2026-01-01T00:00:00Z",
            None,
            None,
            g,
        )
        .unwrap();
    }
    let tp = pattern(false);
    for (graph, count) in [
        (GraphScope::Named(vec![graphs[0]]), 1),
        (GraphScope::Default(graphs.clone()), 1),
        (
            GraphScope::AnyNamed {
                var: "g".into(),
                restrict: Some(graphs),
            },
            2,
        ),
        (GraphScope::Named(vec![-1]), 0),
    ] {
        let ctx = TemporalContext {
            graph,
            ..Default::default()
        };
        let reference = super::super::triple::eval_triple_pattern_limited(
            &store,
            &tp,
            &Bindings::default(),
            &ctx,
            None,
        )
        .unwrap();
        assert_eq!(reference.len(), count);
        assert_eq!(
            bag(&collect(&store, &tp, &Bindings::default(), &ctx, 100).unwrap()),
            bag(&reference)
        );
    }
}

#[test]
fn filter_relation_scope_restores_on_nested_error_and_unwind() {
    assert!(!active());
    let result = std::panic::catch_unwind(|| {
        let _outer = Reset(ACTIVE.replace(true));
        {
            let _nested = Reset(ACTIVE.replace(true));
            assert!(active());
        }
        assert!(active());
        panic!("fixture unwind");
    });
    assert!(result.is_err());
    assert!(!active());
    let store = fixture();
    store.conn.execute_batch("DROP TABLE facts").unwrap();
    assert!(
        collect(
            &store,
            &pattern(false),
            &Bindings::default(),
            &Default::default(),
            100
        )
        .is_err()
    );
    assert!(!active());
}

#[test]
fn complete_filter_relation_keeps_attached_dictionary_aliases() {
    let scratch = tempfile::tempdir().unwrap();
    let main = scratch.path().join("main.db");
    let source = scratch.path().join("source.db");
    let layer = scratch.path().join("layer.db");
    for (path, graph_iri) in [
        (&main, "http://example.org/g1"),
        (&source, "http://example.org/g2"),
    ] {
        let mut store = Store::open(path.to_str().unwrap()).unwrap();
        store.intern("http://example.org/g2").unwrap();
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
    assert_eq!(store.lookup_all("http://example.org/p").unwrap().len(), 2);
    let graphs = ["http://example.org/g1", "http://example.org/g2"]
        .into_iter()
        .map(|g| store.lookup(g).unwrap().unwrap())
        .collect();
    let ctx = TemporalContext {
        graph: crate::sparql::GraphScope::Default(graphs),
        ..Default::default()
    };
    let tp = pattern(false);
    let reference = super::super::triple::eval_triple_pattern_limited(
        &store,
        &tp,
        &Bindings::default(),
        &ctx,
        None,
    )
    .unwrap();
    assert_eq!(reference.len(), 1);
    assert_eq!(
        bag(&collect(&store, &tp, &Bindings::default(), &ctx, 100).unwrap()),
        bag(&reference)
    );
}

#[test]
fn complete_filter_relation_reduces_sqlite_work_without_changing_the_bag() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let mut store = Store::open_in_memory().unwrap();
    let data = (0..6000)
        .map(|i| format!("<http://example.org/s{i}> <http://example.org/p> {i} .\n"))
        .collect::<String>();
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
    let tp = pattern(false);
    let ctx = TemporalContext::default();
    let reference = super::super::triple::eval_triple_pattern_limited(
        &store,
        &tp,
        &Bindings::default(),
        &ctx,
        None,
    )
    .unwrap();
    let grouped = ticks.swap(0, Ordering::Relaxed);
    let actual = collect(&store, &tp, &Bindings::default(), &ctx, 6001).unwrap();
    let lazy = ticks.load(Ordering::Relaxed);
    store
        .conn
        .progress_handler(0, None::<fn() -> bool>)
        .unwrap();
    assert_eq!(actual.len(), 6000);
    assert_eq!(bag(&actual), bag(&reference));
    assert!(lazy > 0, "positive instrument control");
    eprintln!("FILTER_RELATION_VM grouped={grouped} lazy={lazy}");
    assert!(
        lazy < grouped,
        "speculative relation must eliminate SQL grouping work"
    );
    assert!(!active());
}
