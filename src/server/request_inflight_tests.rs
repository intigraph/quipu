use super::*;
use std::future::Future;
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test]
async fn inflight_pending_http_is_classified_and_cancelled_on_real_middleware_path() {
    for (path, method, lane) in [
        ("/set", "POST", "write"),
        ("/query", "POST", "read"),
        ("/rdf-graph-store", "PUT", "write"),
        ("/rdf-graph-store", "GET", "read"),
        ("/private-unclassified-control", "GET", "other"),
    ] {
        let registry = Arc::new(quipu::metrics::inflight::Registry::default());
        let measured = registry.clone();
        let sequence = Arc::new(AtomicU64::new(0));
        let app = axum::Router::new()
            .route(
                path,
                axum::routing::any(|| async {
                    std::future::pending::<axum::response::Response>().await
                }),
            )
            .layer(axum::middleware::from_fn(move |req, next| {
                let sequence = sequence.clone();
                let registry = registry.clone();
                async move { log_request_tracked(req, next, &sequence, registry).await }
            }));
        let request = axum::http::Request::builder()
            .uri(path)
            .method(method)
            .body(axum::body::Body::empty())
            .unwrap();
        let mut future = Box::pin(app.oneshot(request));
        std::future::poll_fn(|cx| {
            assert!(future.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        let mut text = String::new();
        measured.render(&mut text, false);
        assert!(text.contains(&format!(
            "quipu_http_requests_in_flight{{lane=\"{lane}\"}} 1\n"
        )));
        drop(future);
        let mut text = String::new();
        measured.render(&mut text, false);
        assert!(text.contains(&format!(
            "quipu_http_requests_in_flight{{lane=\"{lane}\"}} 0\n"
        )));
        assert!(text.contains("quipu_http_oldest_write_age_seconds 0\n"));
    }
}

#[test]
fn inflight_series_are_in_the_actual_metrics_handler_render_path() {
    let mut text = String::new();
    render_request_starts(&mut text);
    for name in [
        "quipu_http_requests_in_flight",
        "quipu_store_requests_in_flight",
        "quipu_http_oldest_write_age_seconds",
        "quipu_store_oldest_write_age_seconds",
    ] {
        assert!(text.contains(&format!("# TYPE {name} gauge\n")));
    }
}
