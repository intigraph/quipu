use super::*;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    middleware,
    routing::post,
};
use quipu::search_outcome::Operation;
use quipu::{Store, vector::KnowledgeVectorStore};
use serde_json::json;
use tower::ServiceExt;

fn app() -> Router {
    let store = Store::open_in_memory().unwrap();
    let id = store.intern("urn:memory").unwrap();
    store
        .embed_entity(id, "memory scheduler allocation", &[1.0, 0.0], "2026-01-01")
        .unwrap();
    let shared = Arc::new(crate::StoreHandle::writer_only(store));
    Router::new()
        .route("/search", post(crate::tools::search))
        .with_state(shared)
        .layer(middleware::from_fn(crate::request_middleware::log_request))
}

#[tokio::test]
async fn actual_http_search_carries_semantic_evidence_across_blocking_work() {
    for (text, expected) in [
        ("memory scheduler allocation", Outcome::Hits),
        ("quaffleplex scheduler allocation", Outcome::NoContext),
    ] {
        let call = Call::new(Surface::Http, Operation::Search);
        let request = Request::builder()
            .method("POST")
            .uri("/search")
            .header("content-type", "application/json")
            .extension(call.clone())
            .body(Body::from(
                json!({"query":text,"embedding":[1.0,0.0]}).to_string(),
            ))
            .unwrap();
        let response = app().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let event = call.terminal_event().expect("outer HTTP scope must finish");
        assert_eq!(event.summary.outcome, expected);
        assert_eq!(event.summary.candidate_count, Some(1));
        assert_eq!(
            event.summary.admitted_count,
            Some(usize::from(expected == Outcome::Hits))
        );
        assert_eq!(event.surface, Surface::Http);
        assert!(call.finish(Some(200), None).is_none());
    }
}

#[tokio::test]
async fn mcp_http_hop_keeps_final_ownership_until_projection_completes() {
    let call = Call::new(Surface::Mcp, Operation::Search);
    let request = Request::builder()
        .method("POST")
        .uri("/search")
        .header("content-type", "application/json")
        .extension(call.clone())
        .body(Body::from(
            json!({"query":"memory scheduler allocation","embedding":[1.0,0.0]}).to_string(),
        ))
        .unwrap();
    let response = app().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        call.terminal_event().is_none(),
        "HTTP must not count a second terminal MCP event"
    );
    // A projection/serialization failure after HTTP 200 is still a failure.
    let event = call.finish_mcp(Some(Outcome::Error)).unwrap();
    assert_eq!(event.status, Some(200));
    assert_eq!(event.summary, Summary::unavailable(Outcome::Error));
    assert!(call.finish_mcp(None).is_none());
}

#[tokio::test]
async fn refused_and_malformed_requests_never_become_zeros() {
    for (status, expected) in [
        (StatusCode::FORBIDDEN, Outcome::Refused),
        (StatusCode::REQUEST_TIMEOUT, Outcome::Timeout),
    ] {
        let router = Router::new()
            .route("/search", post(move || async move { status }))
            .layer(middleware::from_fn(crate::request_middleware::log_request));
        let call = Call::new(Surface::Http, Operation::Search);
        let request = Request::builder()
            .method("POST")
            .uri("/search")
            .extension(call.clone())
            .body(Body::empty())
            .unwrap();
        router.oneshot(request).await.unwrap();
        assert_eq!(
            call.terminal_event().unwrap().summary,
            Summary::unavailable(expected)
        );
    }
    let call = Call::new(Surface::Http, Operation::Search);
    let request = Request::builder()
        .method("POST")
        .uri("/search")
        .header("content-type", "application/json")
        .extension(call.clone())
        .body(Body::from("not-json"))
        .unwrap();
    assert_eq!(
        app().oneshot(request).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call.terminal_event().unwrap().summary,
        Summary::unavailable(Outcome::Error)
    );
}
