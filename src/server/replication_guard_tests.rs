//! Private requests through the production authentication middleware.
use super::*;
use axum::{body::Body, http::Request};
use sha2::{Digest, Sha256};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tower::ServiceExt;

fn fixture() -> (axum::Router, Arc<AtomicUsize>) {
    let writes = Arc::new(AtomicUsize::new(0));
    let counter = writes.clone();
    let app = axum::Router::new()
        .route(
            "/update",
            axum::routing::post(move || {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    "write reached"
                }
            }),
        )
        .route(
            "/share/asof",
            axum::routing::post(|| async { auth::request_identity().unwrap().principal }),
        )
        .route(
            "/replicate/stream",
            axum::routing::post(|| async { "stream reached" }),
        )
        .route(
            "/replicate/challenge",
            axum::routing::post(|| async { "challenge reached" }),
        )
        .route(
            "/query",
            axum::routing::post(|| async { "public read reached" }),
        );
    let bytes = serde_json::to_vec(&serde_json::json!({"version":1,"credentials":[{
        "credential_id":"standby-fixture", "principal":"urn:standby:fixture",
        "audience":"quipu", "token_sha256":format!("{:x}", Sha256::digest(b"replica-fixture"))
    }]}))
    .unwrap();
    let registry = quipu::crew_credentials::CredentialRegistry::parse(&bytes, "quipu").unwrap();
    let policy = quipu::http_auth::BearerPolicy::new(Some("writer-fixture".into()), None, None, 1)
        .unwrap()
        .with_replication_only(registry)
        .unwrap();
    let store = Arc::new(crate::StoreHandle::writer_only(
        quipu::Store::open_in_memory().unwrap(),
    ));
    (
        layer(
            app,
            Config {
                auth_policy: policy,
                session_keys: Default::default(),
                attest_store: store,
                replica_config: None,
                read_only: false,
                is_relay: false,
                scoped_enabled: false,
            },
        ),
        writes,
    )
}

async fn call(app: axum::Router, path: &str, token: Option<&str>) -> axum::response::Response {
    let mut request = Request::post(path);
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    app.oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn replication_guard_binds_identity_and_refuses_mutation_before_dispatch() {
    let (app, writes) = fixture();
    for path in ["/share/asof", "/replicate/stream", "/replicate/challenge"] {
        assert_eq!(
            call(app.clone(), path, None).await.status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            call(app.clone(), path, Some("wrong-fixture"))
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let response = call(app.clone(), path, Some("replica-fixture")).await;
        assert_eq!(response.status(), StatusCode::OK);
        if path == "/share/asof" {
            assert_eq!(
                axum::body::to_bytes(response.into_body(), 1024)
                    .await
                    .unwrap(),
                "urn:standby:fixture"
            );
        }
    }
    assert_eq!(
        call(app.clone(), "/update", Some("replica-fixture"))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    assert_eq!(
        call(app.clone(), "/query", Some("replica-fixture"))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(app.clone(), "/query", None).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        call(app, "/update", Some("writer-fixture")).await.status(),
        StatusCode::OK
    );
    assert_eq!(writes.load(Ordering::SeqCst), 1);
}
