use super::*;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn headers(value: Option<&str>) -> HeaderMap {
    let mut result = HeaderMap::new();
    if let Some(value) = value {
        result.insert("x-quipu-timeout-ms", value.parse().unwrap());
    }
    result
}

async fn call(context: bool, h: HeaderMap) -> (StatusCode, serde_json::Value) {
    let mut store = quipu::Store::open_in_memory().unwrap();
    quipu::tool_knot(&mut store, &json!({"turtle":"<https://example.test/s> <http://www.w3.org/2000/01/rdf-schema#label> \"budgetneedle\" ."})).unwrap();
    let s = Arc::new(crate::StoreHandle::writer_only(store));
    let response = if context {
        crate::tools::context(
            State(s),
            h,
            axum::Json(json!({"query":"budgetneedle", "max_entities":1})),
        )
        .await
        .map(IntoResponse::into_response)
    } else {
        crate::query_endpoint::query(State(s), h, axum::Json(json!({"query":"ASK {}"}))).await
    }
    .unwrap_or_else(IntoResponse::into_response);
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

#[tokio::test]
async fn client_budget_http_zero_refuses_and_missing_or_generous_keeps_positive() {
    for context in [false, true] {
        let (status, _) = call(context, headers(Some("0"))).await;
        eprintln!(
            "endpoint={} remaining_ms=0 status={status}",
            if context { "/context" } else { "/query" }
        );
        assert_eq!(status, StatusCode::REQUEST_TIMEOUT);
        for value in [None, Some("10000")] {
            let (status, body) = call(context, headers(value)).await;
            eprintln!(
                "endpoint={} remaining_ms={value:?} status={status} positive={body}",
                if context { "/context" } else { "/query" }
            );
            assert_eq!(status, StatusCode::OK, "{body}");
            if context {
                assert_eq!(body["entities"].as_array().unwrap().len(), 1);
            } else {
                assert_eq!(body["result"], true);
            }
        }
    }
}

#[tokio::test]
async fn client_budget_http_malformed_overflow_and_duplicates_are_bad_requests() {
    for context in [false, true] {
        for value in ["", "-1", "+1", "1.5", "1,2", "18446744073709551616"] {
            assert_eq!(
                call(context, headers(Some(value))).await.0,
                StatusCode::BAD_REQUEST
            );
        }
        let mut h = headers(Some("10"));
        h.append("x-quipu-timeout-ms", "20".parse().unwrap());
        assert_eq!(call(context, h).await.0, StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn client_budget_clamps_store_and_request_caps_but_absent_preserves_legacy() {
    let mut config = quipu::SearchConfig {
        query_timeout_ms: 20,
        ..Default::default()
    };
    TEST_REQUEST_BUDGET_MS
        .scope(100, async {
            let b = ReadBudget::from_headers(&headers(Some("10000")), &config).unwrap();
            assert!(b.deadline.unwrap().remaining_millis() <= 20);
        })
        .await;
    config.query_timeout_ms = 100;
    TEST_REQUEST_BUDGET_MS
        .scope(20, async {
            let b = ReadBudget::from_headers(&headers(Some("10000")), &config).unwrap();
            assert!(b.deadline.unwrap().remaining_millis() <= 20);
        })
        .await;
    TEST_REQUEST_BUDGET_MS
        .scope(0, async {
            assert!(
                ReadBudget::from_headers(&headers(None), &config)
                    .unwrap()
                    .deadline
                    .is_none()
            );
            assert!(
                ReadBudget::from_headers(&headers(Some("0")), &config)
                    .unwrap()
                    .deadline
                    .unwrap()
                    .passed()
            );
        })
        .await;
}

#[tokio::test]
async fn client_budget_ordinary_queue_expires_without_running() {
    let lane = Box::leak(Box::new(tokio::sync::Semaphore::new(1)));
    let held = lane.acquire().await.unwrap();
    let result = tokio::time::timeout(
        Duration::from_millis(250),
        admit_with_deadline(
            lane,
            "client-read",
            Some(quipu::time::Deadline::after_millis(15)),
            Instant::now(),
            || -> Result<(), AppError> { panic!("expired waiter must never execute") },
        ),
    )
    .await
    .expect("queue must expire, not wait for a permit");
    assert!(result.is_err());
    assert_eq!(lane.available_permits(), 0);
    drop(held);
    assert_eq!(lane.available_permits(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_budget_execution_retains_same_deadline_and_refuses_late_success() {
    let lane = Box::leak(Box::new(tokio::sync::Semaphore::new(1)));
    let budget = quipu::time::Deadline::after_millis(10);
    let result = admit_with_deadline(
        lane,
        "client-read",
        Some(budget),
        Instant::now(),
        move || {
            assert_eq!(quipu::time::request_deadline(), Some(budget));
            std::thread::sleep(Duration::from_millis(25));
            Ok(())
        },
    )
    .await;
    assert!(result.is_err());
    assert_eq!(lane.available_permits(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_budget_real_query_interrupts_and_next_query_remains_valid() {
    let mut store = quipu::Store::open_in_memory().unwrap();
    let turtle = (0..150)
        .map(|i| {
            format!(
                "<https://example.test/s{i}> <https://example.test/p> <https://example.test/o{i}> ."
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    quipu::tool_knot(&mut store, &json!({"turtle":turtle})).unwrap();
    let s = Arc::new(crate::StoreHandle::writer_only(store));
    let response = crate::query_endpoint::query(State(s.clone()), headers(Some("5")), axum::Json(json!({"query":"SELECT (COUNT(*) AS ?n) WHERE { ?a <https://example.test/p> ?x . ?b <https://example.test/p> ?y . ?c <https://example.test/p> ?z }"})))
        .await.unwrap_or_else(IntoResponse::into_response);
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    let response = crate::query_endpoint::query(State(s), headers(Some("1000")), axum::Json(json!({"query":"ASK { <https://example.test/s0> <https://example.test/p> <https://example.test/o0> }"}))).await.unwrap();
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["result"],
        true
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_budget_cancellation_keeps_permit_with_actual_blocking_work() {
    let lane = Box::leak(Box::new(tokio::sync::Semaphore::new(1)));
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let task = tokio::spawn(admit_with_deadline(
        lane,
        "client-read",
        Some(quipu::time::Deadline::after_millis(1000)),
        Instant::now(),
        move || {
            let _ = ready_tx.send(());
            release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            Ok(())
        },
    ));
    ready_rx.await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(
        lane.available_permits(),
        0,
        "detached work still owns admission"
    );
    release_tx.send(()).unwrap();
    let permit = tokio::time::timeout(Duration::from_millis(500), lane.acquire())
        .await
        .unwrap()
        .unwrap();
    drop(permit);
    assert_eq!(lane.available_permits(), 1);
}
