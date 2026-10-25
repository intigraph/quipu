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
            axum::http::HeaderMap::new(),
            None,
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
            axum::http::HeaderMap::new(),
            None,
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

fn identity_fixture() -> (SharedStore, Ed25519KeyPair, SessionBinding) {
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
    (Arc::new(StoreHandle::writer_only(store)), key, binding)
}

fn identity_router(keys_required: bool) -> (axum::Router, Ed25519KeyPair, SessionBinding) {
    let (store, key, binding) = identity_fixture();
    let app = axum::Router::new()
        .route(
            "/identity",
            axum::routing::post(super::super::session_identity),
        )
        .layer(axum::middleware::from_fn(move |req, next| {
            super::handle(store.clone(), keys_required, req, next)
        }));
    (app, key, binding)
}

fn identity_request(
    key: &Ed25519KeyPair,
    binding: &SessionBinding,
    nonce: &str,
    claim: Option<&str>,
) -> axum::extract::Request {
    let headers: Vec<(&str, &str)> = claim.map(|c| ("X-Quipu-Agent", c)).into_iter().collect();
    signed_post(key, binding, nonce, "/identity", &headers)
}

/// A signed POST of `{}` to `path`, plus extra (declared provenance) headers.
fn signed_post(
    key: &Ed25519KeyPair,
    binding: &SessionBinding,
    nonce: &str,
    path: &str,
    headers: &[(&str, &str)],
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
            path,
            content_type: "application/json",
            body_sha256: &hash,
            audience: None,
        }),
    );
    envelope.signature = hex::encode(key.sign(&message).as_ref());
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&envelope).unwrap());
    let mut request = axum::http::Request::post(path)
        .header("content-type", "application/json")
        .header(super::HEADER, header);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    request.body(axum::body::Body::from("{}")).unwrap()
}

