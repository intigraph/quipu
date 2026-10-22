//! Composition controls for bounded SQL stars and statement-local aliases.
use super::*;
use crate::store::attach::Attachment;
use crate::types::{Op, Value};
use spargebra::term::{NamedNode, Variable};

fn composed() -> (Store, tempfile::TempDir, i64) {
    let dir = tempfile::tempdir().unwrap();
    let seed = |path: &std::path::Path, graph_iri: &str| {
        let mut store = Store::open(path.to_str().unwrap()).unwrap();
        let graph = store.graph_create(graph_iri).unwrap();
        let subject = store.intern("urn:test:subject").unwrap();
        let owner = store.intern("urn:test:owner").unwrap();
        let principal = store.intern("urn:test:principal").unwrap();
        let status = store.intern("urn:test:status").unwrap();
        let rdf_type = store.intern(crate::namespace::RDF_TYPE).unwrap();
        let class = store.intern("urn:test:Action").unwrap();
        for (predicate, value) in [
            (owner, Value::Ref(principal)),
            (rdf_type, Value::Ref(class)),
            (status, Value::Str("open".into())),
            (status, Value::Str("closed".into())),
        ] {
            store
                .transact_to_graph(
                    &[crate::Datum {
                        entity: subject,
                        attribute: predicate,
                        value,
                        valid_from: "2026-10-11T00:00:00Z".into(),
                        valid_to: None,
                        op: Op::Assert,
                    }],
                    "2026-10-11T00:00:00Z",
                    None,
                    None,
                    graph,
                )
                .unwrap();
        }
        graph
    };
    let local = dir.path().join("local.db");
    let graph = seed(&local, "urn:test:local");
    let raw = dir.path().join("raw.db");
    seed(&raw, "urn:test:archive");
    let pack = dir.path().join("archive.db");
    crate::store::respace::respace_file(&raw, &pack, 1).unwrap();
    let store = Store::open_with_attachments(
        local.to_str().unwrap(),
        &[Attachment::read_only("archive", pack.to_str().unwrap())],
    )
    .unwrap();
    (store, dir, graph)
}

fn patterns() -> Vec<TriplePattern> {
    let var = |name| TermPattern::Variable(Variable::new(name).unwrap());
    let pred = |iri| NamedNodePattern::NamedNode(NamedNode::new(iri).unwrap());
    vec![
        TriplePattern {
            subject: var("s"),
            predicate: pred("urn:test:owner"),
            object: NamedNode::new("urn:test:principal").unwrap().into(),
        },
        TriplePattern {
            subject: var("s"),
            predicate: pred("urn:test:status"),
            object: var("value"),
        },
    ]
}

#[test]
fn composed_star_retains_canonical_identity_and_multiple_values() {
    let (store, _dir, graph) = composed();
    let ctx = TemporalContext {
        graph: super::super::GraphScope::Named(vec![graph]),
        ..TemporalContext::default()
    };
    let (rows, headers) = try_eval(&store, &patterns(), &ctx, &Bindings::new())
        .unwrap()
        .unwrap();
    assert_eq!(headers, ["s", "value"]);
    assert_eq!(rows.len(), 2);
    let subject = store.lookup("urn:test:subject").unwrap().unwrap();
    assert!(
        rows.iter()
            .all(|r| r.get("s") == Some(&Value::Ref(subject)))
    );
    let result = super::super::query(&store,
        "SELECT ?s WHERE { GRAPH <urn:test:local> { ?s <urn:test:owner> <urn:test:principal>; <urn:test:status> ?v FILTER(?v=\"open\") } } ORDER BY STR(?s)").unwrap();
    assert_eq!(result.rows().len(), 1);
    let result = super::super::query(&store,
        "SELECT ?s WHERE { GRAPH <urn:test:archive> { ?s <urn:test:owner> <urn:test:principal>; <urn:test:status> ?v FILTER(?v=\"open\") } } ORDER BY STR(?s)").unwrap();
    assert_eq!(result.rows().len(), 1);
    assert_eq!(result.rows()[0].get("s"), Some(&Value::Ref(subject)));
}

