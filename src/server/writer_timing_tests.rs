use super::*;
use crate::handle::StoreHandle;
use std::time::Duration;

fn context(endpoint: &str) -> Context {
    Context {
        endpoint: endpoint.into(),
        registry: Arc::new(Registry::default()),
    }
}
fn snapshot(context: &Context, access: &'static str) -> (u64, f64) {
    context
        .registry
        .0
        .lock()
        .unwrap()
        .get(&(context.endpoint.to_string(), access))
        .copied()
        .unwrap_or_default()
}

#[test]
fn writer_timing_multiple_holds_error_unwind_and_reader_fallback() {
    let handle = StoreHandle::writer_only(quipu::Store::open_in_memory().unwrap());
    let context = context("/knot/promote");
    scoped(Some(context.clone()), || {
        let result: Result<(), ()> = {
            let _guard = handle.write_lock().unwrap();
            Err(())
        };
        assert!(result.is_err());
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = handle.lock();
            panic!("private unwind control");
        }));
        assert!(panic.is_err());
        drop(handle.read());
        drop(handle.writer_barrier());
    });
    assert_eq!(snapshot(&context, "writer").0, 2);
    assert!(snapshot(&context, "writer").1 > 0.0);
    assert_eq!(snapshot(&context, "read-fallback").0, 1);
    assert_eq!(snapshot(&context, "barrier").0, 1);
}

#[test]
fn writer_timing_wait_and_post_unlock_work_are_not_held() {
    let handle = Arc::new(StoreHandle::writer_only(
        quipu::Store::open_in_memory().unwrap(),
    ));
    let held = handle.writer_barrier();
    let context = context("/knot");
    let worker_context = context.clone();
    let worker = handle.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        scoped(Some(worker_context), || {
            ready_tx.send(()).unwrap();
            drop(worker.lock());
            // Deferred work after unlock must not add another hold.
            std::thread::sleep(Duration::from_millis(30));
        });
    });
    ready_rx.recv().unwrap();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(snapshot(&context, "writer"), (0, 0.0));
    drop(held);
    thread.join().unwrap();
    let (count, seconds) = snapshot(&context, "writer");
    assert_eq!(count, 1);
    assert!(
        seconds < 0.04,
        "acquisition wait or deferred work counted: {seconds}"
    );
}

#[test]
fn writer_timing_pool_reader_does_not_consume_writer_metric() {
    let (_dir, handle) = crate::tests::pooled_handle(2);
    let context = context("/query");
    scoped(Some(context.clone()), || drop(handle.read()));
    assert!(context.registry.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn writer_timing_real_http_blocking_context_reaches_knot_and_promote() {
    use tower::ServiceExt;
    for path in ["/knot", "/knot/promote"] {
        let handle = Arc::new(StoreHandle::writer_only(
            quipu::Store::open_in_memory().unwrap(),
        ));
        let before = registry()
            .0
            .lock()
            .unwrap()
            .get(&(path.into(), "writer"))
            .copied()
            .unwrap_or_default();
        let app = axum::Router::new()
            .route(
                path,
                axum::routing::post(move || {
                    let handle = handle.clone();
                    async move {
                        crate::base::blocking(move || {
                            drop(handle.lock());
                            Ok(axum::http::StatusCode::NO_CONTENT)
                        })
                        .await
                    }
                }),
            )
            .layer(axum::middleware::from_fn(
                crate::request_middleware::log_request,
            ));
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(path)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::NO_CONTENT);
        let after = registry()
            .0
            .lock()
            .unwrap()
            .get(&(path.into(), "writer"))
            .copied()
            .unwrap();
        assert!(after.0 > before.0);
        assert!(after.1 > before.1);
    }
}

#[tokio::test]
async fn writer_timing_native_knot_stage_and_detached_promotion() {
    use serde_json::json;
    use tower::ServiceExt;
    let state = Arc::new(StoreHandle::writer_only(
        quipu::Store::open_in_memory().unwrap(),
    ));
    let app = crate::snapshot_upload::routes()
        .with_state(state.clone())
        .layer(axum::middleware::from_fn(
            crate::request_middleware::log_request,
        ));
    let post = |path: &str, body: serde_json::Value| {
        let request = axum::http::Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body.to_string()))
            .unwrap();
        app.clone().oneshot(request)
    };
    let payload = "<urn:writer-timing:native> <http://www.w3.org/2000/01/rdf-schema#label> \"native hold control\" .";
    let response = post("/knot", json!({"turtle":payload})).await.unwrap();
    assert!(response.status().is_success());
    use sha2::Digest;
    let hash = format!("sha256:{:x}", sha2::Sha256::digest(payload.as_bytes()));
    let upload_id =
        quipu::store::snapshot_upload::snapshot_upload_id("writer-timing-native", &hash);
    let before = registry()
        .0
        .lock()
        .unwrap()
        .get(&("/knot/promote".into(), "writer"))
        .copied()
        .unwrap_or_default();
    let stage = post("/knot/stage", json!({"upload_id":upload_id,"snapshot":"writer-timing-native","content_hash":hash,"part_hash":hash,"total_parts":1,"total_bytes":payload.len(),"part_number":0,"payload":payload})).await.unwrap();
    let stage_status = stage.status();
    let stage_body = axum::body::to_bytes(stage.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(
        stage_status.is_success(),
        "{}",
        String::from_utf8_lossy(&stage_body)
    );
    let promote = post("/knot/promote", json!({"upload_id":upload_id}))
        .await
        .unwrap();
    assert!(promote.status().is_success());
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let after = registry()
                .0
                .lock()
                .unwrap()
                .get(&("/knot/promote".into(), "writer"))
                .copied()
                .unwrap_or_default();
            if after.0 > before.0 && after.1 > before.1 {
                println!(
                    "native promote completed writer holds={} held_seconds={}",
                    after.0 - before.0,
                    after.1 - before.1
                );
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
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
            if value["pending"] != true {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let rows = quipu::tool_query(&state.read(), &json!({"query":"SELECT ?o WHERE { <urn:writer-timing:native> <http://www.w3.org/2000/01/rdf-schema#label> ?o }"})).unwrap();
    assert_eq!(rows["count"], 1);
}
