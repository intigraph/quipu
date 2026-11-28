//! A correlated FILTER must decline a driver before counting a whole graph.
use crate::sparql::query;
use crate::store::Datum;
use crate::types::Op;
use crate::{Store, Value};
const GRAPH: &str = "https://example.test/ready";
fn schema(local: &str) -> String {
    format!("https://schema.org/{local}")
}
fn predicate(local: &str) -> String {
    format!("urn:test:{local}")
}
fn put(store: &mut Store, graph: &str, facts: &[(&str, &str, Value)]) {
    let graph = store.graph_create(graph).unwrap();
    let data = facts
        .iter()
        .map(|(subject, predicate, value)| Datum {
            entity: store.intern(subject).unwrap(),
            attribute: store.intern(predicate).unwrap(),
            value: value.clone(),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
            op: Op::Assert,
        })
        .collect();
    store
        .transact_graph_batches(&[(graph, data)], "2026-01-01T00:00:00Z", None, None)
        .unwrap();
}

fn ids(store: &Store, text: &str) -> Vec<String> {
    let mut ids: Vec<_> = query(store, text)
        .unwrap()
        .rows()
        .iter()
        .map(|row| match &row["id"] {
            Value::Str(id) => id.clone(),
            other => panic!("unexpected identifier: {other:?}"),
        })
        .collect();
    ids.sort();
    ids
}

fn fixture() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    let action = Value::Ref(store.intern(&schema("Action")).unwrap());
    let rdf_type = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
    let status = predicate("status");
    let identifier = schema("identifier");
    let blocked_on = predicate("blockedOn");
    let issue_type = predicate("issueType");
    for id in [
        "plain",
        "closed",
        "tombstone",
        "missing",
        "active",
        "mixed",
        "two",
        "ops",
        "ask",
        "foreign",
    ] {
        let subject = format!("urn:item:{id}");
        put(
            &mut store,
            GRAPH,
            &[
                (&subject, rdf_type, action.clone()),
                (&subject, &identifier, Value::Str(id.into())),
                (&subject, &status, Value::Str("open".into())),
            ],
        );
    }
    for (id, states) in [
        ("closed", vec!["closed"]),
        ("tombstone", vec!["tombstone"]),
        ("missing", vec![]),
        ("active", vec!["in_progress"]),
        ("mixed", vec!["closed", "open"]),
        ("two", vec!["closed", "tombstone"]),
        ("foreign", vec![]),
    ] {
        let blocker = format!("urn:blocker:{id}");
        let blocker_ref = Value::Ref(store.intern(&blocker).unwrap());
        put(
            &mut store,
            GRAPH,
            &[(&format!("urn:item:{id}"), &blocked_on, blocker_ref)],
        );
        for state in states {
            // Blockers need not themselves be Actions.
            put(
                &mut store,
                GRAPH,
                &[(&blocker, &status, Value::Str(state.into()))],
            );
        }
    }
    for id in ["ops", "ask"] {
        put(
            &mut store,
            GRAPH,
            &[
                (
                    &format!("urn:item:{id}"),
                    &issue_type,
                    Value::Str(id.into()),
                ),
                (
                    &format!("urn:item:{id}"),
                    &issue_type,
                    Value::Str("task".into()),
                ),
            ],
        );
    }
    // Foreign status must not block a source in the selected graph.
    put(
        &mut store,
        "urn:foreign",
        &[("urn:blocker:foreign", &status, Value::Str("open".into()))],
    );
    store
}

#[test]
fn correlated_filter_does_not_repeat_graph_affordability_counts() {
    // Fresh connections: neither the model nor a previous count can hide the work.
    let store = fixture();
    store.enable_statement_profile();
    let text = format!(
        r#"
        PREFIX schema: <https://schema.org/> PREFIX t: <urn:test:>
        SELECT ?id WHERE {{ GRAPH <{GRAPH}> {{
          ?item t:status "open" .
          ?item a schema:Action ; schema:identifier ?id .
          FILTER NOT EXISTS {{ ?item t:issueType "ops" }}
          FILTER NOT EXISTS {{ ?item t:issueType "ask" }}
          FILTER NOT EXISTS {{ ?item t:blockedOn ?blocker .
            ?blocker t:status ?bs . ?item a schema:Action .
            FILTER(?bs != "closed" && ?bs != "tombstone") }}
        }} }} LIMIT 10000"#
    );
    let span = crate::statement_profile::Span::arm();
    assert_eq!(
        ids(&store, &text),
        ["closed", "foreign", "missing", "plain", "tombstone", "two"]
    );
    let profile = span.finish().unwrap();
    let counts: u64 = profile
        .by_sql
        .iter()
        .filter(|(sql, _)| {
            sql.starts_with("SELECT COUNT(*) FROM facts WHERE op = 1 AND valid_to IS NULL AND g =")
        })
        .map(|(_, (count, _))| count)
        .sum();
    println!("global affordability COUNTs: {counts}");
    assert!(
        counts <= 1,
        "correlated FILTER counted a whole graph {counts} times"
    );
}

#[test]
fn viable_filter_keeps_the_read_model_guard() {
    let store = fixture();
    let text = format!(
        r#"PREFIX schema: <https://schema.org/> PREFIX t: <urn:test:>
        SELECT ?id WHERE {{ GRAPH <{GRAPH}> {{
          ?item t:status ?status ; schema:identifier ?id . FILTER(?status = "open")
        }} }}"#
    );
    store.enable_statement_profile();
    let span = crate::statement_profile::Span::arm();
    assert_eq!(ids(&store, &text).len(), 10);
    let totals = span.finish().unwrap();
    assert!(
        totals.by_sql.keys().any(|sql| sql
            .starts_with("SELECT COUNT(*) FROM facts WHERE op = 1 AND valid_to IS NULL AND g =")),
        "a viable driver must still check model affordability"
    );
}

#[test]
fn resident_model_declines_before_the_seeded_probe() {
    use spargebra::algebra::GraphPattern;
    fn filtered(pattern: &GraphPattern) -> (&spargebra::algebra::Expression, &GraphPattern) {
        match pattern {
            GraphPattern::Project { inner, .. } => filtered(inner),
            GraphPattern::Filter { expr, inner } => (expr, inner),
            other => panic!("expected filtered BGP, got {other:?}"),
        }
    }
    let store = fixture();
    let graph = store.lookup(GRAPH).unwrap().unwrap();
    drop(store.read_model_for(graph).unwrap());
    let parsed = crate::sparql::sparql_parser()
        .parse_query(
            "SELECT ?bs WHERE { ?outer <urn:test:blockedOn> ?item .
         ?item <urn:test:status> ?bs . FILTER(?bs = \"open\") }",
        )
        .unwrap();
    let spargebra::Query::Select { pattern, .. } = parsed else {
        panic!("select")
    };
    let (expr, inner) = filtered(&pattern);
    let ctx = crate::sparql::TemporalContext {
        graph: crate::sparql::GraphScope::Named(vec![graph]),
        ..Default::default()
    };
    let seed = crate::sparql::Bindings::from_iter([(
        "outer".into(),
        Value::Ref(store.lookup("urn:item:active").unwrap().unwrap()),
    )]);
    store.enable_statement_profile();
    let span = crate::statement_profile::Span::arm();
    assert!(
        super::try_eval(&store, expr, inner, &ctx, &seed)
            .unwrap()
            .is_none()
    );
    let totals = span.finish().unwrap();
    assert_eq!(
        totals.statements, 0,
        "resident path must not probe or count"
    );
}
