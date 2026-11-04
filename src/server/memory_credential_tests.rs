//! Actual production auth middleware and memory handlers, no network listener.
use super::*;
use axum::{Router, body::Body, http::Request, routing::post};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tower::ServiceExt;

fn fixture(scope_graph: &str, expiry: u64) -> (Router, crate::SharedStore) {
    let rows = json!([{
        "credential_id":"memory", "principal":"urn:example:alpha", "audience":"quipu-memory",
        "token_sha256":format!("{:x}",Sha256::digest(b"memory-fixture")),
        "expires_at_epoch_secs":expiry,
        "memory_scope":{"graph":scope_graph,"deployment":"urn:example:deployment","project":"urn:example:project"}
    },{
        "credential_id":"legacy", "principal":"urn:example:alpha", "audience":"quipu",
        "token_sha256":format!("{:x}",Sha256::digest(b"legacy-fixture"))
    }]);
    let registry = quipu::crew_credentials::CredentialRegistry::parse(
        &serde_json::to_vec(&json!({"version":1,"credentials":rows})).unwrap(),
        "quipu",
    )
    .unwrap();
    let policy = quipu::http_auth::BearerPolicy::new(Some("shared-fixture".into()), None, None, 1)
        .unwrap()
        .with_named(registry);
    let store = quipu::Store::open_in_memory().unwrap();
    store.graph_create("urn:example:memory").unwrap();
    let state = Arc::new(crate::StoreHandle::writer_only(store));
    let config = quipu::config::MemoryFilesConfig {
        enabled: true,
        graph: "urn:example:memory".into(),
        deployment: "urn:example:deployment".into(),
        project: "urn:example:project".into(),
    };
    let app = Router::new()
        .route("/memory/commit", post(crate::memory_files::commit))
        .route("/memory/projection", post(crate::memory_files::projection))
        .route("/memory/receipt", post(crate::memory_files::receipt))
        .layer(axum::middleware::from_fn(crate::memory_files::scope_guard))
        .layer(axum::Extension(config))
        .route("/query", post(|| async { "read reached" }))
        .route("/update", post(|| async { "write reached" }));
    let app = layer(
        app,
        Config {
            auth_policy: policy,
            session_keys: Default::default(),
            attest_store: state.clone(),
            replica_config: None,
            read_only: false,
            is_relay: false,
            scoped_enabled: false,
        },
    )
    .with_state(state.clone());
    (app, state)
}

async fn call(app: &Router, path: &str, token: &str, body: &Value) -> StatusCode {
    app.clone()
        .oneshot(
            Request::post(path)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

fn commit() -> Value {
    json!({"owner":{"deployment":"urn:example:deployment","project":"urn:example:project",
        "crew":"urn:example:alpha"},"path":"MEMORY.md","text":"synthetic own memory",
        "expected_snapshot":null,"expected_file":null,"idempotency_key":"one",
        "harness":"fixture","account":"selected"})
}

#[tokio::test]
async fn own_memory_succeeds_but_other_owner_and_ordinary_operations_refuse() {
    let (app, state) = fixture("urn:example:memory", quipu::time::epoch_secs() + 3600);
    let request = commit();
    assert_eq!(
        call(&app, "/memory/commit", "memory-fixture", &request).await,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "/memory/receipt", "memory-fixture", &request).await,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "/memory/projection",
            "memory-fixture",
            &request["owner"]
        )
        .await,
        StatusCode::OK
    );
    let before = state.lock().list_transactions().unwrap().len();
    let mut other = request.clone();
    other["owner"]["crew"] = json!("urn:example:beta");
    assert_eq!(
        call(&app, "/memory/commit", "memory-fixture", &other).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "/memory/projection",
            "memory-fixture",
            &other["owner"]
        )
        .await,
        StatusCode::FORBIDDEN
    );
    for path in ["/query", "/update"] {
        assert_eq!(
            call(&app, path, "memory-fixture", &json!({})).await,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(state.lock().list_transactions().unwrap().len(), before);
    assert_eq!(
        call(&app, "/update", "legacy-fixture", &json!({})).await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn expired_and_wrong_repository_credentials_never_dispatch_a_write() {
    for (graph, expiry, status) in [
        ("urn:example:memory", 1, StatusCode::UNAUTHORIZED),
        (
            "urn:example:other",
            quipu::time::epoch_secs() + 3600,
            StatusCode::FORBIDDEN,
        ),
    ] {
        let (app, state) = fixture(graph, expiry);
        let before = state.lock().list_transactions().unwrap().len();
        assert_eq!(
            call(&app, "/memory/commit", "memory-fixture", &commit()).await,
            status
        );
        assert_eq!(state.lock().list_transactions().unwrap().len(), before);
    }
}
