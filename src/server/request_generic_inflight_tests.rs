use super::*;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

#[tokio::test]
async fn inflight_generic_knot_promote_and_graph_store_survive_http_cancel() {
    for path in ["/knot", "/knot/promote", "/rdf-graph-store"] {
        let http = Arc::new(quipu::metrics::inflight::Registry::default());
        let store = Arc::new(quipu::metrics::inflight::Registry::default());
        let measured_http = http.clone();
        let measured_store = store.clone();
        let sequence = Arc::new(AtomicU64::new(0));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let gate = Arc::new(Mutex::new(Some((started_tx, release_rx))));
        let app = axum::Router::new()
            .route(
                path,
                axum::routing::post(move || {
                    let gate = gate.clone();
                    async move {
                        let (started_tx, release_rx) = gate.lock().unwrap().take().unwrap();
                        crate::base::blocking(move || {
                            let _ = started_tx.send(());
                            let _ = release_rx.recv();
                            Ok(axum::http::StatusCode::NO_CONTENT)
                        })
                        .await
                    }
                }),
            )
            .layer(axum::middleware::from_fn(move |req, next| {
                let (sequence, http, store) = (sequence.clone(), http.clone(), store.clone());
                async move { log_request_tracked_stores(req, next, &sequence, http, store).await }
            }));
        let request = axum::http::Request::builder()
            .uri(path)
            .method("POST")
            .body(axum::body::Body::empty())
            .unwrap();
        let work = tokio::spawn(app.oneshot(request));
        started_rx.await.unwrap();
        let render = || {
            let mut text = String::new();
            measured_store.render(&mut text, true);
            text
        };
        assert!(
            render().contains("lane=\"write\",phase=\"active\"} 1"),
            "missing generic work for {path}"
        );
        work.abort();
        assert!(work.await.unwrap_err().is_cancelled());
        let mut http_text = String::new();
        measured_http.render(&mut http_text, false);
        assert!(http_text.contains("lane=\"write\"} 0"));
        assert!(render().contains("lane=\"write\",phase=\"active\"} 1"));
        release_tx.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if render().contains("lane=\"write\",phase=\"active\"} 0") {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
}
