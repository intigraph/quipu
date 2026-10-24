//! Named-graph events on their own routes (event-push design §4, §7).
//!
//! A server that predates named-graph events ignores an unknown `graph`
//! parameter: `/events?graph=G` would answer with ROOT's events, and
//! `/events/commit {graph}` would upsert the ROOT cursor of that consumer id
//! before answering. It answers an unknown ROUTE 404 and writes nothing. So
//! named graphs read and commit only at `/events/graph` and
//! `/events/graph/commit`, and the ROOT routes refuse `graph`, so no client
//! learns to depend on the unsafe form.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use serde_json::{Value, json};

use crate::feed;

const BOARD: &str = "https://example.org/graph/board";
const TS: &str = "2026-10-24T00:00:00Z";

fn store_with_root_and_board_events() -> crate::SharedStore {
    let (_dir, handle) = crate::tests::pooled_handle(0);
    std::mem::forget(_dir);
    {
        let mut s = handle.lock();
        quipu::rdf::ingest_rdf(
            &mut s,
            b"<urn:root:item> <urn:p:x> <urn:o:1> .".as_slice(),
            oxrdfio::RdfFormat::Turtle,
            None,
            TS,
            None,
            None,
        )
        .unwrap();
        s.graph_create_recorded(BOARD, TS, None).unwrap();
        let g = s.committed_graph_id(BOARD).unwrap().unwrap();
        let d = quipu::store::Datum {
            entity: s.intern("urn:board:item").unwrap(),
            attribute: s.intern("urn:p:x").unwrap(),
            value: quipu::Value::Ref(s.intern("urn:o:1").unwrap()),
            valid_from: TS.into(),
            valid_to: None,
            op: quipu::Op::Assert,
        };
        s.transact_to_graph(&[d], TS, None, None, g).unwrap();
    }
    Arc::new(handle)
}

async fn get(s: &crate::SharedStore, graph_route: bool, q: Value) -> Result<Value, String> {
    let p = serde_json::from_value(q).unwrap();
    let r = if graph_route {
        feed::events_graph_get(State(s.clone()), HeaderMap::new(), Query(p)).await
    } else {
        feed::events_get(State(s.clone()), HeaderMap::new(), Query(p)).await
    };
    r.map(|j| j.0).map_err(|e| format!("{e:?}"))
}

async fn commit(s: &crate::SharedStore, graph_route: bool, body: Value) -> Result<Value, String> {
    let r = if graph_route {
        feed::events_graph_commit(State(s.clone()), axum::Json(body)).await
    } else {
        feed::events_commit(State(s.clone()), axum::Json(body)).await
    };
    r.map(|j| j.0).map_err(|e| format!("{e:?}"))
}

fn subjects(batch: &Value) -> Vec<String> {
    batch["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["subject"].as_str().map(String::from))
        .collect()
}

#[tokio::test]
async fn each_route_reads_only_its_own_log() {
    let s = store_with_root_and_board_events();
    let root = get(&s, false, json!({"since": 0})).await.unwrap();
    assert!(
        subjects(&root).iter().all(|x| !x.contains("urn:board")),
        "{root}"
    );
    let board = get(&s, true, json!({"graph": BOARD, "since": 0}))
        .await
        .unwrap();
    let board_subjects = subjects(&board);
    assert!(
        !board_subjects.is_empty() && board_subjects.iter().all(|x| x == "urn:board:item"),
        "{board}"
    );
    assert!(
        board["events"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["graph"] == BOARD)
    );
}

#[tokio::test]
async fn the_root_routes_refuse_graph_and_the_graph_routes_require_it() {
    let s = store_with_root_and_board_events();
    let e = get(&s, false, json!({"graph": BOARD})).await.unwrap_err();
    assert!(e.contains("/events/graph"), "{e}");
    let e = get(&s, true, json!({"since": 0})).await.unwrap_err();
    assert!(e.contains("requires graph"), "{e}");
    let e = commit(&s, true, json!({"consumer_id": "c", "offset": 1}))
        .await
        .unwrap_err();
    assert!(e.contains("requires graph"), "{e}");
    let e = get(
        &s,
        true,
        json!({"graph": "https://example.org/graph/unregistered"}),
    )
    .await
    .unwrap_err();
    assert!(e.contains("not a registered committed named graph"), "{e}");
}

/// ian's control, the new side: a ROOT reader at cursor 1 and a board reader
/// of the SAME consumer id. Neither the refused ROOT commit with `graph` nor
/// the graph commit may move the ROOT cursor.
#[tokio::test]
async fn a_graph_commit_never_moves_the_same_consumers_root_cursor() {
    let s = store_with_root_and_board_events();
    commit(&s, false, json!({"consumer_id": "reader", "offset": 1}))
        .await
        .unwrap();
    let e = commit(
        &s,
        false,
        json!({"consumer_id": "reader", "offset": 102, "graph": BOARD}),
    )
    .await
    .unwrap_err();
    assert!(e.contains("/events/graph/commit"), "{e}");
    let ok = commit(
        &s,
        true,
        json!({"consumer_id": "reader", "graph": BOARD, "offset": 102}),
    )
    .await
    .unwrap();
    assert_eq!(ok["graph"], BOARD);
    let root = get(&s, false, json!({"consumer": "reader"})).await.unwrap();
    assert_eq!(root["committed_offset"], 1, "ROOT cursor unmoved: {root}");
    let board = get(&s, true, json!({"graph": BOARD, "consumer": "reader"}))
        .await
        .unwrap();
    assert_eq!(board["committed_offset"], 102, "{board}");
}
