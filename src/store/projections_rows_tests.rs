//! aegis-0ztg: row listings and multi-graph aggregates over a projection.

use serde_json::json;

use super::{PROJECTION_ROWS_MAX, ProjectionAggregate, ProjectionRows};
use crate::Store;
use crate::config::{ProjectionColumnConfig, ProjectionConfig};

const EX: &str = "http://ex.test/";
const G1: &str = "http://ex.test/g/inbox";
const G2: &str = "http://ex.test/g/inbox#ephemeral";

fn def() -> ProjectionConfig {
    ProjectionConfig {
        name: "items".into(),
        key: format!("{EX}status"),
        columns: vec![
            ProjectionColumnConfig {
                name: "priority".into(),
                predicate: format!("{EX}priority"),
                time: false,
            },
            ProjectionColumnConfig {
                name: "title".into(),
                predicate: format!("{EX}title"),
                time: false,
            },
        ],
    }
}

fn knot(store: &mut Store, graph: &str, ttl: &str) {
    crate::tool_knot(
        store,
        &json!({"turtle": format!("@prefix ex: <{EX}> . @prefix xsd: <http://www.w3.org/2001/XMLSchema#> . {ttl}"),
                "graph": graph, "timestamp": "2026-10-23T00:00:00Z"}),
    )
    .unwrap();
}

fn seeded() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    store.graph_create(G1).unwrap();
    store.graph_create(G2).unwrap();
    knot(
        &mut store,
        G1,
        r#"ex:a ex:status "open" ; ex:priority 1 ; ex:title "inbox: one" .
           ex:b ex:status "open" ; ex:priority 2 ; ex:title "inbox: two"^^xsd:token .
           ex:c ex:status "closed" ; ex:priority 2 ."#,
    );
    // ex:a also in the ephemeral graph: a shadow the caller must be able to see.
    knot(&mut store, G2, r#"ex:a ex:status "open" ; ex:priority 1 ."#);
    store.install_projections(&[def()]).unwrap();
    store
}

fn rows(store: &Store, q: &ProjectionRows<'_>) -> serde_json::Value {
    store.projection_rows(&def(), q).unwrap()
}

#[test]
fn rows_list_the_selected_entities_with_their_graph_and_plain_flags() {
    let store = seeded();
    let a = rows(
        &store,
        &ProjectionRows {
            graph: G1,
            key_in: &[json!("open")],
            ..Default::default()
        },
    );
    assert_eq!(a["truncated"], false);
    let listed = a["rows"].as_array().unwrap();
    assert_eq!(listed.len(), 2, "{a}");
    let by: std::collections::BTreeMap<String, &serde_json::Value> = listed
        .iter()
        .map(|r| (r["entity"].as_str().unwrap().to_string(), r))
        .collect();
    let ra = by[&format!("{EX}a")];
    assert_eq!(ra["graph"], G1);
    assert_eq!(ra["key"], "open");
    assert_eq!(ra["key_plain"], true);
    assert_eq!(ra["columns"], json!({"priority": 1, "title": "inbox: one"}));
    assert_eq!(ra["plain"], json!({"priority": false, "title": true}));
    assert_eq!(ra["multi"], false);
    // A typed literal renders by its lexical form but is flagged as not plain.
    let rb = by[&format!("{EX}b")];
    assert_eq!(rb["columns"]["title"], "inbox: two");
    assert_eq!(rb["plain"]["title"], false);
}

