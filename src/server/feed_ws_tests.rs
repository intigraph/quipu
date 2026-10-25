//! `GET /feed/ws` end to end over a real loopback server: many filters share
//! a few connections, N change subscriptions cost one hub read per commit,
//! frames are the SSE frames byte for byte, resume loses nothing, and a refused
//! message leaves the connection open.
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use quipu::store::{Datum, Store};
use quipu::types::{Op, Value};
use serde_json::{Value as Json, json};
use tokio_tungstenite::tungstenite::Message;

use super::super::feed_hub::Hub;
use crate::{SharedStore, StoreHandle};

const T: &str = "2026-10-25T00:00:00Z";
const BOARD: &str = "urn:graph:board";
const OTHER: &str = "urn:graph:other";

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

fn write(shared: &SharedStore, graph: &str, entity: &str) -> i64 {
    let mut store = shared.lock();
    let datum = Datum {
        entity: store.intern(entity).unwrap(),
        attribute: store.intern("urn:p:title").unwrap(),
        value: Value::Str(format!("title of {entity}")),
        valid_from: T.into(),
        valid_to: None,
        op: Op::Assert,
    };
    let g = match store.registered_graph_id(graph).unwrap() {
        Some(g) => g,
        None => store.graph_create(graph).unwrap(),
    };
    store.transact_to_graph(&[datum], T, None, None, g).unwrap()
}

