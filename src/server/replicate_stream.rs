//! `POST /replicate/stream` and `POST /replicate/challenge`: the scoped
//! replication stream (docs/design/local-sync-mode.md §5) over SSE.
//!
//! The request body names the scope as an as-of share does (`graphs`,
//! `context_graphs`, `expected_store_id`) plus the follower's cursor `since`.
//! It is a POST for the same reason as `/share/asof`: graph IRIs are arbitrary
//! and do not belong in a query string.
//!
//! Each session runs one producer task. It polls the store's change log in
//! the session's scope (`quipu::replicate_source::Session`) whenever a commit
//! is hinted, a challenge arrives or a heartbeat is due, and hands frames to
//! the response through a bounded channel. A client that goes away closes the
//! channel, which ends the task and unregisters the session.
//!
//! A challenge is delivered to its session's task, which answers it from a
//! read made after receipt (`Session::poll`). The session id is the only
//! capability a challenge needs: it is 128 random bits, sent only on the
//! stream it names.

use std::collections::{HashMap, VecDeque};
use std::convert::Infallible;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response, Sse, sse::Event},
};
use futures::stream::unfold;
use quipu::asof_share::CutError;
use quipu::replicate_source::{Frame, Session, StreamRequest};
use serde_json::json;
use tokio::sync::mpsc;

use crate::{SharedStore, base::AppError, feed_stream::stream_slot};

/// Transactions read per poll.
const POLL_TXS: usize = 64;
/// A signed coverage frame is sent at least this often while idle.
const HEARTBEAT: Duration = Duration::from_secs(15);
/// Frames buffered for a slow follower before the producer waits.
const FRAME_BUFFER: usize = 64;
/// Challenges a session holds unanswered before further ones are refused.
const CHALLENGE_BUFFER: usize = 8;

type Registry = Mutex<HashMap<String, mpsc::Sender<[u8; 16]>>>;

fn sessions() -> &'static Registry {
    static SESSIONS: OnceLock<Registry> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Unregisters a session when its producer task ends, however it ends.
struct Registered(String);

impl Drop for Registered {
    fn drop(&mut self) {
        if let Ok(mut map) = sessions().lock() {
            map.remove(&self.0);
        }
    }
}

fn unavailable(error: &str) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [(axum::http::header::RETRY_AFTER, "60")],
        axum::Json(json!({"error": error, "retry_after_secs": 60})),
    )
        .into_response()
}

fn session_id() -> Option<String> {
    use ring::rand::SecureRandom as _;
    let mut bytes = [0u8; 16];
    ring::rand::SystemRandom::new().fill(&mut bytes).ok()?;
    Some(hex::encode(bytes))
}

fn sse(frame: Frame) -> Event {
    Event::default().event(frame.event).data(frame.data)
}

pub(crate) async fn stream(
    State(store): State<SharedStore>,
    axum::Json(request): axum::Json<StreamRequest>,
) -> Response {
    let Some(slot) = stream_slot() else {
        return unavailable("stream_capacity_exhausted");
    };
    // The stream key is the source's signing identity; a source without one
    // cannot sign coverage, so it serves no stream rather than an unsigned one.
    let identity = {
        let writer = store.clone();
        let read = crate::base::blocking(move || Ok(writer.writer.lock().signing_identity())).await;
        match read {
            Ok(Some(identity)) => identity,
            Ok(None) => return unavailable("stream_signing_unavailable"),
            Err(error) => return error.into_response(),
        }
    };
    let Some(id) = session_id() else {
        return unavailable("stream_session_unavailable");
    };
    let opened = {
        let store = store.clone();
        let id = id.clone();
        crate::admission::read_blocking(move || {
            let read = store.read();
            Ok(Session::open(&read, &request, id, identity))
        })
        .await
    };
    let (session, first) = match opened {
        Ok(Ok(opened)) => opened,
        Ok(Err(CutError::Refused(error))) => return AppError::from(error).into_response(),
        Ok(Err(CutError::Aborted(reason))) => {
            return unavailable(&format!("stream_aborted: {}", reason.as_str()));
        }
        Err(error) => return error.into_response(),
    };
    let (frames, received) = mpsc::channel::<Event>(FRAME_BUFFER);
    let (challenges, inbox) = mpsc::channel::<[u8; 16]>(CHALLENGE_BUFFER);
    if let Ok(mut map) = sessions().lock() {
        map.insert(id.clone(), challenges);
    }
    let registered = Registered(id);
    if frames.send(sse(first)).await.is_err() {
        return unavailable("stream_closed");
    }
    tokio::spawn(async move {
        let _registered = registered;
        let _slot = slot;
        produce(store, session, frames, inbox).await;
    });
    let output = unfold(received, |mut received| async move {
        received
            .recv()
            .await
            .map(|event| (Ok::<_, Infallible>(event), received))
    });
    Sse::new(output).into_response()
}

