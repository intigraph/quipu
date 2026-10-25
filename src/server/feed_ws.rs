//! `GET /feed/ws`: the change and event feeds over one WebSocket
//! (docs/design/event-push.md §3).
//!
//! A connection holds ONE of the 64 stream slots, however many subscriptions
//! it carries, so more than 64 filters fit in 64 connections. Each
//! subscription is the same transport-neutral cursor the SSE streams use
//! (`feed_stream::Cursor`): a change subscription is served from the shared
//! reader (`feed_hub`), and its frames are byte-for-byte the SSE frames. The
//! `data` of every frame is sent as the SSE data STRING, unparsed, because a
//! chunked transaction's part digests are hashes of those exact bytes.
//!
//! Client to server, one JSON text message each:
//!   {"op": "subscribe", "sub": "board", "feed": "changes", "since": 713600, "graph": "<iri>"}
//!   {"op": "subscribe", "sub": "items", "feed": "events", "since": 4100, "types": ["entity.updated"]}
//!   {"op": "unsubscribe", "sub": "board"}
//! Server to client:
//!   {"sub": "board", "event": "quipu.changes", "id": "713601", "data": "<the SSE data>"}
//!   {"event": "quipu.heartbeat", "ts": <Unix seconds>}
//!   {"sub": "items", "event": "quipu.error", "data": "<reason>", "last": 4120}
//!
//! Resume is per subscription: re-subscribe with `since` = the last applied
//! cursor. The server keeps no session state. A refused subscribe or an
//! unknown `op` answers that message with `quipu.error`; the connection stays.
//! The heartbeat proves the connection only, never freshness.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use tokio::sync::{OwnedSemaphorePermit, mpsc};

use super::{Cursor, FeedFrame, Params, scope_ok, stream_slot};
use crate::SharedStore;
use crate::event_cursor::resume_offset;

/// Subscriptions one connection may hold at once.
const MAX_PER_CONNECTION: usize = 32;
/// Subscriptions the process holds at once, over every connection.
const MAX_SUBSCRIPTIONS: usize = 1_024;
/// A connection heartbeat, from a timer; it never reads the store.
const HEARTBEAT: Duration = Duration::from_secs(15);
/// Frames buffered for a slow client before its subscriptions wait.
const OUTBOX: usize = 64;
/// The longest subscription name.
const MAX_SUB_LEN: usize = 128;

static SUBSCRIPTIONS: AtomicUsize = AtomicUsize::new(0);

/// One of the process-wide subscriptions, released when dropped. It lives in
/// the subscription's task, so a subscription that ends on its own (after its
/// last error) frees its place at once, not when its connection next speaks.
struct Held(&'static AtomicUsize);

impl Held {
    fn take(quota: &'static AtomicUsize) -> Option<Self> {
        // A plain CAS loop: current stable deprecates `fetch_update` (renamed
        // `try_update`), which older toolchains do not have.
        let mut n = quota.load(Ordering::Acquire);
        while n < MAX_SUBSCRIPTIONS {
            match quota.compare_exchange_weak(n, n + 1, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return Some(Self(quota)),
                Err(current) => n = current,
            }
        }
        None
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Subscription {
    task: tokio::task::JoinHandle<()>,
}

pub(crate) async fn feed_ws(
    State(store): State<SharedStore>,
    upgrade: WebSocketUpgrade,
) -> Response {
    let Some(slot) = stream_slot() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "stream capacity exhausted; reconnect later",
        )
            .into_response();
    };
    upgrade.on_upgrade(move |socket| serve(store, socket, slot))
}

async fn serve(store: SharedStore, mut socket: WebSocket, _slot: OwnedSemaphorePermit) {
    let (outbox, mut frames) = mpsc::channel::<String>(OUTBOX);
    let mut subs: HashMap<String, Subscription> = HashMap::new();
    let mut heartbeat = tokio::time::interval(HEARTBEAT);
    heartbeat.tick().await;
    loop {
        tokio::select! {
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    if let Some(reply) =
                        handle(&store, text.as_str(), &mut subs, &outbox, &SUBSCRIPTIONS)
                        && socket.send(Message::Text(reply.into())).await.is_err()
                    {
                        break;
                    }
                }
                Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                // Pings are answered by the transport; binary is not a message.
                Some(Ok(_)) => {}
            },
            Some(frame) = frames.recv() => {
                if socket.send(Message::Text(frame.into())).await.is_err() {
                    break;
                }
            }
            _ = heartbeat.tick() => {
                let beat = json!({"event": "quipu.heartbeat", "ts": quipu::time::epoch_secs()})
                    .to_string();
                if socket.send(Message::Text(beat.into())).await.is_err() {
                    break;
                }
            }
        }
    }
    for sub in subs.into_values() {
        sub.task.abort();
    }
}