#[test]
fn unsupported_scopes_keep_the_existing_evaluator() {
    let (store, _dir, graph) = composed();
    let ctx = TemporalContext {
        graph: super::super::GraphScope::Named(vec![graph]),
        ..TemporalContext::default()
    };
    for other in [
        TemporalContext::default(),
        TemporalContext {
            row_limit: Some(1),
            ..ctx.clone()
        },
        TemporalContext {
            row_cap: Some(10),
            ..ctx.clone()
        },
        TemporalContext {
            valid_at: Some("2026-10-10T00:00:00Z".into()),
            ..ctx.clone()
        },
        TemporalContext {
            as_of_tx: Some(1),
            ..ctx.clone()
        },
    ] {
        assert!(
            try_eval(&store, &patterns(), &other, &Bindings::new())
                .unwrap()
                .is_none()
        );
    }
    let mut disconnected = patterns();
    disconnected[1].subject = Variable::new("other").unwrap().into();
    assert!(
        try_eval(&store, &disconnected, &ctx, &Bindings::new())
            .unwrap()
            .is_none()
    );
    let mut seed = Bindings::new();
    seed.insert("s".into(), Value::Ref(1));
    assert!(
        try_eval(&store, &patterns(), &ctx, &seed)
            .unwrap()
            .is_none()
    );
}

#[test]
fn alias_snapshot_is_complete_or_falls_back_and_refreshes() {
    let (store, _dir, _graph) = composed();
    let ctx = TemporalContext::default();
    let aliases = super::super::triple_alias::snapshot(&store, &ctx)
        .unwrap()
        .unwrap();
    assert!(!aliases.is_empty());
    for (alias, canonical) in &aliases {
        assert_eq!(store.canonical_id(*alias).unwrap(), *canonical);
    }
    store
        .conn
        .execute_batch("INSERT INTO term_alias(alias_id,canonical_id) VALUES(-10,-11)")
        .unwrap();
    let fresh = super::super::triple_alias::snapshot(&store, &ctx)
        .unwrap()
        .unwrap();
    assert_eq!(fresh.get(&-10), Some(&-11));
    store.conn.execute_batch("WITH RECURSIVE n(v) AS (SELECT 1 UNION ALL SELECT v+1 FROM n WHERE v<4097) INSERT INTO term_alias(alias_id,canonical_id) SELECT -1000-v,-2000-v FROM n").unwrap();
    assert!(
        super::super::triple_alias::snapshot(&store, &ctx)
            .unwrap()
            .is_none(),
        "partial map is unsafe"
    );
}

#[test]
fn composed_absence_rechecks_owner_and_literal_identity() {
    let (mut store, _dir, graph) = composed();
    let owner = store.lookup("urn:test:owner").unwrap().unwrap();
    let principal = store.lookup("urn:test:principal").unwrap().unwrap();
    let status = store.lookup("urn:test:status").unwrap().unwrap();
    let rdf_type = store.lookup(crate::namespace::RDF_TYPE).unwrap().unwrap();
    let class = store.lookup("urn:test:Action").unwrap().unwrap();
    for (name, own, value) in [
        ("none", true, None),
        ("wrong-owner", false, None),
        (
            "language",
            true,
            Some(Value::Lang {
                lexical: "closed".into(),
                lang: "en".into(),
            }),
        ),
        ("iri", true, Some(Value::Ref(principal))),
        ("integer", true, Some(Value::Int(7))),
        ("plain", true, Some(Value::Str("closed".into()))),
        (
            "typed-string",
            true,
            Some(Value::Typed {
                lexical: "closed".into(),
                datatype: crate::namespace::XSD_STRING.into(),
            }),
        ),
    ] {
        let subject = store.intern(&format!("urn:test:{name}")).unwrap();
        let mut data = vec![
            (rdf_type, Value::Ref(class)),
            (owner, Value::Ref(if own { principal } else { class })),
        ];
        if let Some(value) = value {
            data.push((status, value));
        }
        let data: Vec<_> = data
            .into_iter()
            .map(|(attribute, value)| crate::Datum {
                entity: subject,
                attribute,
                value,
                valid_from: "2026-10-11T00:00:00Z".into(),
                valid_to: None,
                op: Op::Assert,
            })
            .collect();
        store
            .transact_to_graph(&data, "2026-10-11T00:00:00Z", None, None, graph)
            .unwrap();
    }
    let query = "SELECT ?s WHERE { GRAPH <urn:test:local> { ?s a <urn:test:Action>; <urn:test:owner> <urn:test:principal> FILTER NOT EXISTS { ?s <urn:test:status> ?v FILTER(isLiteral(?v) && sameTerm(?v,STR(?v))) } } } ORDER BY STR(?s)";
    let result = super::super::query(&store, query).unwrap();
    let names: Vec<_> = result
        .rows()
        .iter()
        .map(|row| match row.get("s").unwrap() {
            Value::Ref(id) => store.resolve(*id).unwrap(),
            _ => panic!("subject must be a reference"),
        })
        .collect();
    assert_eq!(
        names,
        [
            "urn:test:integer",
            "urn:test:iri",
            "urn:test:language",
            "urn:test:none"
        ]
    );
}

