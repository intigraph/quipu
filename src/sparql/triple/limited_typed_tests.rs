//! Typed and fixed-predicate LIMIT work and semantic controls.
use super::{Arc, AtomicUsize, Ordering, RdfFormat, Store, ingest_rdf, keys};

#[test]
fn chaski_typed_limit_one_avoids_grouping_the_type_relation() {
    let mut store = Store::open_in_memory().unwrap();
    let mut data = String::new();
    for i in 0..10_000 {
        use std::fmt::Write;
        writeln!(
            data,
            "<http://example.org/s{i}> a <http://example.org/Class> ."
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
        .parse_query("SELECT ?focus WHERE { ?focus a ?type } LIMIT 1")
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
    assert!(result.rows()[0].contains_key("focus"));
    let instructions = ticks.load(Ordering::Relaxed);
    assert!(instructions > 0, "instrument must observe SQLite work");
    eprintln!("TYPED_LIMIT_ONE_VM_INSTRUCTIONS={instructions}");
    assert!(
        instructions < 10_000,
        "LIMIT 1 performed {instructions} VM instructions"
    );
}

#[test]
fn fixed_predicate_limits_preserve_full_canonical_multisets() {
    let mut store = Store::open_in_memory().unwrap();
    let data = br#"@prefix e: <http://example.org/> .
      @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
      e:s a e:Child, e:Other; e:p 1, 2 .
      e:t a e:Parent; e:p e:s .
      e:Child rdfs:subClassOf e:Parent ."#;
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
    for (name, data) in [
        ("g1", "e:s a e:Child; e:p 1, 3 ."),
        ("g2", "e:s a e:Child; e:p 1, 4 ."),
    ] {
        let g = store
            .overlay_create(&format!("http://example.org/{name}"), 0)
            .unwrap();
        let data = format!("@prefix e: <http://example.org/> . {data}");
        crate::rdf::ingest_rdf_to_graph(
            &mut store,
            data.as_bytes(),
            RdfFormat::Turtle,
            None,
            "2026-01-01T00:00:00Z",
            None,
            None,
            g,
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
    let entity = store.lookup("http://example.org/s").unwrap().unwrap();
    let predicate = store.lookup("http://example.org/p").unwrap().unwrap();
    store
        .conn
        .execute(
            "INSERT INTO facts(e,a,v,g,tx,valid_from,op)
      VALUES(?1,?2,?3,0,9000,'2026-01-02',1)",
            rusqlite::params![entity, predicate, crate::Value::Float(1.0).to_bytes()],
        )
        .unwrap();
    for dataset in [
        "",
        "FROM <http://example.org/g1> FROM <http://example.org/g2>",
        "FROM <http://example.org/missing>",
    ] {
        for pattern in ["?s a ?o", "?s <http://example.org/p> ?o"] {
            for projection in ["?s ?o", "?s"] {
                let text = format!("SELECT {projection} {dataset} WHERE {{{pattern}}}");
                let full = crate::sparql::query(&store, &text).unwrap();
                let limited = crate::sparql::query(&store, &format!("{text} LIMIT 100")).unwrap();
                assert_eq!(keys(&limited), keys(&full), "{text}");
                assert_eq!(limited.variables(), full.variables());
                let one = crate::sparql::query(&store, &format!("{text} LIMIT 1")).unwrap();
                assert_eq!(one.rows().len(), usize::from(!full.rows().is_empty()));
                for row in keys(&one) {
                    assert!(keys(&full).contains(&row));
                }
                assert!(
                    crate::sparql::query(&store, &format!("{text} LIMIT 0"))
                        .unwrap()
                        .rows()
                        .is_empty()
                );
            }
        }
    }
    let types = crate::sparql::query(&store, "SELECT ?s ?o WHERE {?s a ?o} LIMIT 100").unwrap();
    assert_eq!(
        types.rows().len(),
        3,
        "unbound type retains explicit type semantics"
    );
    let class = "SELECT ?s WHERE {?s a <http://example.org/Parent>}";
    let full = crate::sparql::query(&store, class).unwrap();
    assert_eq!(
        full.rows().len(),
        2,
        "bound class retains subclass expansion"
    );
    assert_eq!(
        keys(&full),
        keys(&crate::sparql::query(&store, &format!("{class} LIMIT 100")).unwrap())
    );
}

#[test]
fn fixed_predicate_scope_keeps_wrapped_and_seeded_queries_on_original_paths() {
    use crate::sparql::{Bindings, TemporalContext, limited_scan};
    use spargebra::algebra::GraphPattern;
    for query in [
        "SELECT DISTINCT ?s WHERE {?s a ?o} LIMIT 1",
        "SELECT ?s WHERE {?s a ?o} ORDER BY ?s LIMIT 1",
        "SELECT ?s WHERE {?s a ?o FILTER(isIRI(?o))} LIMIT 1",
        "SELECT ?s WHERE {GRAPH <http://example.org/g> {?s a ?o}} LIMIT 1",
        "SELECT ?s WHERE {?s a ?o . ?s <http://example.org/p> ?v} LIMIT 1",
    ] {
        let spargebra::Query::Select { pattern, .. } =
            crate::sparql::sparql_parser().parse_query(query).unwrap()
        else {
            panic!("SELECT")
        };
        let GraphPattern::Slice { inner, .. } = pattern else {
            panic!("Slice")
        };
        assert!(
            !limited_scan::eligible(&inner, &TemporalContext::default(), &Bindings::default()),
            "{query}"
        );
    }
    let spargebra::Query::Select { pattern, .. } = crate::sparql::sparql_parser()
        .parse_query("SELECT ?s WHERE {?s a ?o} LIMIT 1")
        .unwrap()
    else {
        panic!("SELECT")
    };
    let GraphPattern::Slice { inner, .. } = pattern else {
        panic!("Slice")
    };
    let mut seed = Bindings::default();
    seed.insert("s".into(), crate::Value::Ref(1));
    assert!(!limited_scan::eligible(
        &inner,
        &TemporalContext::default(),
        &seed
    ));
    let entailed = TemporalContext {
        entails_rdfs: true,
        ..TemporalContext::default()
    };
    assert!(!limited_scan::eligible(
        &inner,
        &entailed,
        &Bindings::default()
    ));
}
