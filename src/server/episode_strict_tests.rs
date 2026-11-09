//! Real HTTP handler/auth boundary; no installed server or production data.
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    middleware,
    routing::post,
};
use quipu::http_auth::{AccessDecision, AuthGeneration, AuthenticatedPrincipal, BearerPolicy};
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;

fn app() -> (Router, crate::SharedStore) {
    let mut store = quipu::Store::open_in_memory().unwrap();
    store.set_base_ns(quipu::namespace::DEFAULT_BASE_NS.to_owned());
    let shape = format!(
        "@prefix sh: <http://www.w3.org/ns/shacl#> . <urn:s> a sh:NodeShape ; sh:targetClass <{}Document> .",
        store.base_ns()
    );
    quipu::tool_shapes(
        &mut store,
        &json!({"action":"load","name":"documents","turtle":shape}),
    )
    .unwrap();
    for graph in ["urn:test:strict", "urn:test:advisory"] {
        quipu::tool_graph_create(&store, &json!({"graph":graph})).unwrap();
    }
    quipu::tool_episode(
        &mut store,
        &json!({"name":"seed","nodes":[{"name":"document-alpha","type":"Document"}],"edges":[]}),
    )
    .unwrap();
    let state = Arc::new(crate::StoreHandle::writer_only(store));
    // A test-only shared bearer exercises the production identity adapter.
    let policy = Arc::new(BearerPolicy::new(Some("fixture".into()), None, None, 1).unwrap());
    let router = Router::new()
        .route("/episode", post(super::episode))
        .layer(middleware::from_fn(
            move |mut req: axum::extract::Request, next| {
                let policy = policy.clone();
                async move {
                    let header = req
                        .headers()
                        .get("authorization")
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_owned);
                    let authorization = quipu::http_auth::authorize_bearers(
                        true,
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
    (router, state)
}

fn input(name: &str, node: &str, strict: bool) -> Value {
    json!({"name":name,"graph":"urn:test:strict","strict_new_entities":strict,
        "nodes":[{"name":node,"type":"Document"}],"edges":[]})
}

async fn call(app: Router, input: Value, token: Option<&str>) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .uri("/episode")
        .method("POST")
        .header("content-type", "application/json")
        .header("x-quipu-principal", "urn:test:forged")
        .header("x-quipu-agent", "forged");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .oneshot(request.body(Body::from(input.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn strict_and_advisory_http_requests_do_not_share_options() {
    let (app, state) = app();
    let strict = input("strict", "document-alpha-variant", true);
    let mut advisory = strict.clone();
    advisory["name"] = json!("advisory");
    advisory["graph"] = json!("urn:test:advisory");
    advisory["strict_new_entities"] = json!(false);
    let (refused, admitted) = tokio::join!(
        call(app.clone(), strict, Some("fixture")),
        call(app.clone(), advisory, Some("fixture"))
    );
    assert_eq!(refused.0, StatusCode::BAD_REQUEST, "{refused:?}");
    assert_eq!(admitted.0, StatusCode::OK, "{admitted:?}");
    let locked = state.lock();
    assert!(!locked.resolution_config().enabled);
    assert!(!locked.resolution_config().strict_mode);
}

#[tokio::test]
async fn http_auth_target_and_positive_controls() {
    let (app, state) = app();
    let first = input("positive", "separate-independent-document", true);
    let before = state.lock().latest_tx_id().unwrap();
    assert_eq!(
        call(app.clone(), first.clone(), None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(app.clone(), first.clone(), Some("wrong")).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(before, state.lock().latest_tx_id().unwrap());
    assert_eq!(
        call(app.clone(), first.clone(), Some("fixture")).await.0,
        StatusCode::OK
    );
    let mut reuse = first;
    reuse["name"] = json!("reuse");
    assert_eq!(
        call(app.clone(), reuse, Some("fixture")).await.0,
        StatusCode::OK
    );
    let mut missing = input("missing", "other-independent-document", true);
    missing.as_object_mut().unwrap().remove("graph");
    let before = state.lock().latest_tx_id().unwrap();
    assert_eq!(
        call(app, missing, Some("fixture")).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(before, state.lock().latest_tx_id().unwrap());
}
