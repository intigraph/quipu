//! `POST /projections` handler tests (aegis-tg5 F2, aegis-2fa).

use std::sync::Arc;

use axum::extract::State;
use serde_json::json;

use super::projections;

const EX: &str = "http://ex.test/";

fn handle() -> (tempfile::TempDir, super::SharedStore) {
    let (dir, h) = super::super::tests::pooled_handle(0);
    let def = quipu::config::ProjectionConfig {
        name: "items".into(),
        key: format!("{EX}status"),
        columns: vec![
            quipu::config::ProjectionColumnConfig {
                name: "priority".into(),
                predicate: format!("{EX}priority"),
                time: false,
                equals: None,
            },
            quipu::config::ProjectionColumnConfig {
                name: "assignee".into(),
                predicate: format!("{EX}assignee"),
                time: false,
                equals: None,
            },
        ],
    };
    {
        let mut store = h.lock();
        quipu::tool_knot(
            &mut store,
            &json!({"turtle": format!(
                "@prefix ex: <{EX}> .
                 ex:a ex:status \"open\" ; ex:priority 1 ; ex:assignee \"wu\" .
                 ex:b ex:status \"closed\" ; ex:priority 1 ; ex:assignee \"wu\" .
                 ex:c ex:status \"open\" ; ex:priority 2 ; ex:assignee \"wu\" .
                 ex:d ex:status \"open\" ; ex:priority 1 ; ex:assignee \"ian\" ."
            )}),
        )
        .unwrap();
        store.set_projection_defs(vec![def.clone()]);
        store.install_projections(&[def]).unwrap();
    }
    (dir, Arc::new(h))
}

#[tokio::test]
async fn where_combines_column_filters_with_a_grouping() {
    let (_dir, s) = handle();
    // assignee = wu AND priority = 1, grouped by status (aegis-tg5 7mm).
    let a = projections(
        State(s.clone()),
        axum::Json(json!({"action": "aggregate", "name": "items",
            "where": {"assignee": ["wu"], "priority": [1]}, "group_by": "key"})),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(a["total"], 2, "{a}");
    let mut g: Vec<_> = a["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| {
            (
                g["value"].as_str().unwrap().to_string(),
                g["n"].as_i64().unwrap(),
            )
        })
        .collect();
    g.sort();
    assert_eq!(g, vec![("closed".into(), 1), ("open".into(), 1)]);
    for (body, needle) in [
        (
            json!({"name": "items", "where": ["assignee"]}),
            "where must be an object",
        ),
        (
            json!({"name": "items", "where": {"assignee": "wu"}}),
            "must be an array",
        ),
        (
            json!({"name": "items", "where": {"team": ["x"]}}),
            "has no column",
        ),
    ] {
        let err = projections(State(s.clone()), axum::Json(body))
            .await
            .unwrap_err();
        assert!(format!("{err:?}").contains(needle), "{err:?}");
    }
}

