use super::*;
use quipu::fluent::Scalar;
use std::sync::Arc;

fn shared() -> SharedStore {
    Arc::new(super::super::StoreHandle::writer_only(
        quipu::Store::open_in_memory().unwrap(),
    ))
}

fn request() -> AppendRequest {
    AppendRequest {
        graph: quipu::schema::ROOT_GRAPH_IRI.into(),
        fluent: "urn:fluent:http-test".into(),
        event: "urn:event:http-test".into(),
        subject: "urn:subject".into(),
        predicate: "urn:count".into(),
        property_shape: "urn:count-shape".into(),
        as_of: "2026-11-12T01:00:00Z".into(),
        value: Scalar::Integer(3),
    }
}

fn identity(principal: &str) -> quipu::transaction_auth::Identity {
    quipu::transaction_auth::Identity {
        principal: principal.into(),
        credential_id: Some("http-test".into()),
        auth_class: "named_bearer".into(),
        attestation: Default::default(),
    }
}

#[tokio::test]
async fn open_write_and_legacy_identity_refuse_before_store_work() {
    let store = shared();
    let before = store.read().latest_tx_id().unwrap();
    for principal in [None, Some(identity("legacy-shared-bearer"))] {
        let response = auth::run_identified(
            principal,
            append(State(store.clone()), axum::Json(request())),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    assert_eq!(store.read().latest_tx_id().unwrap(), before);
}

#[tokio::test]
async fn authenticated_request_cannot_activate_an_unloaded_schema() {
    let store = shared();
    let before = store.read().latest_tx_id().unwrap();
    let error = auth::run_identified(
        Some(identity("urn:writer:http-test")),
        append(State(store.clone()), axum::Json(request())),
    )
    .await
    .unwrap_err();
    let response = error.into_response();
    let body = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .unwrap();
    assert!(
        String::from_utf8(body.to_vec())
            .unwrap()
            .contains("not loaded")
    );
    assert_eq!(store.read().latest_tx_id().unwrap(), before);
}

#[cfg(feature = "shacl")]
#[tokio::test]
async fn authenticated_http_append_replay_and_current_share_native_pipeline() {
    let store = shared();
    {
        let mut live = store.lock();
        live.load_shapes(
            "quipu-fluents-v1",
            include_str!("../../shapes/fluents-v1.ttl"),
            "2026-11-12T00:00:00Z",
        )
        .unwrap();
        live.load_shapes(
            "http-binding",
            &format!(
                "<urn:count-shape> <http://www.w3.org/ns/shacl#path> <urn:count>; \
            <http://www.w3.org/ns/shacl#targetNode> <urn:subject>; \
            <{}currentValuePolicy> \"sum\" .",
                quipu::namespace::QUIPU_FLUENT
            ),
            "2026-11-12T00:00:00Z",
        )
        .unwrap();
    }
    async fn send(store: SharedStore, request: AppendRequest) -> Response {
        auth::run_identified(
            Some(identity("urn:writer:http-test")),
            append(State(store), axum::Json(request)),
        )
        .await
        .unwrap()
    }
    async fn json(response: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }
    let first = json(send(store.clone(), request()).await).await;
    let second = json(send(store.clone(), request()).await).await;
    assert_eq!(first, second);
    let response = current(
        State(store.clone()),
        Query(CurrentQuery {
            graph: quipu::schema::ROOT_GRAPH_IRI.into(),
            fluent: request().fluent,
        }),
    )
    .await
    .unwrap();
    let snapshot = json(response).await;
    assert_eq!(snapshot["policy"], "sum");
    assert_eq!(snapshot["producing_txs"], serde_json::json!([first["tx"]]));
    let mut replacement = request();
    replacement.value = Scalar::Integer(4);
    let before = store.read().latest_tx_id().unwrap();
    assert!(
        auth::run_identified(
            Some(identity("urn:writer:http-test")),
            append(State(store.clone()), axum::Json(replacement))
        )
        .await
        .is_err()
    );
    assert_eq!(store.read().latest_tx_id().unwrap(), before);
}
