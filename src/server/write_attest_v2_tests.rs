//! The write-v2 receiver end to end (aegis-2idcev (C')3, wu ruling [81]).
//!
//! The DREAM adapter is an external process: it reaches quipu as an `sd`
//! subprocess over HTTP, so the canonical receiving service is this middleware,
//! not an in-process dispatch. Every test here therefore goes through the FULL
//! stack a production write takes: `handle` (pre-check), the real
//! `graph_create` handler (writer lock, lock-time settlement), then the durable
//! finalization in `handle`. The sibling v2 test stops at `verify` and calls
//! `settle_attestation` directly, so none of these properties was covered.

use std::sync::Arc;

use axum::http::StatusCode;
use quipu::session_attestation::SessionBinding;
use ring::signature::Ed25519KeyPair;
use tower::ServiceExt;

use super::{GRAPH, graph_exists, identity_fixture};
use crate::{SharedStore, StoreHandle};

/// `quipu-write-v2` over `body`, signed for `signed_path` and `audience`, sent
/// to `sent_path`. Unequal paths are a relay re-targeting a signed request.
fn signed_v2(
    key: &Ed25519KeyPair,
    binding: &SessionBinding,
    nonce: &str,
    (signed_path, sent_path): (&str, &str),
    audience: &str,
    body: &[u8],
) -> axum::extract::Request {
    use base64::Engine as _;
    use quipu::session_attestation::{
        AttestationEnvelope, SignedBinding, WRITE_V2, WriteBinding, canonical_message,
    };
    let mut envelope = AttestationEnvelope {
        version: WRITE_V2.into(),
        key_id: binding.key_id.clone(),
        session: binding.session.clone(),
        introducer: binding.introducer.clone(),
        issued_at_epoch: quipu::time::epoch_secs(),
        nonce: nonce.into(),
        signature: String::new(),
        audience: Some(audience.into()),
    };
    let hash = quipu::session_attestation::body_sha256(body);
    let message = canonical_message(
        &envelope,
        &SignedBinding::Write(WriteBinding {
            method: "POST",
            path: signed_path,
            content_type: "application/json",
            body_sha256: &hash,
            audience: Some(audience),
        }),
    );
    envelope.signature = hex::encode(key.sign(&message).as_ref());
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&envelope).unwrap());
    axum::http::Request::post(sent_path)
        .header("content-type", "application/json")
        .header(super::super::HEADER, header)
        .body(axum::body::Body::from(body.to_vec()))
        .unwrap()
}

/// `/graph/create` behind the signed-write middleware, as the server mounts it.
fn receiver(store: &SharedStore) -> axum::Router {
    let guard = store.clone();
    axum::Router::new()
        .route(
            "/graph/create",
            axum::routing::post(crate::tools::graph_create),
        )
        .with_state(store.clone())
        .layer(axum::middleware::from_fn(move |req, next| {
            super::super::handle(guard.clone(), false, req, next)
        }))
}

/// A second store that trusts the same session, so only the audience differs.
fn other_store(binding: &SessionBinding) -> SharedStore {
    let store = quipu::Store::open_in_memory().unwrap();
    store.attestation_register(binding).unwrap();
    Arc::new(StoreHandle::writer_only(store))
}

async fn send(app: &axum::Router, req: axum::extract::Request) -> (StatusCode, serde_json::Value) {
    let response = app.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 16)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

fn create_body() -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({ "graph": GRAPH })).unwrap()
}

fn nonce(n: u8) -> String {
    format!("{n:032x}")
}

fn spent(store: &SharedStore, binding: &SessionBinding, n: u8) -> bool {
    store
        .read()
        .attestation_nonce_spent(&binding.session, &nonce(n))
        .unwrap()
}

