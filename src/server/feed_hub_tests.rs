//! The change feed's shared reader: one store read per committed transaction
//! whatever the number of subscribers, pages identical to a filtered store
//! read, catch-up behind the ring, and an emptied ring on a new log epoch.
use std::sync::Arc;
use std::time::Duration;

use quipu::store::changes::Capture;
use quipu::store::{Datum, Store};
use quipu::types::{Op, Value};
use serde_json::Value as Json;

use super::{Hub, Serve};
use crate::{SharedStore, StoreHandle};

const T: &str = "2026-10-24T00:00:00Z";
const BOARD: &str = "urn:graph:board";
const OTHER: &str = "urn:graph:other";

fn write(shared: &SharedStore, graph: &str, entity: &str) {
    let mut store = shared.lock();
    let datum = Datum {
        entity: store.intern(entity).unwrap(),
        attribute: store.intern("urn:p:title").unwrap(),
        value: Value::Str(format!("title of {entity}")),
        valid_from: T.into(),
        valid_to: None,
        op: Op::Assert,
    };
    // Reuse a registered graph: graph_create commits (and hints) on its own,
    // which would make one write two commits.
    let g = match store.registered_graph_id(graph).unwrap() {
        Some(g) => g,
        None => store.graph_create(graph).unwrap(),
    };
    store.transact_to_graph(&[datum], T, None, None, g).unwrap();
}

fn shared() -> SharedStore {
    let shared: SharedStore = Arc::new(StoreHandle::writer_only(Store::open_in_memory().unwrap()));
    write(&shared, BOARD, "urn:seed");
    write(&shared, OTHER, "urn:x:seed");
    shared
}

/// Wait until the hub's ring head reaches the store's latest transaction.
async fn settled(shared: &SharedStore, hub: &Hub) {
    let latest = shared.read().latest_tx_id().unwrap();
    let mut head = hub.watch();
    for _ in 0..200 {
        if *head.borrow_and_update() >= latest {
            return;
        }
        let _ = tokio::time::timeout(Duration::from_millis(50), head.changed()).await;
    }
    panic!("hub head never reached {latest}");
}

/// Wait until the hub's reader has gone quiet: no read for 100 ms. A write can
/// commit more than one transaction, and a late hint wakes one more (empty)
/// read; measuring before it ends would charge it to the wrong step.
async fn quiet(hub: &Hub) {
    let reads = || hub.reads.load(std::sync::atomic::Ordering::Relaxed);
    let mut last = reads();
    loop {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let now = reads();
        if now == last {
            return;
        }
        last = now;
    }
}

