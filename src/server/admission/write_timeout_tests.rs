use super::*;
use axum::response::IntoResponse;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn hold_writer(
    shared: super::super::SharedStore,
) -> (std::sync::mpsc::Sender<()>, std::thread::JoinHandle<()>) {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let _held = shared.writer.lock();
        ready_tx.send(()).unwrap();
        let _released = release_rx.recv();
    });
    ready_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    (release_tx, thread)
}

async fn assert_write_not_started(error: AppError) {
    let response = error.into_response();
    assert_eq!(
        response.status(),
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(response.headers()[axum::http::header::RETRY_AFTER], "1");
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["code"], "write_not_started");
    assert_eq!(body["write_started"], false);
    assert!(body["waited_ms"].as_u64().unwrap() >= 20);
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("write did not run")
    );
    assert!(!body["error"].as_str().unwrap().contains("query_timeout_ms"));
}

#[tokio::test]
async fn expired_write_admission_never_invokes_handler() {
    let admission = Box::leak(Box::new(tokio::sync::Semaphore::new(1)));
    let permit = admission.acquire().await.unwrap();
    let ran = Arc::new(AtomicBool::new(false));
    let flag = ran.clone();
    let waiter = tokio::spawn(admit_with_deadline(
        admission,
        "write",
        Some(quipu::time::Deadline::after_millis(20)),
        std::time::Instant::now(),
        move || {
            flag.store(true, Ordering::SeqCst);
            Ok(())
        },
    ));
    tokio::time::sleep(std::time::Duration::from_millis(60)).await;
    drop(permit);
    assert_write_not_started(waiter.await.unwrap().unwrap_err()).await;
    assert!(!ran.load(Ordering::SeqCst));
    assert_eq!(admission.available_permits(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expired_writer_lock_never_invokes_tool() {
    let shared = Arc::new(super::super::StoreHandle::writer_only(
        quipu::Store::open_in_memory().unwrap(),
    ));
    let (release, holder) = hold_writer(shared.clone());
    let admission = Box::leak(Box::new(tokio::sync::Semaphore::new(1)));
    let ran = Arc::new(AtomicBool::new(false));
    let flag = ran.clone();
    let other = shared.clone();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let waiter = tokio::spawn(admit_with_deadline(
        admission,
        "write",
        Some(quipu::time::Deadline::after_millis(200)),
        std::time::Instant::now(),
        move || {
            entered_tx.send(()).unwrap();
            let _guard = other.write_lock()?;
            flag.store(true, Ordering::SeqCst);
            Ok(())
        },
    ));
    tokio::time::timeout(std::time::Duration::from_secs(2), entered_rx)
        .await
        .unwrap()
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    release.send(()).unwrap();
    holder.join().unwrap();
    assert_write_not_started(waiter.await.unwrap().unwrap_err()).await;
    assert!(!ran.load(Ordering::SeqCst));
    assert_eq!(admission.available_permits(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_write_handler_refuses_before_creating_a_graph() {
    let shared = Arc::new(super::super::StoreHandle::writer_only(
        quipu::Store::open_in_memory().unwrap(),
    ));
    let (release, holder) = hold_writer(shared.clone());
    let other = shared.clone();
    let waiter = tokio::spawn(TEST_REQUEST_BUDGET_MS.scope(
        200,
        super::super::tools::graph_create(
            axum::extract::State(other),
            axum::http::HeaderMap::new(),
            None,
            axum::Json(serde_json::json!({ "graph": "urn:timeout:control" })),
        ),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    release.send(()).unwrap();
    holder.join().unwrap();
    assert_write_not_started(waiter.await.unwrap().unwrap_err()).await;
    let graphs = quipu::tool_graph_list(&shared.read(), &serde_json::json!({})).unwrap();
    assert!(!graphs.to_string().contains("urn:timeout:control"));
    // Control: exactly the same real handler creates the graph without a budget.
    let _created = super::super::tools::graph_create(
        axum::extract::State(shared.clone()),
        axum::http::HeaderMap::new(),
        None,
        axum::Json(serde_json::json!({ "graph": "urn:timeout:control" })),
    )
    .await
    .unwrap();
    let graphs = quipu::tool_graph_list(&shared.read(), &serde_json::json!({})).unwrap();
    assert!(graphs.to_string().contains("urn:timeout:control"));
}

struct BudgetProvider {
    expire: bool,
}

impl quipu::EmbeddingProvider for BudgetProvider {
    fn embed_text(&self, _: &str) -> quipu::Result<Vec<f32>> {
        if self.expire {
            let deadline =
                quipu::time::request_deadline().expect("real handler admission deadline");
            std::thread::sleep(std::time::Duration::from_millis(
                deadline.remaining_millis() + 2,
            ));
        }
        Ok(vec![1.0, 0.0, 0.0])
    }
    fn dimension(&self) -> usize {
        3
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn episode_handler_refuses_expired_batch_then_commits_large_control() {
    let mut store = quipu::Store::open_in_memory().unwrap();
    store.resolution_config_mut().enabled = true;
    store.embedding_config_mut().auto_embed = false;
    store.set_embedding_provider(Arc::new(BudgetProvider { expire: true }));
    let shared = Arc::new(super::super::StoreHandle::writer_only(store));
    let input = |count: usize| {
        let nodes: Vec<_> = (0..count)
            .map(|i| {
                serde_json::json!({
                    "name": format!("http-document-{i}"), "type": "Document",
                    "description": "request budget control",
                    "properties": {"ordinal": i, "category": "control", "status": "committed"}
                })
            })
            .collect();
        serde_json::json!({"name": format!("http-batch-{count}"), "nodes": nodes, "edges": []})
    };
    let result = TEST_REQUEST_BUDGET_MS
        .scope(
            5000,
            super::super::tools::episode(
                axum::extract::State(shared.clone()),
                axum::Json(input(57)),
            ),
        )
        .await;
    let response = result.unwrap_err().into_response();
    assert_eq!(response.status(), axum::http::StatusCode::REQUEST_TIMEOUT);
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(body["error"].as_str().unwrap().contains("budget 5000ms"));
    assert!(!body["error"].as_str().unwrap().contains("exceeded 0ms"));
    assert!(shared.read().current_facts().unwrap().is_empty());
    shared
        .write_lock()
        .unwrap()
        .set_embedding_provider(Arc::new(BudgetProvider { expire: false }));
    let created = TEST_REQUEST_BUDGET_MS
        .scope(
            5000,
            super::super::tools::episode(
                axum::extract::State(shared.clone()),
                axum::Json(input(200)),
            ),
        )
        .await
        .unwrap()
        .0;
    assert_eq!(created["outcome"], "created");
    assert!(created["count"].as_u64().unwrap() >= 1200);
    assert!(shared.read().current_facts().unwrap().len() >= 1200);
}

#[tokio::test]
async fn expired_read_retains_query_timeout_response() {
    let admission = Box::leak(Box::new(tokio::sync::Semaphore::new(1)));
    let result = admit_with_deadline(
        admission,
        "read",
        Some(quipu::time::Deadline::after_millis(0)),
        std::time::Instant::now(),
        || {
            let store = quipu::Store::open_in_memory()?;
            let _result = quipu::sparql_query(&store, "SELECT ?s ?p ?o WHERE { ?s ?p ?o }")?;
            Ok(())
        },
    )
    .await;
    let response = result.unwrap_err().into_response();
    assert_eq!(response.status(), axum::http::StatusCode::REQUEST_TIMEOUT);
    assert!(
        response
            .headers()
            .get(axum::http::header::RETRY_AFTER)
            .is_none()
    );
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(body["error"].as_str().unwrap().contains("narrow the query"));
    assert!(body.get("write_started").is_none());
}

#[tokio::test]
async fn unexpired_write_runs_and_thread_context_is_restored() {
    let admission = Box::leak(Box::new(tokio::sync::Semaphore::new(1)));
    let result = admit_with_deadline(admission, "write", None, std::time::Instant::now(), || {
        reject_expired_write()?;
        Ok(42)
    })
    .await
    .unwrap();
    assert_eq!(result, 42);
    assert_eq!(admission.available_permits(), 1);
    assert!(WRITE_WAIT_START.with(std::cell::Cell::get).is_none());
    assert!(quipu::time::request_deadline().is_none());
}
