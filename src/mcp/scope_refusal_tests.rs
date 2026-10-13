//! Graph scope mistakes must refuse without changing either graph.

use crate::Store;
use crate::mcp::tool_query;
use crate::mcp::tools::{tool_retract, tool_set};
use crate::types::{Op, Value};
use serde_json::json;

fn scoped_store() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    crate::rdf::ingest_rdf(
        &mut store,
        b"<https://example.org/item> <https://example.org/value> \"root\" .".as_slice(),
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-01-01T00:00:00Z",
        None,
        None,
    )
    .unwrap();
    let graph = store
        .overlay_create("https://example.org/named", 0)
        .unwrap();
    let entity = store.lookup("https://example.org/item").unwrap().unwrap();
    let predicate = store.lookup("https://example.org/value").unwrap().unwrap();
    store
        .overlay_write(
            graph,
            Op::Assert,
            entity,
            predicate,
            Value::Str("named".into()),
            "2026-01-01T00:00:00Z",
        )
        .unwrap();
    store
}

fn values(store: &Store, graph: Option<&str>) -> serde_json::Value {
    let pattern = "<https://example.org/item> <https://example.org/value> ?v";
    let body = graph.map_or_else(
        || pattern.to_string(),
        |g| format!("GRAPH <{g}> {{ {pattern} }}"),
    );
    tool_query(
        store,
        &json!({"query": format!("SELECT ?v WHERE {{ {body} }}")}),
    )
    .unwrap()["rows"]
        .clone()
}

#[test]
fn root_graph_clause_refuses_instead_of_reporting_a_false_zero() {
    let store = scoped_store();
    assert_eq!(values(&store, None), json!([{"v": "root"}]));
    assert_eq!(
        values(&store, Some("https://example.org/named")),
        json!([{"v": "named"}])
    );
    for select in ["SELECT ?s ?p ?o", "SELECT (COUNT(*) AS ?n)", "ASK"] {
        let error = tool_query(
            &store,
            &json!({"query": format!("{select} WHERE {{ GRAPH <urn:quipu:graph:root> {{ ?s ?p ?o }} }}")}),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("default graph"), "{error}");
        assert!(error.contains("omit the GRAPH clause"), "{error}");
    }
    let from = tool_query(
        &store,
        &json!({"query": "SELECT ?v FROM <urn:quipu:graph:root> WHERE { <https://example.org/item> <https://example.org/value> ?v }"}),
    )
    .unwrap();
    assert_eq!(from["rows"], values(&store, None));
}

#[test]
fn root_graph_refusal_covers_empty_branches_expressions_and_prefixes() {
    let store = scoped_store();
    let root = "GRAPH <urn:quipu:graph:root> { ?s ?p ?o }";
    for query in [
        format!("SELECT ?s WHERE {{ <https://example.org/absent> ?p ?o . {root} }}"),
        format!("SELECT ?s WHERE {{ OPTIONAL {{ {root} }} }}"),
        format!("SELECT ?s WHERE {{ {{ ?s ?p ?o }} UNION {{ {root} }} }}"),
        format!("SELECT ?s WHERE {{ ?s ?p ?o MINUS {{ {root} }} }}"),
        format!("SELECT ?s WHERE {{ ?s ?p ?o FILTER NOT EXISTS {{ {root} }} }}"),
        format!("SELECT ?v WHERE {{ BIND(EXISTS {{ {root} }} AS ?v) }}"),
        format!("SELECT ?s WHERE {{ ?s ?p ?o }} ORDER BY (EXISTS {{ {root} }})"),
        format!("SELECT (SUM(IF(EXISTS {{ {root} }}, 1, 0)) AS ?n) WHERE {{ ?s ?p ?o }}"),
        "PREFIX scope: <urn:quipu:graph:> SELECT ?s WHERE { GRAPH scope:root { ?s ?p ?o } }".into(),
        format!("CONSTRUCT {{ ?s ?p ?o }} WHERE {{ {root} }}"),
        format!("DESCRIBE ?s WHERE {{ {root} }}"),
    ] {
        let error = tool_query(&store, &json!({"query": query}))
            .unwrap_err()
            .to_string();
        assert!(error.contains("omit the GRAPH clause"), "{query}: {error}");
    }
    let literal = tool_query(
        &store,
        &json!({"query": "SELECT ?v WHERE { VALUES ?v { \"GRAPH <urn:quipu:graph:root>\" } }"}),
    )
    .unwrap();
    assert_eq!(
        literal["rows"],
        json!([{"v": "GRAPH <urn:quipu:graph:root>"}])
    );
}

#[test]
fn root_mutations_refuse_graph_before_writing_or_interning() {
    for graph in [
        json!("https://example.org/named"),
        json!("urn:quipu:graph:root"),
        json!("https://example.org/missing"),
        json!(null),
    ] {
        for operation in ["set", "retract"] {
            let mut store = scoped_store();
            let root = values(&store, None);
            let named = values(&store, Some("https://example.org/named"));
            let transactions = store.list_transactions().unwrap().len();
            let input = json!({
                "entity": "https://example.org/item",
                "predicate": "https://example.org/value",
                "value": "replacement",
                "graph": graph,
                "allow_orphan": true
            });
            let error = if operation == "set" {
                tool_set(&mut store, &input)
            } else {
                tool_retract(&mut store, &input)
            }
            .unwrap_err()
            .to_string();
            assert!(error.contains("graph"), "{operation}: {error}");
            assert!(error.contains("ROOT"), "{operation}: {error}");
            assert_eq!(values(&store, None), root);
            assert_eq!(values(&store, Some("https://example.org/named")), named);
            assert_eq!(store.list_transactions().unwrap().len(), transactions);

            // Even a new predicate must not be interned by a refused set.
            let mut new_predicate = input;
            new_predicate["predicate"] = json!("https://example.org/new-predicate");
            assert!(tool_set(&mut store, &new_predicate).is_err());
            assert!(
                store
                    .lookup("https://example.org/new-predicate")
                    .unwrap()
                    .is_none()
            );
        }
    }
}

#[test]
fn unscoped_mutations_change_only_root_and_leave_named_facts() {
    let mut store = scoped_store();
    let input = json!({
        "entity": "https://example.org/item",
        "predicate": "https://example.org/value",
        "value": "replacement",
        "allow_orphan": true
    });
    tool_set(&mut store, &input).unwrap();
    assert_eq!(values(&store, None), json!([{"v": "replacement"}]));
    assert_eq!(
        values(&store, Some("https://example.org/named")),
        json!([{"v": "named"}])
    );
    tool_retract(&mut store, &input).unwrap();
    assert_eq!(values(&store, None), json!([]));
    assert_eq!(
        values(&store, Some("https://example.org/named")),
        json!([{"v": "named"}])
    );
}