/// The page fields that do not depend on when the page was read.
fn stable(page: &Json) -> Json {
    serde_json::json!({
        "records": page["records"], "since": page["since"], "next_tx": page["next_tx"],
        "capture": page["capture"], "source": page["source"],
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ten_subscribers_cost_one_store_read_per_commit() {
    let shared = shared();
    let hub = Hub::of(&shared);
    settled(&shared, &hub).await;
    quiet(&hub).await;
    let mut cursors = [shared.read().latest_tx_id().unwrap(); 10];
    let mut scanned = [0; 10];
    let reads = || hub.reads.load(std::sync::atomic::Ordering::Relaxed);
    for i in 0..5 {
        let before = reads();
        write(&shared, BOARD, &format!("urn:item:{i}"));
        settled(&shared, &hub).await;
        quiet(&hub).await;
        assert_eq!(
            reads() - before,
            1,
            "commit {i}: one store read, whatever the number of subscribers"
        );
        // Serving all ten subscribers reads nothing.
        for (cursor, scanned) in cursors.iter_mut().zip(scanned.iter_mut()) {
            let Serve::Page(page) = hub.serve(*cursor, Some(BOARD), scanned) else {
                panic!("subscriber not served from the ring");
            };
            *cursor = page["next_tx"].as_i64().unwrap();
        }
        assert_eq!(reads() - before, 1);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_ring_page_equals_the_filtered_store_page() {
    let shared = shared();
    let hub = Hub::of(&shared);
    settled(&shared, &hub).await;
    let start = shared.read().latest_tx_id().unwrap();
    write(&shared, OTHER, "urn:x:1");
    write(&shared, BOARD, "urn:item:1");
    write(&shared, OTHER, "urn:x:2");
    settled(&shared, &hub).await;
    for graph in [Some(BOARD), Some(OTHER), None] {
        let mut cursor = start;
        let mut scanned = 0;
        loop {
            let served = hub.serve(cursor, graph, &mut scanned);
            let store = shared.read();
            let g = graph.map(|iri| store.lookup(iri).unwrap().unwrap());
            let want = store
                .changes_after(cursor, 1, Capture::OldAndNewValues, g)
                .unwrap()
                .to_json();
            match served {
                Serve::Page(page) => {
                    assert_eq!(stable(&page), stable(&want), "graph {graph:?} at {cursor}");
                    cursor = page["next_tx"].as_i64().unwrap();
                }
                Serve::Wait => {
                    assert!(want["records"].as_array().unwrap().is_empty());
                    break;
                }
                Serve::CatchUp => panic!("a cursor inside the ring must not catch up"),
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cursor_behind_the_ring_catches_up_from_the_store() {
    let shared = shared();
    let hub = Hub::of(&shared);
    settled(&shared, &hub).await;
    let mut scanned = 0;
    // The seed transaction predates the hub: the ring starts after it.
    assert!(matches!(
        hub.serve(0, Some(BOARD), &mut scanned),
        Serve::CatchUp
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn other_graphs_never_push_a_subscriber_into_catch_up() {
    let shared = shared();
    let hub = Hub::of(&shared);
    settled(&shared, &hub).await;
    let cursor = shared.read().latest_tx_id().unwrap();
    let mut scanned = 0;
    for i in 0..3 {
        write(&shared, OTHER, &format!("urn:x:{i}"));
        settled(&shared, &hub).await;
        assert!(matches!(
            hub.serve(cursor, Some(BOARD), &mut scanned),
            Serve::Wait
        ));
    }
    assert_eq!(scanned, shared.read().latest_tx_id().unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_log_epoch_empties_the_ring() {
    let shared = shared();
    let hub = Hub::of(&shared);
    settled(&shared, &hub).await;
    let cursor = shared.read().latest_tx_id().unwrap();
    write(&shared, BOARD, "urn:item:1");
    settled(&shared, &hub).await;
    let mut scanned = 0;
    assert!(matches!(
        hub.serve(cursor, Some(BOARD), &mut scanned),
        Serve::Page(_)
    ));
    shared
        .lock()
        .mint_log_epoch("test: history removed")
        .unwrap();
    write(&shared, BOARD, "urn:item:2");
    settled(&shared, &hub).await;
    let mut scanned = 0;
    assert!(
        matches!(hub.serve(cursor, Some(BOARD), &mut scanned), Serve::CatchUp),
        "a page of the old epoch was served from memory"
    );
}

/// A real `/changes/stream` subscriber; its frames, as `(event, id)`.
fn subscribe(
    shared: &SharedStore,
    since: i64,
    graph: &str,
) -> tokio::sync::mpsc::Receiver<(String, String)> {
    use futures::StreamExt;
    let params =
        serde_json::from_value(serde_json::json!({"since": since, "graph": graph})).unwrap();
    let (frames, received) = tokio::sync::mpsc::channel(64);
    let shared = shared.clone();
    tokio::spawn(async move {
        let response = super::super::changes_stream(
            axum::extract::State(shared),
            axum::http::HeaderMap::new(),
            axum::extract::Query(params),
        )
        .await;
        let mut body = response.into_body().into_data_stream();
        let mut buffer = String::new();
        while let Some(Ok(chunk)) = body.next().await {
            buffer.push_str(std::str::from_utf8(&chunk).unwrap());
            while let Some(end) = buffer.find("\n\n") {
                let block: String = buffer.drain(..end + 2).collect();
                let field = |name: &str| {
                    block
                        .lines()
                        .find_map(|l| l.strip_prefix(name))
                        .unwrap_or_default()
                        .to_string()
                };
                let event = field("event: ");
                if !event.is_empty() && frames.send((event, field("id: "))).await.is_err() {
                    return;
                }
            }
        }
    });
    received
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_quiet_graph_behind_the_ring_floor_stops_reading_the_store() {
    // BOARD's last transaction predates the hub; OTHER moves the store past
    // it, so the ring's floor is above the subscriber's cursor.
    let shared = shared();
    let board_last = shared.read().latest_tx_id().unwrap() - 1;
    write(&shared, OTHER, "urn:x:before");
    let hub = Hub::of(&shared);
    settled(&shared, &hub).await;
    let mut frames = subscribe(&shared, board_last, BOARD);
    quiet(&hub).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let catchup = || hub.catchup_reads.load(std::sync::atomic::Ordering::Relaxed);
    let before = catchup();

    // Commits in other graphs only: served from the ring, no store read.
    for i in 0..3 {
        write(&shared, OTHER, &format!("urn:x:{i}"));
        settled(&shared, &hub).await;
    }
    quiet(&hub).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        catchup(),
        before,
        "unrelated commits sent the subscriber to the store"
    );

    // And nothing in scope is lost: the next BOARD commit arrives.
    write(&shared, BOARD, "urn:item:after");
    let board_tx = shared.read().latest_tx_id().unwrap();
    let frame = tokio::time::timeout(Duration::from_secs(5), frames.recv())
        .await
        .expect("the BOARD commit never arrived")
        .unwrap();
    assert_eq!(frame, ("quipu.changes".to_string(), board_tx.to_string()));
}
