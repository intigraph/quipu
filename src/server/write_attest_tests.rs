//! The `rw_handler!` barrier for signed writes that mutate WITHOUT a
//! transaction (aegis-bys8d1, malcolm's arm). Over HTTP a replay is stopped by
//! the pre-check and create is idempotent, so a server test cannot tell whether
//! `refuse_if_refused` (tools.rs, right after the writer lock) exists at all.
//! This drives the real `graph_create` handler with a request whose binding
//! was valid at the pre-check and revoked before the writer lock: exactly the
//! case only that line stops.

use std::sync::Arc;

use axum::extract::State;
use quipu::session_attestation::SessionBinding;
use quipu::transaction_auth::{AttestState, AttestationSlot, Identity, PendingAttestation};
use ring::signature::{Ed25519KeyPair, KeyPair};

use crate::{SharedStore, StoreHandle};

const GRAPH: &str = "urn:barrier:graph";

fn signed_request(revoke: bool) -> (SharedStore, Arc<PendingAttestation>, Identity) {
    let store = quipu::Store::open_in_memory().unwrap();
    let key = Ed25519KeyPair::from_seed_unchecked(&[31; 32]).unwrap();
    let now = quipu::time::epoch_secs();
    let mut binding = SessionBinding::new(
        "urn:crew:seeds",
        "barrier",
        hex::encode(key.public_key().as_ref()),
        "urn:crew:lead",
        now - 60,
        now + 3600,
    )
    .unwrap();
    binding.allow_write = true;
    store.attestation_register(&binding).unwrap();
    if revoke {
        // Valid when the middleware checked it; revoked while the write queued.
        store.attestation_revoke("barrier").unwrap();
    }
    let pending = PendingAttestation::new(
        binding.session.clone(),
        "0123456789abcdef0123456789abcdef".into(),
        binding.key_id.clone(),
        binding.introducer.clone(),
    );
    let identity = Identity {
        principal: binding.agent,
        credential_id: Some(binding.key_id),
        auth_class: "attested_session".into(),
        attestation: AttestationSlot(Some(pending.clone())),
    };
    (Arc::new(StoreHandle::writer_only(store)), pending, identity)
}

fn graph_exists(store: &SharedStore) -> bool {
    quipu::tool_graph_list(&store.read(), &serde_json::json!({}))
        .unwrap()
        .to_string()
        .contains(GRAPH)
}

#[tokio::test]
async fn a_refused_signed_graph_create_creates_no_graph() {
    let (store, pending, identity) = signed_request(true);
    let result = super::super::run_identified(
        Some(identity),
        crate::tools::graph_create(
            State(store.clone()),
            axum::Json(serde_json::json!({ "graph": GRAPH })),
        ),
    )
    .await;
    assert!(result.is_err(), "a refused request must not reach the tool");
    assert!(matches!(
        pending.state(),
        AttestState::Refused {
            verdict: "revoked",
            ..
        }
    ));
    assert!(
        !graph_exists(&store),
        "the refused request created the graph"
    );
}

#[tokio::test]
async fn control_the_same_signed_graph_create_lands_when_not_refused() {
    let (store, pending, identity) = signed_request(false);
    let result = super::super::run_identified(
        Some(identity),
        crate::tools::graph_create(
            State(store.clone()),
            axum::Json(serde_json::json!({ "graph": GRAPH })),
        ),
    )
    .await;
    assert!(result.is_ok());
    assert_eq!(pending.state(), AttestState::Spent);
    assert!(graph_exists(&store));
}

#[test]
fn expired_first_writer_lock_does_not_spend_signed_nonce() {
    let (store, pending, identity) = signed_request(false);
    quipu::transaction_auth::with_identity(Some(identity), || {
        {
            let _deadline =
                quipu::time::set_request_deadline(Some(quipu::time::Deadline::after_millis(0)));
            assert!(store.write_lock().is_err());
            assert_eq!(pending.state(), AttestState::Unspent);
        }
        // Positive control: the same request can settle after the expired scope.
        let _guard = store.write_lock().unwrap();
        assert_eq!(pending.state(), AttestState::Spent);
    });
    assert!(!graph_exists(&store));
}

fn identity_router() -> (axum::Router, Ed25519KeyPair, SessionBinding) {
    let store = quipu::Store::open_in_memory().unwrap();
    let key = Ed25519KeyPair::from_seed_unchecked(&[51; 32]).unwrap();
    let now = quipu::time::epoch_secs();
    let mut binding = SessionBinding::new(
        "synthetic-owner",
        "identity-control",
        hex::encode(key.public_key().as_ref()),
        "synthetic-introducer",
        now - 10,
        now + 3600,
    )
    .unwrap();
    binding.allow_write = true;
    store.attestation_register(&binding).unwrap();
    let store = Arc::new(StoreHandle::writer_only(store));
    let app = axum::Router::new()
        .route(
            "/identity",
            axum::routing::post(super::super::session_identity),
        )
        .layer(axum::middleware::from_fn(move |req, next| {
            super::handle(store.clone(), req, next)
        }));
    (app, key, binding)
}

fn identity_request(
    key: &Ed25519KeyPair,
    binding: &SessionBinding,
    nonce: &str,
    claim: Option<&str>,
) -> axum::extract::Request {
    use base64::Engine as _;
    use quipu::session_attestation::{
        AttestationEnvelope, SignedBinding, WRITE_V1, WriteBinding, canonical_message,
    };
    let mut envelope = AttestationEnvelope {
        version: WRITE_V1.into(),
        key_id: binding.key_id.clone(),
        session: binding.session.clone(),
        introducer: binding.introducer.clone(),
        issued_at_epoch: quipu::time::epoch_secs(),
        nonce: nonce.into(),
        signature: String::new(),
        audience: None,
    };
    let hash = quipu::session_attestation::body_sha256(b"{}");
    let message = canonical_message(
        &envelope,
        &SignedBinding::Write(WriteBinding {
            method: "POST",
            path: "/identity",
            content_type: "application/json",
            body_sha256: &hash,
            audience: None,
        }),
    );
    envelope.signature = hex::encode(key.sign(&message).as_ref());
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&envelope).unwrap());
    let mut request = axum::http::Request::post("/identity")
        .header("content-type", "application/json")
        .header(super::HEADER, header);
    if let Some(claim) = claim {
        request = request.header("X-Quipu-Agent", claim);
    }
    request.body(axum::body::Body::from("{}")).unwrap()
}

#[tokio::test]
async fn identity_is_key_derived_and_replay_wrong_key_and_actor_override_refuse() {
    use axum::http::StatusCode;
    use tower::ServiceExt;
    let (app, key, binding) = identity_router();
    let nonce = "00000000000000000000000000000001";
    let response = app
        .clone()
        .oneshot(identity_request(&key, &binding, nonce, None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(value["principal"], "synthetic-owner");
    assert_eq!(value["key_id"], binding.key_id);
    assert_eq!(value["session"], binding.session);
    assert_eq!(value["authenticated"], true);
    let replay = app
        .clone()
        .oneshot(identity_request(&key, &binding, nonce, None))
        .await
        .unwrap();
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
    let wrong = Ed25519KeyPair::from_seed_unchecked(&[52; 32]).unwrap();
    let response = app
        .clone()
        .oneshot(identity_request(
            &wrong,
            &binding,
            "00000000000000000000000000000002",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = app
        .clone()
        .oneshot(identity_request(
            &key,
            &binding,
            "00000000000000000000000000000003",
            Some("synthetic-other"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = app
        .oneshot(
            axum::http::Request::post("/identity")
                .body(axum::body::Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