fn refusal(sub: Option<&str>, reason: &str) -> String {
    let mut message = json!({"event": "quipu.error", "data": reason});
    if let Some(sub) = sub {
        message["sub"] = json!(sub);
    }
    message.to_string()
}

/// Apply one client message; the reply to send for it, if any.
fn handle(
    store: &SharedStore,
    text: &str,
    subs: &mut HashMap<String, Subscription>,
    outbox: &mpsc::Sender<String>,
    quota: &'static AtomicUsize,
) -> Option<String> {
    let Ok(message) = serde_json::from_str::<Value>(text) else {
        return Some(refusal(None, "message is not JSON"));
    };
    let sub = message["sub"].as_str().map(str::to_string);
    match message["op"].as_str() {
        Some("subscribe") => subscribe(store, &message, subs, outbox, quota).err(),
        Some("unsubscribe") => {
            let Some(sub) = sub else {
                return Some(refusal(None, "unsubscribe names no sub"));
            };
            match subs.remove(&sub) {
                Some(removed) => {
                    removed.task.abort();
                    None
                }
                None => Some(refusal(Some(&sub), "no such subscription")),
            }
        }
        _ => Some(refusal(sub.as_deref(), "unknown op")),
    }
}

fn subscribe(
    store: &SharedStore,
    message: &Value,
    subs: &mut HashMap<String, Subscription>,
    outbox: &mpsc::Sender<String>,
    quota: &'static AtomicUsize,
) -> Result<(), String> {
    let Some(sub) = message["sub"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= MAX_SUB_LEN)
    else {
        return Err(refusal(None, "subscribe needs a sub of 1 to 128 bytes"));
    };
    let fail = |reason: &str| Err(refusal(Some(sub), reason));
    // Ended subscriptions (after their last error) free their place.
    subs.retain(|_, s| !s.task.is_finished());
    if subs.contains_key(sub) {
        return fail("a subscription with this sub is open");
    }
    let changes = match message["feed"].as_str() {
        Some("changes") => true,
        Some("events") => false,
        _ => return fail("feed must be changes or events"),
    };
    let types = match &message["types"] {
        Value::Null => None,
        Value::Array(list) => {
            let names: Option<Vec<&str>> = list.iter().map(Value::as_str).collect();
            match names {
                Some(names) => Some(names.join(",")),
                None => return fail("types must be strings"),
            }
        }
        _ => return fail("types must be a list"),
    };
    let since = match &message["since"] {
        Value::Null => None,
        value => match value.as_i64() {
            Some(since) => Some(since),
            None => return fail("since must be an integer cursor"),
        },
    };
    let params: Params = match serde_json::from_value(json!({
        "since": since, "types": types,
        "group": message.get("group"), "graph": message.get("graph"),
    })) {
        Ok(params) => params,
        Err(_) => return fail("unsupported feed scope"),
    };
    if !scope_ok(&params, changes) {
        return fail("unsupported or oversized feed scope");
    }
    let offset = match resume_offset(since, None) {
        Ok(offset) => offset,
        Err(reason) => return fail(reason),
    };
    if subs.len() >= MAX_PER_CONNECTION {
        return fail("too many subscriptions on this connection");
    }
    let Some(held) = Held::take(quota) else {
        return fail("subscription capacity exhausted; retry later");
    };
    let mut cursor = Cursor::open(store.clone(), offset, params, changes, None);
    let name = sub.to_string();
    let outbox = outbox.clone();
    let task = tokio::spawn(async move {
        let _held = held;
        while let Some(frame) = cursor.next_frame().await {
            let message = encode(&name, &frame, cursor.offset());
            if outbox.send(message).await.is_err() {
                return;
            }
        }
    });
    subs.insert(sub.to_string(), Subscription { task });
    Ok(())
}

/// One frame as a WebSocket message. An error names the last cursor the
/// subscription delivered, so the client knows where to resume.
fn encode(sub: &str, frame: &FeedFrame, last: i64) -> String {
    let mut message = json!({"sub": sub, "event": frame.event, "data": frame.data});
    if let Some(id) = frame.id {
        message["id"] = json!(id.to_string());
    }
    if frame.event == "quipu.error" {
        message["last"] = json!(last);
    }
    message.to_string()
}

#[cfg(test)]
#[path = "feed_ws_tests.rs"]
mod tests;