#[tokio::test]
async fn identity_is_key_derived_and_replay_and_wrong_key_refuse() {
    use axum::http::StatusCode;
    use tower::ServiceExt;
    let (app, key, binding) = identity_router(false);
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
    // Enforcement off: a differing DECLARED agent is provenance, not a claim
    // over the key, so it is admitted and the principal stays key-derived.
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
    assert_eq!(response.status(), StatusCode::OK);
    let value: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(value["principal"], "synthetic-owner");
    // Enforcement on: the same override is refused.
    let (strict, key, binding) = identity_router(true);
    let response = strict
        .oneshot(identity_request(
            &key,
            &binding,
            "00000000000000000000000000000006",
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_signed_reads_admit_exactly_one_before_handler_dispatch() {
    let (store, key, binding) = identity_fixture();
    let request = identity_request(&key, &binding, "00000000000000000000000000000004", None);
    let headers = request.headers().clone();
    let gate = Arc::new(tokio::sync::Barrier::new(16));
    let mut tasks = Vec::new();
    for _ in 0..16 {
        let (store, headers, gate) = (store.clone(), headers.clone(), gate.clone());
        tasks.push(tokio::spawn(async move {
            let mut req = axum::http::Request::post("/identity")
                .body(axum::body::Body::from("{}"))
                .unwrap();
            *req.headers_mut() = headers;
            gate.wait().await;
            // Deliberately stop before the handler and post-handler spend:
            // admission itself must atomically exclude every duplicate.
            super::verify(&store, false, req)
                .await
                .map(|_| ())
                .map_err(|e| e.verdict)
        }));
    }
    let (mut admitted, mut replayed) = (0, 0);
    for task in tasks {
        match task.await.unwrap() {
            Ok(()) => admitted += 1,
            Err("replay") => replayed += 1,
            Err(other) => panic!("unexpected refusal: {other}"),
        }
    }
    assert_eq!((admitted, replayed), (1, 15));
    // A fresh nonce is still usable: the gate is not a blanket refusal.
    assert!(
        super::verify(
            &store,
            false,
            identity_request(&key, &binding, "00000000000000000000000000000005", None,)
        )
        .await
        .is_ok()
    );
}

/// The REAL sd request shape (aegis-m6o): a signed write from a registered
/// host key that also DECLARES the crew agent and the harness session, neither
/// equal to the binding. quipu #687 refused exactly this with enforcement off,
/// and every crew write failed. Off must admit it; on must refuse only a true
/// mismatch, and admit matching declarations.
#[tokio::test]
async fn sd_signed_write_with_declared_provenance_is_refused_only_when_keys_are_required() {
    let sd_headers = [
        ("X-Quipu-Agent", "malcolm"),
        ("X-Quipu-Session", "0185d3e0-crew-harness-session"),
        ("X-Quipu-Harness", "claude"),
    ];
    let (store, key, binding) = identity_fixture();
    let write = |nonce: &str, headers: &[(&str, &str)]| {
        signed_post(&key, &binding, nonce, "/graph/create", headers)
    };

    let off = super::verify(
        &store,
        false,
        write("000000000000000000000000000000a1", &sd_headers),
    )
    .await;
    assert!(
        off.is_ok(),
        "enforcement off refused the sd request shape: {:?}",
        off.err().map(|r| r.message)
    );

    let on = super::verify(
        &store,
        true,
        write("000000000000000000000000000000a2", &sd_headers),
    )
    .await;
    let Err(refusal) = on else {
        panic!("enforcement on admitted a mismatched declared identity");
    };
    assert_eq!(refusal.verdict, "invalid");
    assert_eq!(
        refusal.message,
        "declared identity differs from the authenticated session"
    );

    let matching = [
        ("X-Quipu-Agent", binding.agent.as_str()),
        ("X-Quipu-Session", binding.session.as_str()),
    ];
    let control = super::verify(
        &store,
        true,
        write("000000000000000000000000000000a3", &matching),
    )
    .await;
    assert!(
        control.is_ok(),
        "enforcement on refused a matching declaration: {:?}",
        control.err().map(|r| r.message)
    );
}

/// A signed POST of `{}` to `path` under `quipu-write-v2`, bound to
/// `audience` (the store the client believes it is writing to).
fn signed_post_v2(
    key: &Ed25519KeyPair,
    binding: &SessionBinding,
    nonce: &str,
    path: &str,
    audience: &str,
    relabel: Option<&str>,
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
    let hash = quipu::session_attestation::body_sha256(b"{}");
    let message = canonical_message(
        &envelope,
        &SignedBinding::Write(WriteBinding {
            method: "POST",
            path,
            content_type: "application/json",
            body_sha256: &hash,
            audience: Some(audience),
        }),
    );
    envelope.signature = hex::encode(key.sign(&message).as_ref());
    // A relay that edits the audience claim after signing.
    if let Some(claim) = relabel {
        envelope.audience = Some(claim.into());
    }
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&envelope).unwrap());
    axum::http::Request::post(path)
        .header("content-type", "application/json")
        .header(super::HEADER, header)
        .body(axum::body::Body::from("{}"))
        .unwrap()
}

/// The HTTP adapter's v2 path, end to end through `verify` and the writer
/// lock: the audience is this store's own id, a relabelled claim fails the
/// signature, and a binding revoked while the write is queued is refused at
/// settlement without spending the nonce. The older tests here are v1-only,
/// so none of them could see an audience or settlement regression on v2.
#[tokio::test]
async fn a_v2_signed_write_is_bound_to_this_store_and_settles_under_the_lock() {
    let (store, key, binding) = identity_fixture();
    let here = store.read().store_id().unwrap();
    let elsewhere = quipu::Store::open_in_memory().unwrap().store_id().unwrap();
    assert_ne!(here, elsewhere, "control: two store identities");
    let nonce = |n: u8| format!("{n:032x}");

    // Admitted for this store.
    let ok = super::verify(
        &store,
        false,
        signed_post_v2(&key, &binding, &nonce(1), "/graph/create", &here, None),
    )
    .await;
    assert!(ok.is_ok(), "{:?}", ok.err().map(|r| r.message));

    // Signed for another store.
    let Err(refusal) = super::verify(
        &store,
        false,
        signed_post_v2(&key, &binding, &nonce(2), "/graph/create", &elsewhere, None),
    )
    .await
    else {
        panic!("a signature for another store was admitted");
    };
    assert!(
        refusal.message.contains("audience mismatch"),
        "{}",
        refusal.message
    );

    // Signed for another store, claim relabelled to this one.
    let Err(refusal) = super::verify(
        &store,
        false,
        signed_post_v2(
            &key,
            &binding,
            &nonce(3),
            "/graph/create",
            &elsewhere,
            Some(&here),
        ),
    )
    .await
    else {
        panic!("a relabelled audience was admitted");
    };
    assert_eq!(refusal.verdict, "badsig");

    // Valid at the pre-check, revoked while queued: refused at settlement.
    let Ok((pending, _, _)) = super::verify(
        &store,
        false,
        signed_post_v2(&key, &binding, &nonce(4), "/graph/create", &here, None),
    )
    .await
    else {
        panic!("pre-check refused a valid v2 request");
    };
    store.read().attestation_revoke(&binding.session).unwrap();
    let settled = store.lock().settle_attestation(&pending);
    assert!(settled.is_err());
    assert!(matches!(
        pending.state(),
        AttestState::Refused {
            verdict: "revoked",
            ..
        }
    ));
    assert!(
        !store
            .read()
            .attestation_nonce_spent(&binding.session, &nonce(4))
            .unwrap(),
        "a refused settlement spends nothing"
    );
}
