use std::sync::Arc;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    routing::post,
};
use serde_json::json;
use tower::ServiceExt;

use super::super::{SharedStore, StoreHandle};
use super::{entity_history, history_read};

fn fixture() -> (quipu::Store, serde_json::Value) {
    let mut store = quipu::Store::open_in_memory().unwrap();
    let graph = "https://example.org/board";
    let g = store.graph_create(graph).unwrap();
    quipu::rdf::ingest_rdf_to_graph(
        &mut store,
        b"<https://example.org/item> <https://example.org/title> \"one\" .".as_slice(),
        oxrdfio::RdfFormat::NTriples,
        None,
        "2026-01-01T00:00:00Z",
        None,
        None,
        g,
    )
    .unwrap();
    let request = json!({"iri":"https://example.org/item","graph":graph,"source_store_id":store.store_id().unwrap()});
    (store, request)
}

#[tokio::test]
async fn entity_history_http_returns_qualified_graph_and_refuses_bad_scope() {
    let (store, input) = fixture();
    let shared: SharedStore = Arc::new(StoreHandle::writer_only(store));
    let app = Router::new()
        .route("/entity_history", post(entity_history))
        .with_state(shared);
    for bad in [false, true] {
        let mut request = input.clone();
        if bad {
            request["source_store_id"] = json!("wrong-source");
        }
        let response = app
            .clone()
            .oneshot(
                Request::post("/entity_history")
                    .header("content-type", "application/json")
                    .body(Body::from(request.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        if bad {
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        } else {
            assert_eq!(status, StatusCode::OK, "{body}");
            assert_eq!(body["schema"], quipu::acquisition::history::CONTRACT);
            assert_eq!(body["count"], 1);
            assert_eq!(body["graph"], input["graph"]);
        }
    }
}

#[test]
fn entity_history_read_refuses_attestation_without_spending_nonce() {
    use quipu::session_attestation::SessionBinding;
    use quipu::transaction_auth::{
        AttestState, AttestationSlot, Identity, PendingAttestation, with_identity,
    };
    use ring::signature::{Ed25519KeyPair, KeyPair};
    let (store, input) = fixture();
    let key = Ed25519KeyPair::from_seed_unchecked(&[59; 32]).unwrap();
    let now = quipu::time::epoch_secs();
    let mut binding = SessionBinding::new(
        "urn:crew:seeds",
        "history",
        hex::encode(key.public_key().as_ref()),
        "urn:crew:lead",
        now - 60,
        now + 3600,
    )
    .unwrap();
    binding.allow_write = true;
    store.attestation_register(&binding).unwrap();
    let pending = PendingAttestation::new(
        binding.session.clone(),
        "abcdef0123456789abcdef0123456789".into(),
        binding.key_id.clone(),
        binding.introducer.clone(),
    );
    let identity = Identity {
        principal: binding.agent,
        credential_id: Some(binding.key_id),
        auth_class: "attested_session".into(),
        attestation: AttestationSlot(Some(pending.clone())),
    };
    let handle = StoreHandle::writer_only(store);
    with_identity(Some(identity), || {
        assert!(
            format!("{:?}", history_read(&handle, &input).unwrap_err())
                .contains("signed write attestation")
        );
        assert_eq!(pending.state(), AttestState::Unspent);
        drop(handle.lock()); // Positive control: this really was spendable.
        assert_eq!(pending.state(), AttestState::Spent);
    });
    assert_eq!(history_read(&handle, &input).unwrap()["count"], 1);
}