#[tokio::test]
async fn two_dimensions_and_the_rows_cursor_round_trip_through_the_handler() {
    let (_dir, s) = handle();
    let a = projections(
        State(s.clone()),
        axum::Json(json!({"action": "aggregate", "name": "items",
            "group_by": ["key", "priority"]})),
    )
    .await
    .unwrap()
    .0;
    let mut cells: Vec<_> = a["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| (g["values"].clone(), g["n"].as_i64().unwrap()))
        .collect();
    cells.sort_by_key(|c| c.0.to_string());
    assert_eq!(
        cells,
        vec![
            (json!(["closed", 1]), 1),
            (json!(["open", 1]), 2),
            (json!(["open", 2]), 1)
        ]
    );
    // Page one row at a time, feeding next_after back verbatim.
    let mut seen = Vec::new();
    let mut after = serde_json::Value::Null;
    loop {
        let p = projections(
            State(s.clone()),
            axum::Json(json!({"action": "rows", "name": "items", "limit": 1, "after": after})),
        )
        .await
        .unwrap()
        .0;
        seen.extend(
            p["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["entity"].clone()),
        );
        if p["truncated"] == false {
            break;
        }
        after = p["next_after"].clone();
    }
    assert_eq!(seen.len(), 4, "{seen:?}");
    for (body, needle) in [
        (
            json!({"name": "items", "group_by": []}),
            "group_by must not be an empty list",
        ),
        (
            json!({"name": "items", "group_by": [{"column": "priority", "bucket": "week"}]}),
            "a group_by dimension is",
        ),
        (
            json!({"name": "items", "until": {"at": "2026-10-21T00:00:00Z"}}),
            "until must be",
        ),
        (
            json!({"action": "rows", "name": "items", "after": {"entity": "http://ex.test/a"}}),
            "after must be a page's next_after",
        ),
    ] {
        let err = projections(State(s.clone()), axum::Json(body))
            .await
            .unwrap_err();
        assert!(format!("{err:?}").contains(needle), "{needle}: {err:?}");
    }
}

#[tokio::test]
async fn list_advertises_rows_and_rows_answers_through_the_handler() {
    let (_dir, s) = handle();
    let l = projections(State(s.clone()), axum::Json(json!({"action": "list"})))
        .await
        .unwrap()
        .0;
    assert_eq!(l["actions"], json!(["aggregate", "list", "rows"]));
    let r = projections(
        State(s.clone()),
        axum::Json(json!({"action": "rows", "name": "items",
            "key_in": ["open"], "where": {"assignee": ["wu"]},
            "columns": ["priority"], "limit": 10})),
    )
    .await
    .unwrap()
    .0;
    let mut got: Vec<_> = r["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["entity"].as_str().unwrap().to_string(),
                r["columns"].clone(),
            )
        })
        .collect();
    got.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        got,
        vec![
            (format!("{EX}a"), json!({"priority": 1})),
            (format!("{EX}c"), json!({"priority": 2})),
        ]
    );
    assert_eq!(r["truncated"], false);
    for (body, needle) in [
        (
            json!({"action": "rows", "name": "items", "graphs": "g"}),
            "graphs must be",
        ),
        (
            json!({"action": "rows", "name": "items", "graphs": []}),
            "graphs must be",
        ),
        (
            json!({"action": "rows", "name": "items", "columns": "priority"}),
            "columns must be",
        ),
        (
            json!({"action": "rows", "name": "items", "limit": -1}),
            "limit must be an integer from 1 to 1000",
        ),
        (
            json!({"action": "rows", "name": "items", "limit": 1001}),
            "limit must be an integer from 1 to 1000",
        ),
        (
            json!({"action": "rows", "name": "items", "limit": 0}),
            "limit must be an integer from 1 to 1000",
        ),
        (
            json!({"action": "rows", "name": "items", "graph": 42}),
            "graph must be graph IRIs",
        ),
        (
            json!({"action": "aggregate", "name": "items", "graph": ""}),
            "graph must be graph IRIs",
        ),
        (
            json!({"action": "rows", "name": "items", "graphs": [""]}),
            "graphs entries must be graph IRIs",
        ),
        (
            json!({"action": "rows", "name": "items", "graphs": ["  "]}),
            "graphs entries must be graph IRIs",
        ),
        (
            json!({"action": "aggregate", "name": "items", "graphs": ["not an IRI"]}),
            "graphs entries must be graph IRIs",
        ),
        (
            json!({"action": "rows", "name": "items", "where": {"priority": [1.5]}}),
            "is not a 64-bit integer",
        ),
        (
            json!({"action": "aggregate", "name": "items", "key_in": [18446744073709551615u64]}),
            "is not a 64-bit integer",
        ),
        (
            json!({"action": "rows", "name": "items", "key_in": "closed"}),
            "key_in must be an array",
        ),
        (
            json!({"action": "aggregate", "name": "items", "since": {"column": "priority"}}),
            "since must be",
        ),
        (
            json!({"action": "nope", "name": "items"}),
            "expected list, aggregate or rows",
        ),
    ] {
        let err = projections(State(s.clone()), axum::Json(body))
            .await
            .unwrap_err();
        assert!(format!("{err:?}").contains(needle), "{needle}: {err:?}");
    }
}
