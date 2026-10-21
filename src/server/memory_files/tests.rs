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
        .route("/knot", post(crate::publication::knot))
        .route("/memory/commit", post(super::commit))
        .route("/memory/read", post(super::read))
        .route("/memory/export", post(super::export))
        .route("/memory/projection", post(super::projection))
        .route("/memory/receipt", post(super::receipt))
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
    assert_eq!(
        call(&app, "/memory/receipt", Some("alpha"), &request)
            .await
            .1["receipt"],
        Value::Null
    );
    let (status, receipt) = call(&app, "/memory/commit", Some("alpha"), &request).await;
    assert_eq!(status, StatusCode::OK);
    let commit_tx = state.lock().list_transactions().unwrap().last().unwrap().id;
    assert_eq!(
        call(&app, "/memory/receipt", Some("alpha"), &request)
            .await
            .1,
        receipt
    );
    assert_eq!(
        call(&app, "/memory/receipt", Some("beta"), &request)
            .await
            .0,
        StatusCode::FORBIDDEN
    );

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
    let typed_owner: quipu::memory_files::Owner = serde_json::from_value(owner.clone()).unwrap();
    let owner_iri = format!(
        "urn:quipu:memory:owner:{:x}",
        Sha256::digest(serde_json::to_vec(&typed_owner).unwrap())
    );
    let forged = json!({"graph":"urn:example:memory","source":"memory-file","actor":owner["crew"],
        "turtle":format!("<{owner_iri}> <https://quipu.dev/memory#head> \"forged\" .")});
    let (status, refusal) = call(&app, "/knot", Some("beta"), &forged).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(refusal.to_string().contains("reserved file-memory"));
    let ordinary = json!({"graph":"urn:example:memory","source":"memory-file",
        "turtle":"<urn:example:public> <urn:example:label> \"PUBLIC_CONTROL\" ."});
    assert_eq!(
        call(&app, "/knot", Some("beta"), &ordinary).await.0,
        StatusCode::OK
    );
    assert_eq!(call(&app, "/memory/export", None, &owner).await.1, export);
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
    let auth = store.transaction_auth(commit_tx).unwrap().unwrap();
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
