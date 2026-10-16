use std::cell::Cell;

use oxrdfio::RdfFormat;
use spargebra::{Query, SparqlParser, algebra::GraphPattern};

use super::applicable;
use crate::{
    rdf::ingest_rdf_to_graph,
    sparql::{Bindings, QueryResult, query},
    store::Store,
    types::Value,
};

thread_local! { static DISABLED: Cell<bool> = const { Cell::new(false) }; }
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
    let mut result: Vec<_> = rows
        .iter()
        .map(|row| {
            let mut values: Vec<_> = row.iter().collect();
            values.sort_by_key(|(k, _)| *k);
            format!("{values:?}")
        })
        .collect();
    result.sort();
    result
}

fn put(store: &mut Store, graph: &str, data: &str) {
    let graph = store.graph_create(graph).unwrap();
    ingest_rdf_to_graph(
        store,
        data.as_bytes(),
        RdfFormat::NTriples,
        None,
        "2026-01-01T00:00:00Z",
        None,
        None,
        graph,
    )
    .unwrap();
}

fn fixture(composed: bool) -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.db");
    let mut store = Store::open(main.to_str().unwrap()).unwrap();
    store.set_read_model_enabled(false);
    let raw = dir.path().join("raw.db");
    let mut provider = Store::open(raw.to_str().unwrap()).unwrap();
    for (gi, graph) in [
        "http://example.org/board",
        "http://example.org/board#ephemeral",
    ]
    .iter()
    .enumerate()
    {
        let mut identity = String::new();
        let mut statuses = String::new();
        for (i, (name, values)) in [
            ("open", vec!["\"open\""]),
            ("invalid", vec!["\"invalid\""]),
            ("missing", vec![]),
            ("language", vec!["\"open\"@en"]),
            (
                "number",
                vec!["\"7\"^^<http://www.w3.org/2001/XMLSchema#integer>"],
            ),
            ("reference", vec!["<http://example.org/open>"]),
            ("mixed", vec!["\"open\"", "\"invalid\""]),
            ("two_invalid", vec!["\"invalid\"", "\"broken\""]),
            ("wrong_owner", vec![]),
            ("wrong_type", vec![]),
        ]
        .iter()
        .enumerate()
        {
            let subject = format!("<http://example.org/{gi}/{name}>");
            let owner = if *name == "wrong_owner" {
                "\"other\""
            } else if i % 2 == 0 {
                "\"target\""
            } else {
                "<http://example.org/principal/target>"
            };
            let ty = if *name == "wrong_type" {
                "http://example.org/Other"
            } else {
                "https://schema.org/Action"
            };
            identity.push_str(&format!("{subject} <https://schema.org/agent> {owner} .\n{subject} <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <{ty}> .\n"));
            for value in values {
                statuses.push_str(&format!(
                    "{subject} <http://example.org/status> {value} .\n"
                ));
            }
        }
        put(&mut store, graph, &identity);
        if composed {
            put(&mut provider, graph, &statuses);
        } else {
            put(&mut store, graph, &statuses);
        }
    }
    // A separate graph's missing status must not enter either board graph.
    put(
        &mut store,
        "http://example.org/foreign",
        "<http://example.org/foreign> <https://schema.org/agent> \"target\" .\n<http://example.org/foreign> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <https://schema.org/Action> .\n",
    );
    ingest_rdf_to_graph(&mut store,
        &b"<http://example.org/root-only> <https://schema.org/agent> \"target\" .\n<http://example.org/root-only> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <https://schema.org/Action> .\n"[..],
        RdfFormat::NTriples, None, "2026-01-01T00:00:00Z", None, None, 0).unwrap();
    drop(provider);
    drop(store);
    let store = if composed {
        let pack = dir.path().join("pack.db");
        crate::store::respace::respace_file(&raw, &pack, 1).unwrap();
        Store::open_with_attachments(
            main.to_str().unwrap(),
            &[crate::store::attach::Attachment::read_only(
                "archive",
                pack.to_str().unwrap(),
            )],
        )
        .unwrap()
    } else {
        Store::open(main.to_str().unwrap()).unwrap()
    };
    store.set_read_model_enabled(false);
    (store, dir)
}