#[test]
fn sql_relation_bound_never_returns_a_dense_prefix() {
    let (store, _dir, graph) = composed();
    store.conn.execute_batch("WITH RECURSIVE n(v) AS (SELECT 1 UNION ALL SELECT v+1 FROM n WHERE v<50000) INSERT INTO terms(iri) SELECT 'urn:test:dense:'||v FROM n").unwrap();
    store.conn.execute("INSERT INTO facts(e,a,v,tx,valid_from,op,g) SELECT id,?1,?2,(SELECT MAX(id) FROM transactions),'2026-10-11T00:00:00Z',1,?3 FROM terms WHERE iri LIKE 'urn:test:dense:%'",
        rusqlite::params![store.lookup("urn:test:status").unwrap().unwrap(),Value::Str("open".into()).to_bytes(),graph]).unwrap();
    let ctx = TemporalContext {
        graph: super::super::GraphScope::Named(vec![graph]),
        ..TemporalContext::default()
    };
    assert!(
        try_eval(&store, &patterns(), &ctx, &Bindings::new())
            .unwrap()
            .is_none(),
        "50001st row must select the original planner"
    );
    store.conn.execute("INSERT INTO facts(e,a,v,tx,valid_from,op,g) SELECT id,?1,?2,(SELECT MAX(id) FROM transactions),'2026-10-11T00:00:00Z',1,?3 FROM terms WHERE iri LIKE 'urn:test:dense:%'",
        rusqlite::params![store.lookup(crate::namespace::RDF_TYPE).unwrap().unwrap(), Value::Ref(store.lookup("urn:test:Action").unwrap().unwrap()).to_bytes(), graph]).unwrap();
    let inner = spargebra::algebra::GraphPattern::Bgp {
        patterns: patterns(),
    };
    assert!(
        super::super::composed_absence::candidates(
            &store,
            ["s", "urn:test:Action", "urn:test:status"],
            graph,
            &inner,
            &ctx
        )
        .unwrap()
        .is_none(),
        "dense class candidate collection must fall back"
    );
}

#[test]
fn malformed_unrelated_relation_retains_selective_lookup() {
    let (store, _dir, graph) = composed();
    let subject = store.intern("urn:test:unowned-malformed").unwrap();
    let status = store.lookup("urn:test:status").unwrap().unwrap();
    store.conn.execute("INSERT INTO facts(e,a,v,tx,valid_from,op,g) VALUES(?1,?2,?3,(SELECT MAX(id) FROM transactions),'2026-10-11T00:00:00Z',1,?4)",
        rusqlite::params![subject,status,vec![255_u8],graph]).unwrap();
    let ctx = TemporalContext {
        graph: super::super::GraphScope::Named(vec![graph]),
        ..TemporalContext::default()
    };
    assert!(
        try_eval(&store, &patterns(), &ctx, &Bindings::new())
            .unwrap()
            .is_none()
    );
    let query = "SELECT ?s ?v WHERE { GRAPH <urn:test:local> { ?s <urn:test:owner> <urn:test:principal>; <urn:test:status> ?v } } ORDER BY STR(?s)";
    // The composed path stays selective around the unrelated bad blob.
    let composed_answer = {
        let _view = store.scope_composition(true);
        let parsed = super::super::sparql_parser().parse_query(query).unwrap();
        super::super::eval_parsed(&store, parsed, &TemporalContext::default()).unwrap()
    };
    assert_eq!(
        composed_answer.rows().len(),
        2,
        "unrelated bad blob must not break a selective query"
    );
    // urn:test:local is not an attached graph, so the query itself answers as
    // the unattached store does (aegis-xacr), which refuses the same blob.
    let local = super::super::query(&store, query);
    assert!(
        matches!(&local, Err(crate::Error::InvalidValue(m)) if m.contains("unknown tag")),
        "{local:?}"
    );
}

