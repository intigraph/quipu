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

#[tokio::test]
async fn inflight_native_detached_promotion_outlives_its_http_response() {
    use serde_json::json;
    use sha2::Digest;
    let state = Arc::new(crate::StoreHandle::writer_only(
        quipu::Store::open_in_memory().unwrap(),
    ));
    let http = Arc::new(quipu::metrics::inflight::Registry::default());
    let store = Arc::new(quipu::metrics::inflight::Registry::default());
    let measured_http = http.clone();
    let measured_store = store.clone();
    let sequence = Arc::new(AtomicU64::new(0));
    let app = crate::snapshot_upload::routes()
        .with_state(state.clone())
        .layer(axum::middleware::from_fn(move |req, next| {
            let (sequence, http, store) = (sequence.clone(), http.clone(), store.clone());
            async move { log_request_tracked_stores(req, next, &sequence, http, store).await }
        }));
    let post = |path: &str, body: serde_json::Value| {
        app.clone().oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(body.to_string()))
                .unwrap(),
        )
    };
    let payload = "<urn:inflight:native> <http://www.w3.org/2000/01/rdf-schema#label> \"native lifetime control\" .";
    let hash = format!("sha256:{:x}", sha2::Sha256::digest(payload.as_bytes()));
    let upload_id = quipu::store::snapshot_upload::snapshot_upload_id("inflight-native", &hash);
    let stage = post("/knot/stage", json!({"upload_id":upload_id,"snapshot":"inflight-native","content_hash":hash,"part_hash":hash,"total_parts":1,"total_bytes":payload.len(),"part_number":0,"payload":payload})).await.unwrap();
    assert!(stage.status().is_success());
    let lock_owner = state.clone();
    let (locked_tx, locked_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let _held = lock_owner.writer.lock();
        locked_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    });
    locked_rx.await.unwrap();
    let response = post("/knot/promote", json!({"upload_id":upload_id}))
        .await
        .unwrap();
    assert!(response.status().is_success());
    let render = || {
        let mut text = String::new();
        measured_store.render(&mut text, true);
        text
    };
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while !render().contains("lane=\"write\",phase=\"active\"} 1") {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let mut http_text = String::new();
    measured_http.render(&mut http_text, false);
    assert!(http_text.contains("lane=\"write\"} 0"));
    assert!(render().contains("lane=\"write\",phase=\"active\"} 1"));
    release_tx.send(()).unwrap();
    holder.join().unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let response = post("/knot/promote", json!({"upload_id":upload_id}))
                .await
                .unwrap();
            let status = response.status();
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            assert!(status.is_success(), "{}", String::from_utf8_lossy(&bytes));
            let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            if value["pending"] != true && render().contains("lane=\"write\",phase=\"active\"} 0") {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let rows = quipu::tool_query(&state.read(), &json!({"query":"SELECT ?o WHERE { <urn:inflight:native> <http://www.w3.org/2000/01/rdf-schema#label> ?o }"})).unwrap();
    assert_eq!(rows["count"], 1);
}