/// A store with both graphs registered, served on a loopback port.
async fn serve() -> (SharedStore, String) {
    let shared: SharedStore = Arc::new(StoreHandle::writer_only(Store::open_in_memory().unwrap()));
    write(&shared, BOARD, "urn:seed");
    write(&shared, OTHER, "urn:x:seed");
    let app = axum::Router::new()
        .route("/feed/ws", axum::routing::get(super::feed_ws))
        .route(
            "/changes/stream",
            axum::routing::get(super::super::changes_stream),
        )
        .with_state(shared.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (shared, format!("ws://{addr}/feed/ws"))
}

async fn connect(url: &str) -> Socket {
    tokio_tungstenite::connect_async(url).await.unwrap().0
}

async fn send(socket: &mut Socket, message: Json) {
    socket
        .send(Message::Text(message.to_string().into()))
        .await
        .unwrap();
}

/// The next non-heartbeat message, within `within`.
async fn next(socket: &mut Socket, within: Duration) -> Option<Json> {
    let deadline = tokio::time::Instant::now() + within;
    loop {
        let left = deadline.checked_duration_since(tokio::time::Instant::now())?;
        let message = tokio::time::timeout(left, socket.next())
            .await
            .ok()??
            .ok()?;
        if let Message::Text(text) = message {
            let json: Json = serde_json::from_str(text.as_str()).unwrap();
            if json["event"] != "quipu.heartbeat" {
                return Some(json);
            }
        }
    }
}

fn subscribe(sub: &str, since: i64, graph: &str) -> Json {
    json!({"op": "subscribe", "sub": sub, "feed": "changes", "since": since, "graph": graph})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ninety_filters_share_three_connections() {
    let (shared, url) = serve().await;
    let since = shared.read().latest_tx_id().unwrap();
    let mut sockets = Vec::new();
    for c in 0..3 {
        let mut socket = connect(&url).await;
        for s in 0..30 {
            send(&mut socket, subscribe(&format!("c{c}s{s}"), since, BOARD)).await;
        }
        sockets.push(socket);
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let tx = write(&shared, BOARD, "urn:item:1");
    for socket in &mut sockets {
        let mut seen = std::collections::BTreeSet::new();
        while seen.len() < 30 {
            let frame = next(socket, Duration::from_secs(5)).await.expect("a frame");
            assert_eq!(frame["event"], "quipu.changes", "{frame}");
            assert_eq!(frame["id"], tx.to_string());
            seen.insert(frame["sub"].as_str().unwrap().to_string());
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn twenty_change_subscriptions_cost_one_hub_read_per_commit() {
    let (shared, url) = serve().await;
    let since = shared.read().latest_tx_id().unwrap();
    let mut socket = connect(&url).await;
    for s in 0..20 {
        send(&mut socket, subscribe(&format!("s{s}"), since, BOARD)).await;
    }
    // Every subscription attaches to the hub after its first store page.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let hub = Hub::of(&shared);
    let reads = || hub.reads.load(std::sync::atomic::Ordering::Relaxed);
    let catchup = || hub.catchup_reads.load(std::sync::atomic::Ordering::Relaxed);
    let (before, before_catchup) = (reads(), catchup());
    for i in 0..3 {
        write(&shared, BOARD, &format!("urn:item:{i}"));
        for _ in 0..20 {
            next(&mut socket, Duration::from_secs(5))
                .await
                .expect("a frame");
        }
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    // A write commits once; a late hint may wake one empty read per commit.
    let hub_reads = reads() - before;
    assert!((3..=6).contains(&hub_reads), "hub reads {hub_reads}");
    assert_eq!(
        catchup(),
        before_catchup,
        "a subscriber read the store itself"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_frame_is_the_sse_frame_byte_for_byte() {
    let (shared, url) = serve().await;
    let since = shared.read().latest_tx_id().unwrap();
    let tx = write(&shared, BOARD, "urn:item:1");
    let mut socket = connect(&url).await;
    send(&mut socket, subscribe("board", since, BOARD)).await;
    let frame = next(&mut socket, Duration::from_secs(5)).await.unwrap();
    assert_eq!(frame["id"], tx.to_string());

    // The same cursor over SSE, from the store.
    let page = crate::feed::changes_get(
        axum::extract::State(shared.clone()),
        axum::http::HeaderMap::new(),
        axum::extract::Query(
            serde_json::from_value(json!({"since": since, "limit": 1,
                "capture": "old_and_new_values", "graph": BOARD}))
            .unwrap(),
        ),
    )
    .await
    .unwrap()
    .0;
    let stable = |page: &Json| {
        json!({"records": page["records"], "since": page["since"], "next_tx": page["next_tx"],
               "capture": page["capture"], "source": page["source"]})
    };
    let sent: Json = serde_json::from_str(frame["data"].as_str().unwrap()).unwrap();
    assert_eq!(stable(&sent), stable(&page));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resuming_from_the_last_cursor_loses_nothing() {
    let (shared, url) = serve().await;
    let start = shared.read().latest_tx_id().unwrap();
    let mut txs = Vec::new();
    for i in 0..3 {
        txs.push(write(&shared, BOARD, &format!("urn:item:{i}")));
    }
    let mut socket = connect(&url).await;
    send(&mut socket, subscribe("board", start, BOARD)).await;
    let first = next(&mut socket, Duration::from_secs(5)).await.unwrap();
    let last: i64 = first["id"].as_str().unwrap().parse().unwrap();
    drop(socket);
    // More commits while disconnected.
    for i in 3..5 {
        txs.push(write(&shared, BOARD, &format!("urn:item:{i}")));
    }
    let mut socket = connect(&url).await;
    send(&mut socket, subscribe("board", last, BOARD)).await;
    let mut got = vec![last];
    while got.len() < txs.len() {
        let frame = next(&mut socket, Duration::from_secs(5))
            .await
            .expect("a frame");
        got.push(frame["id"].as_str().unwrap().parse().unwrap());
    }
    assert_eq!(got, txs);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refused_message_leaves_the_connection_open() {
    let (shared, url) = serve().await;
    let since = shared.read().latest_tx_id().unwrap();
    let mut socket = connect(&url).await;
    for (message, why) in [
        (json!({"op": "dance"}), "unknown op"),
        (
            json!({"op": "subscribe", "sub": "x", "feed": "bogus"}),
            "feed",
        ),
        (
            json!({"op": "subscribe", "sub": "x", "feed": "changes", "types": ["t"]}),
            "scope",
        ),
        (json!({"op": "unsubscribe", "sub": "never"}), "no such"),
    ] {
        send(&mut socket, message).await;
        let reply = next(&mut socket, Duration::from_secs(5)).await.unwrap();
        assert_eq!(reply["event"], "quipu.error", "{why}: {reply}");
    }
    send(&mut socket, subscribe("board", since, BOARD)).await;
    send(&mut socket, subscribe("board", since, BOARD)).await;
    let duplicate = next(&mut socket, Duration::from_secs(5)).await.unwrap();
    assert_eq!(duplicate["event"], "quipu.error");
    assert_eq!(duplicate["sub"], "board");
    // The connection still serves the open subscription.
    let tx = write(&shared, BOARD, "urn:item:1");
    let frame = next(&mut socket, Duration::from_secs(5)).await.unwrap();
    assert_eq!(frame["id"], tx.to_string());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_connection_holds_at_most_thirty_two_subscriptions() {
    let (shared, url) = serve().await;
    let since = shared.read().latest_tx_id().unwrap();
    let mut socket = connect(&url).await;
    for s in 0..32 {
        send(&mut socket, subscribe(&format!("s{s}"), since, BOARD)).await;
    }
    send(&mut socket, subscribe("one-too-many", since, BOARD)).await;
    let refused = next(&mut socket, Duration::from_secs(5)).await.unwrap();
    assert_eq!(refused["event"], "quipu.error");
    assert_eq!(refused["sub"], "one-too-many");
    // Unsubscribing frees a place.
    send(&mut socket, json!({"op": "unsubscribe", "sub": "s0"})).await;
    send(&mut socket, subscribe("now-fits", since, OTHER)).await;
    let tx = write(&shared, OTHER, "urn:x:1");
    let frame = next(&mut socket, Duration::from_secs(5)).await.unwrap();
    assert_eq!(frame["sub"], "now-fits");
    assert_eq!(frame["id"], tx.to_string());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn idle_subscriptions_cost_no_reads() {
    let (shared, url) = serve().await;
    let since = shared.read().latest_tx_id().unwrap();
    let mut socket = connect(&url).await;
    for s in 0..10 {
        send(&mut socket, subscribe(&format!("s{s}"), since, BOARD)).await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    let hub = Hub::of(&shared);
    let reads = || hub.reads.load(std::sync::atomic::Ordering::Relaxed);
    let catchup = || hub.catchup_reads.load(std::sync::atomic::Ordering::Relaxed);
    let (before, before_catchup) = (reads(), catchup());
    // No commits: nothing is read, whatever the number of subscribers.
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(reads(), before, "an idle hub read the store");
    assert_eq!(
        catchup(),
        before_catchup,
        "an idle subscriber read the store"
    );
    assert!(
        next(&mut socket, Duration::from_millis(200))
            .await
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_transaction_over_the_frame_budget_crosses_whole_and_byte_exact() {
    use sha2::{Digest, Sha256};
    let (shared, url) = serve().await;
    let since = shared.read().latest_tx_id().unwrap();
    let mut socket = connect(&url).await;
    send(&mut socket, subscribe("board", since, BOARD)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    // ~1.5 MB in one transaction: over the 1 MiB frame budget.
    let tx = {
        let mut store = shared.lock();
        let g = store.registered_graph_id(BOARD).unwrap().unwrap();
        let datums: Vec<Datum> = (0..30)
            .map(|i| Datum {
                entity: store.intern(&format!("urn:big:{i}")).unwrap(),
                attribute: store.intern("urn:p:body").unwrap(),
                value: Value::Str("x".repeat(50_000)),
                valid_from: T.into(),
                valid_to: None,
                op: Op::Assert,
            })
            .collect();
        store.transact_to_graph(&datums, T, None, None, g).unwrap()
    };
    let begin = next(&mut socket, Duration::from_secs(10)).await.unwrap();
    assert_eq!(begin["event"], "quipu.changes.begin", "{begin}");
    assert!(begin.get("id").is_none(), "only the barrier carries an id");
    let mut hasher = Sha256::new();
    let mut parts = 0;
    let commit = loop {
        let frame = next(&mut socket, Duration::from_secs(10)).await.unwrap();
        match frame["event"].as_str().unwrap() {
            "quipu.changes.part" => {
                assert!(frame.get("id").is_none());
                hasher.update(frame["data"].as_str().unwrap().as_bytes());
                parts += 1;
            }
            "quipu.changes.commit" => break frame,
            other => panic!("unexpected {other}"),
        }
    };
    assert!(parts >= 2, "{parts} parts");
    assert_eq!(commit["id"], tx.to_string());
    let summary: Json = serde_json::from_str(commit["data"].as_str().unwrap()).unwrap();
    assert_eq!(summary["parts"], parts);
    assert_eq!(summary["digest"], hex::encode(hasher.finalize()));
}
