use super::*;

#[test]
fn dense_guard_and_subject_stars_match_original_across_providers() {
    for composed in [false, true] {
        let (store, _dir) = fixture_density(composed, true);
        let guard =
            include_str!("owner_guard.sparql").replacen("ASK {", "SELECT ?s ?bad WHERE {", 1);
        let star = "SELECT ?status ?s WHERE { GRAPH <http://example.org/board> { ?s <https://schema.org/agent> \"target\" ; a <https://schema.org/Action> ; <http://example.org/status> ?status } }";
        for text in [&guard, star] {
            let old = reference(|| query(&store, text).unwrap());
            let new = query(&store, text).unwrap();
            assert_eq!(new.variables(), old.variables(), "composed={composed}");
            assert_eq!(bag(new.rows()), bag(old.rows()), "composed={composed}");
            assert!(!new.rows().is_empty(), "positive selected-row control");
        }
    }
}

#[test]
fn dense_subject_batch_is_selected_and_unsupported_scopes_decline() {
    use spargebra::term::{NamedNode, NamedNodePattern, TermPattern, TriplePattern, Variable};
    let patterns: Vec<_> = [
        (
            "https://schema.org/agent",
            TermPattern::Literal(oxrdf::Literal::new_simple_literal("target")),
        ),
        (
            crate::namespace::RDF_TYPE,
            TermPattern::NamedNode(NamedNode::new("https://schema.org/Action").unwrap()),
        ),
    ]
    .into_iter()
    .map(|(predicate, object)| TriplePattern {
        subject: TermPattern::Variable(Variable::new("s").unwrap()),
        predicate: NamedNodePattern::NamedNode(NamedNode::new(predicate).unwrap()),
        object,
    })
    .collect();
    for composed in [false, true] {
        let (store, _dir) = fixture_density(composed, true);
        let ctx = crate::sparql::TemporalContext {
            graph: crate::sparql::GraphScope::Named(
                store.lookup_all("http://example.org/board").unwrap(),
            ),
            ..Default::default()
        };
        let found =
            crate::sparql::batch_star::try_eval(&store, &patterns, &ctx, &Bindings::new()).unwrap();
        assert!(
            found.is_some(),
            "must exercise batched plan, composed={composed}"
        );
        assert!(found.unwrap().0.len() > 128);
        let mut unsupported = ctx.clone();
        unsupported.graph = crate::sparql::GraphScope::default();
        assert!(
            crate::sparql::batch_star::try_eval(&store, &patterns, &unsupported, &Bindings::new())
                .unwrap()
                .is_none()
        );
        unsupported = ctx.clone();
        unsupported.as_of_tx = Some(1);
        assert!(
            crate::sparql::batch_star::try_eval(&store, &patterns, &unsupported, &Bindings::new())
                .unwrap()
                .is_none()
        );
        unsupported = ctx.clone();
        unsupported.row_limit = Some(1);
        assert!(
            crate::sparql::batch_star::try_eval(&store, &patterns, &unsupported, &Bindings::new())
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn speculative_batch_does_not_surface_an_eliminated_malformed_property() {
    for composed in [false, true] {
        let (mut store, _dir) = fixture_density(composed, true);
        let entity = store
            .lookup("http://example.org/0/wrong_type")
            .unwrap()
            .unwrap();
        let graph = store.lookup("http://example.org/board").unwrap().unwrap();
        let property = store.intern("http://example.org/status").unwrap();
        store
            .transact_to_graph(
                &[crate::Datum {
                    entity,
                    attribute: property,
                    value: Value::Str("placeholder".into()),
                    valid_from: "2026-01-01T00:00:00Z".into(),
                    valid_to: None,
                    op: crate::types::Op::Assert,
                }],
                "2026-01-01T00:00:00Z",
                None,
                None,
                graph,
            )
            .unwrap();
        store
            .conn
            .execute(
                "UPDATE facts SET v=x'ff' WHERE e=?1 AND a=?2",
                [entity, property],
            )
            .unwrap();
        let text = "SELECT ?s ?status WHERE { GRAPH <http://example.org/board> { ?s <https://schema.org/agent> <http://example.org/principal/target> ; a <https://schema.org/Action> ; <http://example.org/status> ?status } }";
        let old = reference(|| query(&store, text).unwrap());
        let new = query(&store, text).unwrap();
        assert!(!old.rows().is_empty());
        assert_eq!(new.variables(), old.variables());
        assert_eq!(bag(new.rows()), bag(old.rows()), "composed={composed}");
    }
}

/// Explicit comparison with the preceding subject-by-subject planner.
#[test]
#[ignore = "12,001-action dense-owner timing demonstration; run explicitly"]
fn dense_owner_guard_demo() {
    println!("DENSE_GUARD_DEMO fixture_start=true");
    let mut store = Store::open_in_memory().unwrap();
    store.set_read_model_enabled(false);
    for (gi, graph) in [
        "http://example.org/board",
        "http://example.org/board#ephemeral",
    ]
    .iter()
    .enumerate()
    {
        let mut data = String::new();
        for i in 0..6_000 {
            let owner = if i >= 4_000 {
                "\"unrelated\""
            } else if i % 2 == 0 {
                "\"target\""
            } else {
                "<http://example.org/principal/target>"
            };
            let subject = format!("<http://example.org/dense-demo/{gi}/{i}>");
            data.push_str(&format!("{subject} <https://schema.org/agent> {owner} .\n{subject} <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <https://schema.org/Action> .\n{subject} <http://example.org/status> \"open\" .\n"));
        }
        put(&mut store, graph, &data);
    }
    put(
        &mut store,
        "http://example.org/board",
        "<http://example.org/dense-demo/invalid> <https://schema.org/agent> \"target\" .\n<http://example.org/dense-demo/invalid> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <https://schema.org/Action> .\n<http://example.org/dense-demo/invalid> <http://example.org/status> \"invalid\" .\n",
    );
    println!("DENSE_GUARD_DEMO fixture_ready=true");
    for optimized in [false, true, true, false] {
        let start = std::time::Instant::now();
        let result = if optimized {
            query(&store, include_str!("owner_guard.sparql")).unwrap()
        } else {
            v1_reference(|| query(&store, include_str!("owner_guard.sparql")).unwrap())
        };
        assert!(matches!(result, QueryResult::Ask(true)));
        println!(
            "DENSE_GUARD_DEMO mode={} positive_guard=true elapsed_ms={:.3}",
            if optimized {
                "batched"
            } else {
                "preceding_per_subject"
            },
            start.elapsed().as_secs_f64() * 1000.0
        );
    }
}

#[test]
fn oversized_speculative_relation_falls_back_without_losing_rows() {
    let (mut store, _dir) = fixture_density(false, true);
    let graph = store.lookup("http://example.org/board").unwrap().unwrap();
    let property = store.intern("http://example.org/wide").unwrap();
    let mut facts = Vec::new();
    for i in (0..320).step_by(2) {
        facts.push(crate::Datum {
            entity: store
                .lookup(&format!("http://example.org/0/dense/{i}"))
                .unwrap()
                .unwrap(),
            attribute: property,
            value: Value::Str("x".repeat(32 * 1024)),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
            op: crate::types::Op::Assert,
        });
    }
    store
        .transact_to_graph(&facts, "2026-01-01T00:00:00Z", None, None, graph)
        .unwrap();
    let text = "SELECT ?s ?wide WHERE { GRAPH <http://example.org/board> { ?s <https://schema.org/agent> \"target\" ; a <https://schema.org/Action> ; <http://example.org/wide> ?wide } }";
    let Query::Select { mut pattern, .. } = SparqlParser::new().parse_query(text).unwrap() else {
        panic!()
    };
    loop {
        pattern = match pattern {
            GraphPattern::Project { inner, .. } | GraphPattern::Graph { inner, .. } => *inner,
            GraphPattern::Bgp { patterns } => {
                let ctx = crate::sparql::TemporalContext {
                    graph: crate::sparql::GraphScope::Named(vec![graph]),
                    ..Default::default()
                };
                assert!(
                    crate::sparql::batch_star::try_eval(&store, &patterns, &ctx, &Bindings::new())
                        .unwrap()
                        .is_none(),
                    "large literals must decline before retaining the complete speculative relation"
                );
                break;
            }
            _ => panic!("expected a single named star"),
        };
    }
    let old = reference(|| query(&store, text).unwrap());
    let new = query(&store, text).unwrap();
    assert_eq!(old.rows().len(), 160);
    assert_eq!(new.variables(), old.variables());
    assert_eq!(bag(new.rows()), bag(old.rows()));
}

#[test]
fn dense_guard_preserves_a_shared_non_subject_binding() {
    let (mut store, _dir) = fixture_density(false, true);
    put(
        &mut store,
        "http://example.org/board",
        "<http://example.org/0/dense/0> <http://example.org/status> \"target\" .\n",
    );
    let text = "SELECT ?s ?bad WHERE { GRAPH <http://example.org/board> { { ?s <http://example.org/status> ?bad . FILTER(?bad = \"target\") } ?s <https://schema.org/agent> ?bad } }";
    let old = reference(|| query(&store, text).unwrap());
    let new = query(&store, text).unwrap();
    assert_eq!(old.rows().len(), 1, "positive shared-binding control");
    assert_eq!(new.variables(), old.variables());
    assert_eq!(bag(new.rows()), bag(old.rows()));
}

fn star_patterns(text: &str) -> Vec<spargebra::term::TriplePattern> {
    let Query::Select { mut pattern, .. } = SparqlParser::new().parse_query(text).unwrap() else {
        panic!()
    };
    loop {
        pattern = match pattern {
            GraphPattern::Project { inner, .. } | GraphPattern::Graph { inner, .. } => *inner,
            GraphPattern::Bgp { patterns } => return patterns,
            _ => panic!("expected a single named star"),
        };
    }
}

fn board_ctx(store: &Store) -> crate::sparql::TemporalContext {
    crate::sparql::TemporalContext {
        graph: crate::sparql::GraphScope::Named(
            store.lookup_all("http://example.org/board").unwrap(),
        ),
        ..Default::default()
    }
}

#[test]
fn windows_split_a_dense_star_without_changing_its_rows() {
    // Every status shape: two values on one subject, language, number, IRI.
    let text = "SELECT ?s ?status WHERE { GRAPH <http://example.org/board> { ?s <https://schema.org/agent> \"target\" ; a <https://schema.org/Action> ; <http://example.org/status> ?status } }";
    for composed in [false, true] {
        let (store, _dir) = fixture_density(composed, true);
        let patterns = star_patterns(text);
        let ctx = board_ctx(&store);
        let run = |window| {
            crate::sparql::batch_star::try_eval_windowed(
                &store,
                &patterns,
                &ctx,
                &Bindings::new(),
                window,
            )
            .unwrap()
            .expect("dense star must batch")
        };
        let (whole, variables) = run(usize::MAX);
        assert!(whole.len() > 128, "positive dense control");
        for window in [1, 7, 64] {
            let (split, split_variables) = run(window);
            assert_eq!(split_variables, variables, "window={window}");
            assert_eq!(
                bag(&split),
                bag(&whole),
                "window={window} composed={composed}"
            );
        }
        let old = reference(|| query(&store, text).unwrap());
        assert_eq!(bag(query(&store, text).unwrap().rows()), bag(old.rows()));
    }
}

/// One board graph with `count` Actions owned by the literal "target", each
/// with an "open" status. Written as facts, not parsed RDF, to stay quick.
fn dense_owner_board(count: usize) -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path().join("board.db").to_str().unwrap()).unwrap();
    store.set_read_model_enabled(false);
    let graph = store.graph_create("http://example.org/board").unwrap();
    let agent = store.intern("https://schema.org/agent").unwrap();
    let rdf_type = store.intern(crate::namespace::RDF_TYPE).unwrap();
    let action = store.intern("https://schema.org/Action").unwrap();
    let status = store.intern("http://example.org/status").unwrap();
    let mut facts = Vec::with_capacity(count * 3);
    for i in 0..count {
        let entity = store
            .intern(&format!("http://example.org/owned/{i}"))
            .unwrap();
        for (attribute, value) in [
            (agent, Value::Str("target".into())),
            (rdf_type, Value::Ref(action)),
            (status, Value::Str("open".into())),
        ] {
            facts.push(crate::Datum {
                entity,
                attribute,
                value,
                valid_from: "2026-01-01T00:00:00Z".into(),
                valid_to: None,
                op: crate::types::Op::Assert,
            });
        }
    }
    store
        .transact_to_graph(&facts, "2026-01-01T00:00:00Z", None, None, graph)
        .unwrap();
    (store, dir)
}

#[test]
fn an_owner_denser_than_one_window_is_batched_not_probed_per_subject() {
    // aegis-t9n: one principal owned 22,679 Actions. Its anchor relation
    // overflowed the old 8 MiB accounting (about 1 KiB per row) and every
    // subject was probed alone. 8,500 subjects is just past that bound.
    let (store, _dir) = dense_owner_board(8_500);
    let text = "SELECT ?s ?status WHERE { GRAPH <http://example.org/board> { ?s <https://schema.org/agent> \"target\" ; a <https://schema.org/Action> ; <http://example.org/status> ?status } }";
    let batched = crate::sparql::batch_star::try_eval(
        &store,
        &star_patterns(text),
        &board_ctx(&store),
        &Bindings::new(),
    )
    .unwrap()
    .expect("a dense owner must batch in windows");
    assert_eq!(batched.0.len(), 8_500, "every dense subject is returned");
    let old = reference(|| query(&store, text).unwrap());
    let new = query(&store, text).unwrap();
    assert_eq!(new.variables(), old.variables());
    assert_eq!(bag(new.rows()), bag(old.rows()));
    assert_eq!(bag(&batched.0), bag(old.rows()));
}