/// The session's producer: poll, send, then wait for a commit, a challenge or
/// the heartbeat. Ends when the follower goes away, the session ends, or the
/// store cannot be read (the follower resumes from its cursor).
async fn produce(
    store: SharedStore,
    mut session: Session,
    frames: mpsc::Sender<Event>,
    mut inbox: mpsc::Receiver<[u8; 16]>,
) {
    // Subscribe BEFORE the first read so no commit falls between them.
    let mut hints = store.commit_wake.subscribe();
    hints.borrow_and_update();
    let mut nonces: VecDeque<[u8; 16]> = VecDeque::new();
    loop {
        let nonce = nonces.front().copied();
        let polled = {
            let store = store.clone();
            crate::admission::read_blocking(move || {
                let read = store.read();
                let result = session.poll(&read, POLL_TXS, nonce).map_err(AppError::from);
                Ok((session, result))
            })
            .await
        };
        let Ok((returned, result)) = polled else {
            return;
        };
        session = returned;
        let Ok((batch, answered)) = result else {
            return;
        };
        if answered {
            nonces.pop_front();
        }
        let sent_any = !batch.is_empty();
        for frame in batch {
            if frames.send(sse(frame)).await.is_err() {
                return;
            }
        }
        if session.ended() {
            return;
        }
        // More may be waiting (a full page, or a nonce still to answer).
        if sent_any || !nonces.is_empty() {
            continue;
        }
        tokio::select! {
            changed = hints.changed() => {
                if changed.is_err() {
                    return;
                }
                // Wait for the hinted commit to finish before reading it.
                let writer = store.clone();
                if crate::base::blocking(move || {
                    drop(writer.writer.lock());
                    Ok(())
                })
                .await
                .is_err()
                {
                    return;
                }
            }
            nonce = inbox.recv() => match nonce {
                Some(nonce) => nonces.push_back(nonce),
                None => return,
            },
            // The follower went away: free its slot and session now.
            () = frames.closed() => return,
            () = tokio::time::sleep(HEARTBEAT) => {
                if frames.send(sse(session.heartbeat())).await.is_err() {
                    return;
                }
            }
        }
    }
}

/// A freshness challenge for an open session.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChallengeRequest {
    session: String,
    /// 16 bytes, hex.
    nonce: String,
}

pub(crate) async fn challenge(axum::Json(request): axum::Json<ChallengeRequest>) -> Response {
    let Some(nonce) = hex::decode(&request.nonce)
        .ok()
        .and_then(|bytes| <[u8; 16]>::try_from(bytes).ok())
    else {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({"error": "nonce must be 16 bytes of hex"})),
        )
            .into_response();
    };
    let sender = sessions()
        .lock()
        .ok()
        .and_then(|map| map.get(&request.session).cloned());
    let Some(sender) = sender else {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(json!({"error": "no open stream session with that id"})),
        )
            .into_response();
    };
    match sender.try_send(nonce) {
        Ok(()) => (
            StatusCode::ACCEPTED,
            axum::Json(json!({"session": request.session})),
        )
            .into_response(),
        Err(mpsc::error::TrySendError::Full(_)) => (
            StatusCode::TOO_MANY_REQUESTS,
            axum::Json(json!({"error": "too many unanswered challenges on this session"})),
        )
            .into_response(),
        Err(mpsc::error::TrySendError::Closed(_)) => (
            StatusCode::NOT_FOUND,
            axum::Json(json!({"error": "the stream session has ended"})),
        )
            .into_response(),
    }
}

#[cfg(test)]
#[path = "replicate_stream_tests.rs"]
mod tests;