#[test]
fn complete_relation_matches_grouped_bag_with_duplicates_and_literal_aliases() {
    let (mut store, _dir, graph) = composed();
    let subject = store.lookup("urn:test:subject").unwrap().unwrap();
    let status = store.lookup("urn:test:status").unwrap().unwrap();
    for value in [
        Value::Str("same".into()),
        Value::Typed {
            lexical: "same".into(),
            datatype: crate::namespace::XSD_STRING.into(),
        },
        Value::Lang {
            lexical: "same".into(),
            lang: "en".into(),
        },
        Value::Int(7),
        Value::Typed {
            lexical: "7".into(),
            datatype: "urn:test:datatype".into(),
        },
    ] {
        store
            .transact_to_graph(
                &[crate::Datum {
                    entity: subject,
                    attribute: status,
                    value,
                    valid_from: "2026-10-11T00:00:00Z".into(),
                    valid_to: None,
                    op: Op::Assert,
                }],
                "2026-10-11T00:00:00Z",
                None,
                None,
                graph,
            )
            .unwrap();
    }
    store.conn.execute("INSERT INTO facts(e,a,v,tx,valid_from,op,g) SELECT e,a,v,(SELECT MAX(id) FROM transactions),'2026-10-11T00:00:00Z',op,g FROM facts WHERE e=?1 AND a=?2 AND tx<(SELECT MAX(id) FROM transactions) LIMIT 1", rusqlite::params![subject,status]).unwrap();
    let bag = |rows: Vec<Bindings>| {
        let mut rows: Vec<_> = rows
            .into_iter()
            .map(|row| {
                let mut cells: Vec<_> = row
                    .into_iter()
                    .map(|(key, value)| (key, value.term_key()))
                    .collect();
                cells.sort();
                cells
            })
            .collect();
        rows.sort();
        rows
    };
    for name in ["urn:test:local", "urn:test:archive"] {
        let graph_ids = store.lookup_all(name).unwrap();
        assert_eq!(graph_ids.len(), 1, "positive mounted graph coverage");
        let ctx = TemporalContext {
            graph: super::super::GraphScope::Named(graph_ids),
            ..TemporalContext::default()
        };
        for pattern in patterns() {
            let grouped = super::super::triple::eval_triple_pattern_limited(
                &store,
                &pattern,
                &Bindings::new(),
                &ctx,
                None,
            )
            .unwrap();
            let complete = super::super::triple::eval_complete_relation(
                &store,
                &pattern,
                &ctx,
                MAX_RELATION_ROWS + 1,
            )
            .unwrap();
            assert!(!grouped.is_empty(), "positive graph/predicate coverage");
            assert_eq!(
                bag(grouped),
                bag(complete),
                "same canonical binding bag in {name}"
            );
        }
    }
    let ctx = TemporalContext {
        graph: super::super::GraphScope::Named(vec![graph]),
        deadline: Some(crate::time::Deadline::after_millis(0)),
        ..TemporalContext::default()
    };
    assert!(
        try_eval(&store, &patterns(), &ctx, &Bindings::new()).is_err(),
        "no deadline bypass"
    );
}

#[test]
fn complete_relation_keeps_missing_subject_dictionary_errors() {
    let (store, _dir, graph) = composed();
    let owner = store.lookup("urn:test:owner").unwrap().unwrap();
    store
        .conn
        .pragma_update(None, "foreign_keys", false)
        .unwrap();
    store.conn.execute(
        "INSERT INTO facts(e,a,v,tx,valid_from,op,g) SELECT 99999999,a,v,tx,valid_from,op,g FROM facts WHERE a=?1 AND g=?2 LIMIT 1",
        rusqlite::params![owner, graph],
    ).unwrap();
    let ctx = TemporalContext {
        graph: super::super::GraphScope::Named(vec![graph]),
        ..TemporalContext::default()
    };
    let pattern = patterns().remove(0);
    assert!(
        super::super::triple::eval_triple_pattern_limited(
            &store,
            &pattern,
            &Bindings::new(),
            &ctx,
            None,
        )
        .is_err(),
        "ordinary evaluator must observe the corrupt dictionary reference"
    );
    assert!(super::super::triple::eval_complete_relation(
        &store, &pattern, &ctx, MAX_RELATION_ROWS + 1,
    ).is_err(), "verified binding must preserve the dictionary error");
}