#[test]
fn rows_over_two_graphs_show_a_shadow_as_two_rows() {
    let store = seeded();
    let graphs = [G1, G2];
    let a = rows(
        &store,
        &ProjectionRows {
            graphs: &graphs,
            key_in: &[json!("open")],
            columns: Some(&["priority"]),
            ..Default::default()
        },
    );
    let a_rows: Vec<_> = a["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["entity"] == format!("{EX}a"))
        .map(|r| r["graph"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(a_rows, vec![G1.to_string(), G2.to_string()], "{a}");
    assert_eq!(a["rows"][0]["columns"], json!({"priority": 1}));
    // Aggregates over the same graphs group per graph and carry plain.
    let agg = store
        .aggregate_projection(
            &def(),
            &ProjectionAggregate {
                graphs: &graphs,
                group_by: Some("key"),
                ..Default::default()
            },
        )
        .unwrap();
    let mut groups: Vec<(String, String, i64, bool)> = agg["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| {
            (
                g["graph"].as_str().unwrap().to_string(),
                g["value"].as_str().unwrap().to_string(),
                g["n"].as_i64().unwrap(),
                g["plain"].as_bool().unwrap(),
            )
        })
        .collect();
    groups.sort();
    assert_eq!(
        groups,
        vec![
            (G1.into(), "closed".into(), 1, true),
            (G1.into(), "open".into(), 2, true),
            (G2.into(), "open".into(), 1, true),
        ]
    );
}

#[test]
fn rows_limit_truncates_and_never_returns_a_partial_answer_silently() {
    let store = seeded();
    let a = rows(
        &store,
        &ProjectionRows {
            graph: G1,
            limit: 1,
            ..Default::default()
        },
    );
    assert_eq!(a["truncated"], true);
    assert_eq!(a["rows"].as_array().unwrap().len(), 1);
    let all = rows(
        &store,
        &ProjectionRows {
            graph: G1,
            ..Default::default()
        },
    );
    assert_eq!(all["truncated"], false);
    assert_eq!(all["rows"].as_array().unwrap().len(), 3);
}

#[test]
fn rows_refuse_bad_inputs_even_when_the_scope_matches_nothing() {
    let store = seeded();
    let empty: [serde_json::Value; 0] = [];
    let nothing = [("priority", &empty[..])];
    for (q, needle) in [
        (
            ProjectionRows {
                graph: G1,
                columns: Some(&["owner"]),
                ..Default::default()
            },
            "has no column",
        ),
        (
            ProjectionRows {
                graph: G1,
                columns: Some(&["key"]),
                ..Default::default()
            },
            "key is always returned",
        ),
        (
            ProjectionRows {
                graph: G1,
                limit: PROJECTION_ROWS_MAX + 1,
                ..Default::default()
            },
            "exceeds",
        ),
        (
            ProjectionRows {
                graph: G1,
                filters: &nothing,
                columns: Some(&["owner"]),
                ..Default::default()
            },
            "has no column",
        ),
        (
            ProjectionRows {
                graph: "http://ex.test/no-such-graph",
                limit: PROJECTION_ROWS_MAX + 1,
                ..Default::default()
            },
            "exceeds",
        ),
    ] {
        let err = store.projection_rows(&def(), &q).unwrap_err();
        assert!(err.to_string().contains(needle), "{needle}: {err}");
    }
    let five = ["a", "b", "c", "d", "e"];
    let err = store
        .projection_rows(
            &def(),
            &ProjectionRows {
                graphs: &five,
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(err.to_string().contains("at most 4"), "{err}");
}

#[test]
fn rows_flag_a_multi_valued_entity() {
    let mut store = seeded();
    knot(&mut store, G1, r#"ex:c ex:priority 3 ."#);
    let a = rows(
        &store,
        &ProjectionRows {
            graph: G1,
            key_in: &[json!("closed")],
            ..Default::default()
        },
    );
    assert_eq!(a["rows"][0]["multi"], true, "{a}");
    assert_eq!(a["multi_valued_rows"], 1);
}

#[test]
fn rows_refuse_an_uninstalled_definition() {
    let mut store = Store::open_in_memory().unwrap();
    store.graph_create(G1).unwrap();
    knot(&mut store, G1, r#"ex:a ex:status "open" ."#);
    let err = store
        .projection_rows(
            &def(),
            &ProjectionRows {
                graph: G1,
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(err.to_string().contains("not installed"), "{err}");
}