/// A settled envelope cannot be replayed here, cannot be replayed at another
/// store that trusts the same session, and cannot be re-targeted to another
/// path. Each refusal mutates nothing.
#[tokio::test]
async fn a_settled_v2_write_replays_nowhere_and_binds_its_path() {
    let (store, key, binding) = identity_fixture();
    let here = store.read().store_id().unwrap();
    let app = receiver(&store);
    let body = create_body();
    let first = || {
        signed_v2(
            &key,
            &binding,
            &nonce(1),
            ("/graph/create", "/graph/create"),
            &here,
            &body,
        )
    };

    let (status, value) = send(&app, first()).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert!(graph_exists(&store));
    assert!(spent(&store, &binding, 1));

    // The same bytes again, at the same store.
    let (status, value) = send(&app, first()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{value}");
    assert_eq!(value["verdict"], "replay");

    // The same bytes, AFTER settling here, at a store that trusts this session.
    let elsewhere = other_store(&binding);
    let (status, value) = send(&receiver(&elsewhere), first()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{value}");
    assert_eq!(value["verdict"], "invalid", "{value}");
    assert!(
        value["message"]
            .as_str()
            .unwrap_or_default()
            .contains("audience"),
        "{value}"
    );
    assert!(!graph_exists(&elsewhere), "the other store was mutated");
    assert!(!spent(&elsewhere, &binding, 1));

    // Signed for another audited route, sent to this one.
    let (status, value) = send(
        &app,
        signed_v2(
            &key,
            &binding,
            &nonce(2),
            ("/knot", "/graph/create"),
            &here,
            &body,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{value}");
    assert_eq!(value["verdict"], "badsig");
    assert!(!spent(&store, &binding, 2));
}

/// Sixteen copies of one envelope race through the whole stack. The pre-check
/// alone cannot exclude them (each reads "unspent"); the lock-time spend must,
/// so exactly one creates and fifteen are refused as replays.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_copies_of_one_v2_write_admit_exactly_one() {
    let (store, key, binding) = identity_fixture();
    let here = store.read().store_id().unwrap();
    let app = receiver(&store);
    let template = signed_v2(
        &key,
        &binding,
        &nonce(3),
        ("/graph/create", "/graph/create"),
        &here,
        &create_body(),
    );
    let headers = template.headers().clone();
    let gate = Arc::new(tokio::sync::Barrier::new(16));
    let mut tasks = Vec::new();
    for _ in 0..16 {
        let (app, headers, gate) = (app.clone(), headers.clone(), gate.clone());
        tasks.push(tokio::spawn(async move {
            let mut req = axum::http::Request::post("/graph/create")
                .body(axum::body::Body::from(create_body()))
                .unwrap();
            *req.headers_mut() = headers;
            gate.wait().await;
            send(&app, req).await
        }));
    }
    let (mut created, mut replayed) = (0, 0);
    for task in tasks {
        match task.await.unwrap() {
            (StatusCode::OK, _) => created += 1,
            (StatusCode::UNAUTHORIZED, v) if v["verdict"] == "replay" => replayed += 1,
            (status, v) => panic!("unexpected outcome {status}: {v}"),
        }
    }
    assert_eq!((created, replayed), (1, 15));
    assert!(graph_exists(&store));
}

/// A handler that fails AFTER taking the writer lock wrote nothing, but its
/// nonce was spent in autocommit when the lock was taken. Retrying the same
/// envelope is a replay, never a second attempt against a later state.
#[tokio::test]
async fn a_v2_write_that_fails_inside_the_lock_still_spends_its_nonce() {
    let (store, key, binding) = identity_fixture();
    let here = store.read().store_id().unwrap();
    let app = receiver(&store);
    let bad = serde_json::to_vec(&serde_json::json!({ "graph": GRAPH, "timestamp": "yesterday" }))
        .unwrap();
    let request = || {
        signed_v2(
            &key,
            &binding,
            &nonce(4),
            ("/graph/create", "/graph/create"),
            &here,
            &bad,
        )
    };
    let (status, value) = send(&app, request()).await;
    assert!(
        status.is_client_error() && status != StatusCode::UNAUTHORIZED,
        "{status}: {value}"
    );
    assert!(!graph_exists(&store));
    assert!(
        spent(&store, &binding, 4),
        "a failed handler left the nonce replayable"
    );
    let (status, value) = send(&app, request()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{value}");
    assert_eq!(value["verdict"], "replay");
}

/// The lock-time layer on its own: the handler called directly, with no
/// middleware finalization after it. Only the spend at lock acquisition can
/// leave the nonce spent here.
#[tokio::test]
async fn the_lock_time_spend_alone_survives_a_failed_handler() {
    let (store, pending, identity) = super::signed_request(false);
    let result = super::super::super::run_identified(
        Some(identity),
        crate::tools::graph_create(
            axum::extract::State(store.clone()),
            axum::http::HeaderMap::new(),
            None,
            axum::Json(serde_json::json!({ "graph": GRAPH, "timestamp": "yesterday" })),
        ),
    )
    .await;
    assert!(result.is_err());
    assert!(!graph_exists(&store));
    assert!(
        store
            .read()
            .attestation_nonce_spent(&pending.session, &pending.nonce)
            .unwrap(),
        "the lock-time spend did not survive the handler error"
    );
}

/// The finalization layer on its own: the body is rejected before the handler
/// runs, so no writer lock is ever taken. Only the middleware's durable spend
/// stops the same envelope from being replayed later.
#[tokio::test]
async fn a_v2_write_refused_before_the_lock_is_finalized_by_the_middleware() {
    let (store, key, binding) = identity_fixture();
    let here = store.read().store_id().unwrap();
    let app = receiver(&store);
    let request = || {
        signed_v2(
            &key,
            &binding,
            &nonce(5),
            ("/graph/create", "/graph/create"),
            &here,
            b"not json",
        )
    };
    let (status, value) = send(&app, request()).await;
    assert!(
        status.is_client_error() && status != StatusCode::UNAUTHORIZED,
        "{status}: {value}"
    );
    assert!(
        spent(&store, &binding, 5),
        "no lock was taken and nothing finalized"
    );
    let (status, value) = send(&app, request()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{value}");
    assert_eq!(value["verdict"], "replay");
}

/// Valid at the pre-check, revoked while queued for the writer lock: the full
/// stack answers 401 revoked, creates nothing and spends nothing.
#[tokio::test]
async fn a_v2_write_revoked_while_queued_is_refused_end_to_end() {
    let (store, key, binding) = identity_fixture();
    let here = store.read().store_id().unwrap();
    let (revoker, session) = (store.clone(), binding.session.clone());
    let guard = store.clone();
    // The revocation lands between the middleware pre-check and the handler's
    // writer lock, which is the window a queued write waits in.
    let app = axum::Router::new()
        .route(
            "/graph/create",
            axum::routing::post(
                move |state: axum::extract::State<SharedStore>,
                      headers: axum::http::HeaderMap,
                      matched: Option<axum::extract::MatchedPath>,
                      json: axum::Json<serde_json::Value>| async move {
                    revoker.read().attestation_revoke(&session).unwrap();
                    crate::tools::graph_create(state, headers, matched, json).await
                },
            ),
        )
        .with_state(store.clone())
        .layer(axum::middleware::from_fn(move |req, next| {
            super::super::handle(guard.clone(), false, req, next)
        }));
    let (status, value) = send(
        &app,
        signed_v2(
            &key,
            &binding,
            &nonce(6),
            ("/graph/create", "/graph/create"),
            &here,
            &create_body(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{value}");
    assert_eq!(value["verdict"], "revoked");
    assert!(!graph_exists(&store));
    assert!(
        !spent(&store, &binding, 6),
        "a refused settlement spends nothing"
    );
}
