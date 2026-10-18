//! In-process HTTP routing with actual registry verification; no listener.
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    middleware,
    routing::post,
};
use quipu::http_auth::{AccessDecision, AuthGeneration, AuthenticatedPrincipal, BearerPolicy};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tower::ServiceExt;

fn fixture() -> (Router, crate::SharedStore) {
    let rows = ["alpha", "beta"].map(|name| {
        json!({"credential_id":name,
        "principal":format!("urn:example:{name}"),"audience":"quipu",
        "token_sha256":format!("{:x}",Sha256::digest(name.as_bytes()))})
    });
    let registry = quipu::crew_credentials::CredentialRegistry::parse(
        &serde_json::to_vec(&json!({"version":1,"credentials":rows})).unwrap(),
        "quipu",
    )
    .unwrap();
    let policy = Arc::new(
        BearerPolicy::new(Some("shared".into()), None, None, 1)
            .unwrap()
            .with_named(registry),
    );
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
        .route("/memory/commit", post(super::commit))
        .route("/memory/read", post(super::read))
        .route("/memory/export", post(super::export))
        .route("/memory/projection", post(super::projection))
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024))
        .layer(axum::Extension(config))
        .layer(middleware::from_fn(
            move |mut req: axum::extract::Request, next| {
                let policy = policy.clone();
                async move {
                    let header = req
                        .headers()
                        .get("authorization")
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_owned);
                    let write =
                        quipu::http_auth::is_write_request(req.uri().path(), req.method().as_str());
                    let required =
                        write || quipu::http_auth::requires_read_identity(req.uri().path());
                    let authorization = quipu::http_auth::authorize_bearers(
                        required,
                        false,
                        &policy,
                        header.as_deref(),
                        1,
                    );
                    if authorization.decision != AccessDecision::Allow {
                        return axum::response::Response::builder()
                            .status(401)
                            .body(Body::empty())
                            .unwrap();
                    }
                    if authorization.generation == Some(AuthGeneration::Current) {
                        req.extensions_mut()
                            .insert(AuthenticatedPrincipal::LEGACY_SHARED_BEARER);
                    }
                    crate::auth::run_authorized(
                        req,
                        next,
                        &policy,
                        authorization,
                        header.as_deref(),
                    )
                    .await
                }
            },
        ))
        .with_state(state.clone());
    (app, state)
}

async fn call(app: &Router, path: &str, token: Option<&str>, body: &Value) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .uri(path)
        .method("POST")
        .header("content-type", "application/json")
        .header("x-quipu-principal", "urn:example:alpha");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 8 * 1024 * 1024)
        .await
        .unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

#[tokio::test]
async fn own_writer_binding_replay_conflict_and_cross_crew_read() {
    let (app, state) = fixture();
    let owner = json!({"deployment":"urn:example:deployment","project":"urn:example:project","crew":"urn:example:alpha"});
    let mut request = json!({"owner":owner,"path":"MEMORY.md","text":"α\r\nexact source",
        "expected_snapshot":null,"expected_file":null,"idempotency_key":"one","harness":"fixture","account":"selected"});
    assert_eq!(
        call(&app, "/memory/commit", None, &request).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "/memory/commit", Some("shared"), &request)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, "/memory/commit", Some("beta"), &request).await.0,
        StatusCode::FORBIDDEN
    );
    let (status, receipt) = call(&app, "/memory/commit", Some("alpha"), &request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        call(&app, "/memory/commit", Some("alpha"), &request)
            .await
            .1,
        receipt
    );
    let (status, export) = call(&app, "/memory/export", None, &owner).await;
    assert_eq!(status, StatusCode::OK); // intentional inherited cross-crew read
    assert_eq!(export["files"][0]["text"], "α\r\nexact source");
    assert_eq!(
        export["snapshot"]["revision"],
        receipt["receipt"]["snapshot"]
    );
    assert_eq!(
        call(&app, "/memory/projection", None, &owner).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "/memory/projection", Some("shared"), &owner)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, "/memory/projection", Some("beta"), &owner)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (status, projection) = call(&app, "/memory/projection", Some("alpha"), &owner).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(projection["authenticated_owner"], owner["crew"]);
    assert_eq!(projection["export"], export);
    request["idempotency_key"] = json!("stale-new-request");
    assert_eq!(
        call(&app, "/memory/commit", Some("alpha"), &request)
            .await
            .0,
        StatusCode::CONFLICT
    );
    request["owner"]["project"] = json!("urn:example:unrelated");
    assert_eq!(
        call(&app, "/memory/commit", Some("alpha"), &request)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let store = state.lock();
    let tx = store.list_transactions().unwrap().last().unwrap().id;
    let auth = store.transaction_auth(tx).unwrap().unwrap();
    assert_eq!(auth.principal, "urn:example:alpha");
    assert_eq!(auth.auth_class, "named_bearer");
}

#[tokio::test]
async fn oversized_body_refuses_before_file_parsing_or_storage() {
    let (app, state) = fixture();
    let app = app.layer(axum::extract::DefaultBodyLimit::max(64 * 1024 * 1024));
    let request = Request::builder()
        .uri("/memory/commit")
        .method("POST")
        .header("content-type", "application/json")
        .header("authorization", "Bearer alpha")
        .body(Body::from("x".repeat(1024 * 1024 + 1)))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(state.lock().list_transactions().unwrap().is_empty());
}
