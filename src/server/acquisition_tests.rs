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