#[test]
fn guard_preserves_bag_headers_and_graph_scope_across_providers() {
    for composed in [false, true] {
        let (store, _dir) = fixture(composed);
        let text =
            include_str!("owner_guard.sparql").replacen("ASK {", "SELECT ?s ?bad WHERE {", 1);
        let old = reference(|| query(&store, &text).unwrap());
        let new = query(&store, &text).unwrap();
        assert_eq!(new.variables(), old.variables());
        assert_eq!(bag(new.rows()), bag(old.rows()), "composed={composed}");
        for excluded in ["http://example.org/foreign", "http://example.org/root-only"] {
            let id = store.lookup(excluded).unwrap().unwrap();
            assert!(
                !new.rows()
                    .iter()
                    .any(|row| row.get("s") == Some(&Value::Ref(id)))
            );
        }
        for gi in 0..2 {
            for (name, expected) in [
                ("open", 0),
                ("invalid", 1),
                ("missing", 1),
                ("language", 2),
                ("number", 2),
                ("reference", 2),
                ("mixed", 0),
                ("two_invalid", 2),
                ("wrong_owner", 0),
                ("wrong_type", 0),
            ] {
                let id = store
                    .lookup(&format!("http://example.org/{gi}/{name}"))
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    new.rows()
                        .iter()
                        .filter(|row| row.get("s") == Some(&Value::Ref(id)))
                        .count(),
                    expected,
                    "{composed}/{gi}/{name}"
                );
            }
        }
        assert!(matches!(
            query(&store, include_str!("owner_guard.sparql")).unwrap(),
            QueryResult::Ask(true)
        ));
    }
}

fn operands(text: &str) -> (GraphPattern, GraphPattern) {
    let Query::Select { mut pattern, .. } = SparqlParser::new().parse_query(text).unwrap() else {
        panic!("SELECT required")
    };
    loop {
        pattern = match pattern {
            GraphPattern::Project { inner, .. } => *inner,
            GraphPattern::Join { left, right } => return (*left, *right),
            other => panic!("join required: {other:?}"),
        };
    }
}

#[test]
fn matcher_refuses_new_bindings_volatile_and_scope_operators() {
    for text in [
        "SELECT * WHERE { { ?s <http://example.org/p> ?x FILTER(?x = ?new) } ?s <http://example.org/q> ?new }",
        "SELECT * WHERE { { ?s <http://example.org/p> ?x FILTER(RAND() < 0.5) } ?s <http://example.org/q> 1 }",
        "SELECT * WHERE { { ?s <http://example.org/p> ?x OPTIONAL { ?s <http://example.org/z> ?z } FILTER(?x = 1) } ?s <http://example.org/q> 1 }",
    ] {
        let (left, right) = operands(text);
        assert!(applicable(&left, &right).is_none());
    }
}

#[test]
fn matcher_admits_existing_subject_only() {
    let (left, right) = operands(
        r#"SELECT * WHERE { { ?s <http://example.org/p> ?x FILTER(?x = "open") } ?s <http://example.org/owner> "target" }"#,
    );
    assert_eq!(
        applicable(&left, &right),
        Some(vec!["s".into(), "x".into()])
    );
}

/// An explicit local demo, kept out of the normal test workload.
#[test]
#[ignore = "12,000-action timing demonstration; run explicitly"]
fn owner_guard_demo_12000_actions() {
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
        for i in 0..6000 {
            let owner = if i < 4 { "target" } else { "unrelated" };
            let s = format!("<http://example.org/demo/{gi}/{i}>");
            data.push_str(&format!("{s} <https://schema.org/agent> \"{owner}\" .\n{s} <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <https://schema.org/Action> .\n{s} <http://example.org/status> \"open\" .\n"));
        }
        put(&mut store, graph, &data);
    }
    let text = include_str!("owner_guard.sparql");
    for optimized in [false, true, true, false] {
        let start = std::time::Instant::now();
        let result = if optimized {
            query(&store, text).unwrap()
        } else {
            reference(|| query(&store, text).unwrap())
        };
        assert!(matches!(result, QueryResult::Ask(false)));
        println!(
            "GUARD_DEMO mode={} result=false elapsed_ms={:.3}",
            if optimized {
                "subject_bound"
            } else {
                "independent_join"
            },
            start.elapsed().as_secs_f64() * 1000.0
        );
    }
    put(
        &mut store,
        "http://example.org/board",
        "<http://example.org/demo/invalid> <https://schema.org/agent> \"target\" .\n<http://example.org/demo/invalid> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <https://schema.org/Action> .\n<http://example.org/demo/invalid> <http://example.org/status> \"invalid\" .\n",
    );
    for optimized in [false, true] {
        let result = if optimized {
            query(&store, text).unwrap()
        } else {
            reference(|| query(&store, text).unwrap())
        };
        assert!(matches!(result, QueryResult::Ask(true)));
        println!(
            "GUARD_DEMO mode={} invalid_owned_positive=true",
            if optimized {
                "subject_bound"
            } else {
                "independent_join"
            }
        );
    }
}
