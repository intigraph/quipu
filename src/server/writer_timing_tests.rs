use super::*;
use crate::handle::StoreHandle;
use std::sync::Arc;
use std::time::Duration;

fn provenance(client: &str, endpoint: &str) -> Arc<quipu::write_provenance::RequestProvenance> {
    Arc::new(quipu::write_provenance::RequestProvenance::classify(
        client,
        endpoint,
        Default::default(),
    ))
}
fn snapshot(client: &str, endpoint: &str) -> (u64, f64) {
    registry()
        .0
        .lock()
        .unwrap()
        .get(&(endpoint.into(), client.into()))
        .copied()
        .unwrap_or_default()
}

#[test]
fn writer_timing_error_unwind_and_cancellation_release_actual_guard() {
    let handle = StoreHandle::writer_only(quipu::Store::open_in_memory().unwrap());
    let client = "hold-errors";
    quipu::write_provenance::scoped(Some(provenance(client, "/knot")), || {
        let result: Result<(), ()> = {
            let _held = handle.lock();
            Err(())
        };
        assert!(result.is_err());
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _held = handle.write_lock().unwrap();
                panic!("unwind control");
            }))
            .is_err()
        );
        // Cancelled admission must not become a completed acquisition.
        let control = quipu::promotion_control::PromotionControl::new(Duration::ZERO);
        assert!(handle.promotion_lock(&control).is_err());
    });
    let (count, seconds) = snapshot(client, "/knot");
    assert_eq!(count, 2);
    assert!(seconds > 0.0);
    assert!(handle.writer.try_lock().is_some());
}

#[test]
fn writer_timing_wait_and_post_unlock_work_are_excluded() {
    let handle = Arc::new(StoreHandle::writer_only(
        quipu::Store::open_in_memory().unwrap(),
    ));
    let held = handle.writer.lock();
    let copy = handle.clone();
    let (ready, wait) = std::sync::mpsc::channel();
    let (report, reported) = std::sync::mpsc::channel();
    let (finish, finished) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        quipu::write_provenance::scoped(Some(provenance("hold-wait", "/knot")), || {
            ready.send(()).unwrap();
            let guard = copy.lock();
            let started = guard.started;
            drop(guard);
            report
                .send((started, snapshot("hold-wait", "/knot")))
                .unwrap();
            // The request remains alive doing post-unlock work. No elapsed
            // upper bound can fail merely because this thread is descheduled.
            finished.recv().unwrap();
        });
    });
    wait.recv().unwrap();
    assert_eq!(snapshot("hold-wait", "/knot"), (0, 0.0));
    let before_release = Instant::now();
    drop(held);
    let (started, recorded) = reported.recv().unwrap();
    assert!(started >= before_release, "timer included acquisition wait");
    assert_eq!(recorded.0, 1);
    assert!(recorded.1 > 0.0);
    assert_eq!(snapshot("hold-wait", "/knot"), recorded);
    finish.send(()).unwrap();
    thread.join().unwrap();
    assert_eq!(snapshot("hold-wait", "/knot"), recorded);
}

#[test]
fn writer_timing_read_interfaces_are_not_explicit_writer_holds() {
    let handle = StoreHandle::writer_only(quipu::Store::open_in_memory().unwrap());
    quipu::write_provenance::scoped(Some(provenance("hold-read", "/knot")), || {
        drop(handle.read());
        drop(handle.point_read());
        drop(handle.vector_read());
    });
    assert_eq!(snapshot("hold-read", "/knot"), (0, 0.0));
    let (_dir, pooled) = crate::tests::pooled_handle(2);
    quipu::write_provenance::scoped(Some(provenance("hold-read", "/knot")), || {
        drop(pooled.read());
    });
    assert_eq!(snapshot("hold-read", "/knot"), (0, 0.0));
}

#[test]
fn writer_timing_labels_and_registry_are_bounded() {
    let registry = Registry::default();
    for n in 0..1000 {
        registry.observe(("/knot".into(), format!("client-{n}")), 1.0);
    }
    assert_eq!(registry.0.lock().unwrap().len(), MAX_SERIES + 1);
    let mut output = String::new();
    registry.render(&mut output);
    assert!(output.contains("endpoint=\"/knot\",client=\"other\""));
    quipu::write_provenance::scoped(
        Some(provenance("BAD\"\nclient", "/caller/path/123")),
        || {
            assert_eq!(labels(), ("other".into(), "bad".into()));
        },
    );
}

#[test]
fn writer_timing_endpoint_survives_client_saturation() {
    let registry = Registry::default();
    for n in 0..300 {
        registry.observe(("/knot/promote".into(), format!("client-{n}")), 1.0);
    }
    registry.observe(("/knot".into(), "late-client".into()), 2.5);
    let mut output = String::new();
    registry.render(&mut output);
    assert!(
        output.contains(
            "quipu_writer_lock_held_seconds_total{endpoint=\"/knot\",client=\"late-client\"} 2.5"
        ),
        "late knot endpoint disappeared after caller saturation"
    );
    assert!(!output.contains("endpoint=\"overflow\""));
}

#[test]
fn writer_timing_each_endpoint_keeps_its_overflow_and_known_clients() {
    let registry = Registry::default();
    for endpoint in [
        "other",
        "internal",
        "/knot",
        "/knot/promote",
        "/import/promote",
    ] {
        for n in 0..300 {
            registry.observe((endpoint.into(), format!("client-{n}")), 1.0);
        }
        registry.observe((endpoint.into(), "tail-client".into()), 3.0);
        registry.observe((endpoint.into(), "client-0".into()), 2.0);
        let entries = registry.0.lock().unwrap();
        assert_eq!(
            entries.get(&(endpoint.into(), "other".into())),
            Some(&(45, 47.0))
        );
        assert_eq!(
            entries.get(&(endpoint.into(), "client-0".into())),
            Some(&(2, 3.0))
        );
        assert_eq!(
            entries
                .keys()
                .filter(|(route, _)| route == endpoint)
                .count(),
            MAX_SERIES + 1
        );
    }
    assert_eq!(registry.0.lock().unwrap().len(), 5 * (MAX_SERIES + 1));
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
            .header("x-quipu-client", "hold-native")
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
    let before = snapshot("hold-native", "/knot/promote");
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
            let after = snapshot("hold-native", "/knot/promote");
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
    assert!(snapshot("hold-native", "/knot").1 > 0.0);
    let mut metrics = String::new();
    render(&mut metrics);
    assert!(metrics.contains(
        "quipu_writer_lock_held_seconds_total{endpoint=\"/knot/promote\",client=\"hold-native\"}"
    ));
}
