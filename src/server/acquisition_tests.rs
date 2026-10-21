use std::sync::Arc;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    routing::post,
};
use serde_json::{Value, json};
use tower::ServiceExt;

use super::super::{SharedStore, StoreHandle, publication, query_endpoint};

async fn call(app: Router, route: &str, input: Value) -> (StatusCode, Value) {
    let response = app
        .oneshot(
            Request::post(route)
                .header("content-type", "application/json")
                .body(Body::from(input.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let body = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| json!({"text":String::from_utf8_lossy(&bytes)}));
    (status, body)
}

#[tokio::test]
async fn acquisition_http_contract_and_malformed_query_pin() {
    let mut store = quipu::Store::open_in_memory().unwrap();
    let iri = "https://example.org/project/board";
    let graph = store.graph_create(iri).unwrap();
    quipu::rdf::ingest_rdf_to_graph(
        &mut store,
        b"<https://example.org/item/one> <https://example.org/title> \"before\" .".as_slice(),
        oxrdfio::RdfFormat::NTriples,
        None,
        "2026-01-01T00:00:00Z",
        None,
        None,
        graph,
    )
    .unwrap();
    let tx = store.latest_tx_id().unwrap();
    let identity = store.store_id().unwrap();
    let shared: SharedStore = Arc::new(StoreHandle::writer_only(store));
    let app = Router::new()
        .route("/acquire", post(publication::acquire))
        .route("/query", post(query_endpoint::query_post_http))
        .with_state(shared);
    let input = json!({"source_store_id":identity,"graphs":[iri],"tx":tx});
    let (status, body) = call(app.clone(), "/acquire", input.clone()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let payload: quipu::acquisition::Payload = serde_json::from_value(body).unwrap();
    payload
        .verify(&serde_json::from_value(input.clone()).unwrap())
        .unwrap();
    assert_eq!(payload.source_receipt.applied_tx, tx);
    assert!(payload.graphs[iri].contains("before"));
    let (status, body) = call(
        app.clone(),
        "/query",
        json!({"query":"ASK {}", "tx":"bogus"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.get("error").is_some());
    let (status, body) = call(app.clone(), "/query", json!({"query":"ASK {}"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], true);
    let mut invalid = input;
    invalid["tx"] = Value::Null;
    let (status, body) = call(app.clone(), "/acquire", invalid).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, body) = call(
        app,
        "/acquire",
        json!({"source_store_id":"foreign", "graphs":[iri]}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.get("source_receipt").is_none());
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[tokio::test]
async fn composed_http_acquisition_keeps_v1_refusal_and_required_scope() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = quipu::Store::open(dir.path().join("main.db").to_str().unwrap()).unwrap();
    let iri = "https://example.org/project/board";
    for name in [iri, "https://example.org/archive"] {
        let graph = store.graph_create(name).unwrap();
        quipu::rdf::ingest_rdf_to_graph(
            &mut store,
            b"<https://example.org/item> <https://example.org/text> \"committed comment\" ."
                .as_slice(),
            oxrdfio::RdfFormat::NTriples,
            None,
            "2026-01-01T00:00:00Z",
            None,
            None,
            graph,
        )
        .unwrap();
    }
    store
        .freeze_graph(
            "https://example.org/archive",
            dir.path().to_str().unwrap(),
            "2026-01-02T00:00:00Z",
            None,
        )
        .unwrap();
    let identity = store.store_id().unwrap();
    let shared: SharedStore = Arc::new(StoreHandle::writer_only(store));
    let app = Router::new()
        .route("/acquire", post(publication::acquire))
        .route("/acquire/composed", post(publication::acquire_composed))
        .with_state(shared);
    let input = json!({"source_store_id":identity,"graphs":[iri]});
    println!("DEMO request {input}");
    let (status, body) = call(app.clone(), "/acquire/composed", input.clone()).await;
    println!("DEMO POST /acquire/composed response {status} {body}");
    assert_eq!(status, StatusCode::OK, "{body}");
    let payload: quipu::acquisition::composed::Payload = serde_json::from_value(body).unwrap();
    payload
        .verify(&serde_json::from_value(input.clone()).unwrap())
        .unwrap();
    assert_eq!(payload.source_receipt.composition.providers.len(), 1);
    assert!(payload.graphs[iri].contains("committed comment"));
    let (status, body) = call(app.clone(), "/acquire", input.clone()).await;
    println!("DEMO POST /acquire response {status} {body}");
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.get("source_receipt").is_none());
    for extra in [
        json!({"tx":null}),
        json!({"tx":"1"}),
        json!({"expected_generation":"wrong"}),
        json!({"graphs":[iri,format!("{iri}#seeds-ephemeral")]}),
        json!({"max_bytes":1}),
    ] {
        let mut refused = input.clone();
        for (key, value) in extra.as_object().unwrap() {
            refused[key] = value.clone();
        }
        println!("DEMO refusal request {refused}");
        let (status, body) = call(app.clone(), "/acquire/composed", refused).await;
        println!("DEMO refusal response {status} {body}");
        assert!(!status.is_success(), "{body}");
        assert!(body.get("source_receipt").is_none());
    }
}

fn eligibility_store() -> (quipu::Store, String) {
    let mut store = quipu::Store::open_in_memory().unwrap();
    let iri = "https://example.org/project/board";
    let graph = store.graph_create(iri).unwrap();
    quipu::rdf::ingest_rdf_to_graph(
        &mut store,
        b"<https://example.org/item/one> <https://example.org/title> \"t\" .".as_slice(),
        oxrdfio::RdfFormat::NTriples,
        None,
        "2026-01-01T00:00:00Z",
        None,
        None,
        graph,
    )
    .unwrap();
    (store, iri.to_string())
}

#[tokio::test]
async fn eligibility_http_contract_is_an_observation_not_a_receipt() {
    let (store, iri) = eligibility_store();
    let head = store.latest_tx_id().unwrap();
    let identity = store.store_id().unwrap();
    let shared: SharedStore = Arc::new(StoreHandle::writer_only(store));
    let app = Router::new()
        .route(
            "/acquire/eligibility",
            post(publication::acquire_eligibility),
        )
        .with_state(shared);
    let input = json!({"source_store_id":identity,"graphs":[iri],"expected_head_tx":head});
    let (status, body) = call(app.clone(), "/acquire/eligibility", input).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["schema"], quipu::acquisition::eligibility::CONTRACT);
    assert_eq!(body["observed_head_tx"], head);
    assert_eq!(body["graphs"][iri.as_str()]["status"], "eligible");
    assert!(body.get("applied_tx").is_none() && body.get("source_receipt").is_none());
    // Unknown fields and a pin are refused: eligibility is observed at the head.
    let pinned = json!({"source_store_id":identity,"graphs":[iri],"tx":head});
    let (status, _) = call(app.clone(), "/acquire/eligibility", pinned).await;
    assert!(status.is_client_error(), "{status}");
    let moved = json!({"source_store_id":identity,"graphs":[iri],"expected_head_tx":head + 1});
    let (status, body) = call(app, "/acquire/eligibility", moved).await;
    assert!(!status.is_success(), "{status} {body}");
}

/// The eligibility guard must never settle a signed attestation. The same
/// request through the ordinary writer guard spends the nonce, so an Unspent
/// result here is a measurement, not an attestation that was never pending.
#[test]
fn an_attested_eligibility_request_is_refused_without_spending_its_nonce() {
    use quipu::session_attestation::SessionBinding;
    use quipu::transaction_auth::{
        AttestState, AttestationSlot, Identity, PendingAttestation, with_identity,
    };
    use ring::signature::{Ed25519KeyPair, KeyPair};

    let (store, iri) = eligibility_store();
    let key = Ed25519KeyPair::from_seed_unchecked(&[47; 32]).unwrap();
    let now = quipu::time::epoch_secs();
    let mut binding = SessionBinding::new(
        "urn:crew:seeds",
        "eligibility",
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
        "fedcba9876543210fedcba9876543210".into(),
        binding.key_id.clone(),
        binding.introducer.clone(),
    );
    let identity = Identity {
        principal: binding.agent,
        credential_id: Some(binding.key_id),
        auth_class: "attested_session".into(),
        attestation: AttestationSlot(Some(pending.clone())),
    };
    let input = quipu::acquisition::eligibility::Request {
        source_store_id: store.store_id().unwrap(),
        graphs: vec![iri],
        expected_head_tx: None,
    };
    let handle = StoreHandle::writer_only(store);
    with_identity(Some(identity), || {
        let refused = publication::eligibility(&handle, &input).unwrap_err();
        assert!(format!("{refused:?}").contains("signed write attestation"));
        assert_eq!(pending.state(), AttestState::Unspent);
        // The dedicated guard itself settles nothing either.
        drop(handle.metadata_lock());
        assert_eq!(pending.state(), AttestState::Unspent);
        // Positive control: the ordinary writer guard spends it.
        drop(handle.lock());
        assert_eq!(pending.state(), AttestState::Spent);
    });
    // Unattested, the same handle observes normally.
    assert_eq!(
        publication::eligibility(&handle, &input)
            .unwrap()
            .graphs
            .len(),
        1
    );
}
