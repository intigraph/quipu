use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    routing::post,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

use super::super::{SharedStore, StoreHandle};
use super::{READING, share_asof, spool_dir};

const G: &str = "https://example.org/project/board";

fn app() -> (Router, String) {
    let mut store = quipu::Store::open_in_memory().unwrap();
    let graph = store.graph_create(G).unwrap();
    quipu::rdf::ingest_rdf_to_graph(
        &mut store,
        b"<https://example.org/item/one> <https://example.org/title> \"one\" .\n\
          <https://example.org/item/two> <https://example.org/title> \"two\" ."
            .as_slice(),
        oxrdfio::RdfFormat::NTriples,
        None,
        "2026-01-01T00:00:00Z",
        None,
        None,
        graph,
    )
    .unwrap();
    let identity = store.store_id().unwrap();
    let shared: SharedStore = Arc::new(StoreHandle::writer_only(store));
    let app = Router::new()
        .route("/share/asof", post(share_asof))
        .with_state(shared);
    (app, identity)
}

async fn post_cut(app: Router, input: Value) -> (StatusCode, Option<String>, String) {
    let response = app
        .oneshot(
            Request::post("/share/asof")
                .header("content-type", "application/json")
                .body(Body::from(input.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let retry = response
        .headers()
        .get("retry-after")
        .map(|v| v.to_str().unwrap().to_string());
    let bytes = to_bytes(response.into_body(), 64 * 1024 * 1024)
        .await
        .unwrap();
    (status, retry, String::from_utf8(bytes.to_vec()).unwrap())
}

/// `(event, id, data)` for each SSE event in a body.
fn events(body: &str) -> Vec<(String, Option<String>, String)> {
    body.split("\n\n")
        .filter(|block| !block.trim().is_empty())
        .filter_map(|block| {
            let mut event = None;
            let mut id = None;
            let mut data = None;
            for line in block.lines() {
                if let Some(v) = line.strip_prefix("event: ") {
                    event = Some(v.to_string());
                } else if let Some(v) = line.strip_prefix("id: ") {
                    id = Some(v.to_string());
                } else if let Some(v) = line.strip_prefix("data: ") {
                    data = Some(v.to_string());
                }
            }
            Some((event?, id, data?))
        })
        .collect()
}

/// One test, so the process-wide read-phase lock is never contended by a
/// sibling test: busy first, then a real cut, then a refusal.
#[tokio::test]
async fn a_cut_streams_begin_parts_commit_and_refusals_come_first() {
    let (app, identity) = app();
    let input = json!({"graphs": [G], "expected_store_id": identity});

    // A read phase already running: refused with Retry-After, not queued.
    READING.store(true, Ordering::Release);
    let (status, retry, body) = post_cut(app.clone(), input.clone()).await;
    READING.store(false, Ordering::Release);
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(retry.as_deref(), Some("60"));
    assert!(body.contains("cut_in_progress"), "{body}");

    // The cut.
    let (status, _, body) = post_cut(app.clone(), input.clone()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let frames = events(&body);
    let (first, last) = (frames.first().unwrap(), frames.last().unwrap());
    assert_eq!(first.0, "quipu.asof.begin", "{body}");
    assert_eq!(last.0, "quipu.asof.commit", "{body}");
    let begin: Value = serde_json::from_str(&first.2).unwrap();
    let commit: Value = serde_json::from_str(&last.2).unwrap();
    assert!(first.1.is_none(), "only the barrier carries an id");
    assert_eq!(
        last.1.as_deref(),
        Some(begin["source_tx"].to_string().as_str())
    );
    assert_eq!(begin["manifest"]["source"]["store_id"], identity);
    assert_eq!(begin["source"], commit["source"]);
    assert_eq!(begin["manifest_sha256"], commit["manifest_sha256"]);
    let parts = &frames[1..frames.len() - 1];
    assert_eq!(parts.len() as u64, begin["parts"].as_u64().unwrap());
    let mut hasher = Sha256::new();
    let mut records = 0;
    for (event, id, data) in parts {
        assert_eq!(event, "quipu.asof.part");
        assert!(id.is_none());
        let part: Value = serde_json::from_str(data).unwrap();
        assert_eq!(
            part["digest_before"],
            hex::encode(hasher.clone().finalize())
        );
        hasher.update(data.as_bytes());
        records += part["records"].as_array().unwrap().len();
    }
    let digest = hex::encode(hasher.finalize());
    assert_eq!(begin["digest"], digest);
    assert_eq!(commit["digest"], digest);
    assert_eq!(records, 2);
    assert_eq!(begin["manifest"]["completeness"]["graphs"][G]["records"], 2);
    // The spool is gone once the stream has been sent.
    let spool = spool_dir().join(format!(
        "{}.parts",
        begin["manifest_sha256"]
            .as_str()
            .unwrap()
            .trim_start_matches("sha256:")
    ));
    assert!(!spool.exists(), "{}", spool.display());

    // A refusal is an ordinary error response, never a started stream.
    let wrong = json!({"graphs": [G], "expected_store_id": "urn:uuid:another-store"});
    let (status, _, body) = post_cut(app.clone(), wrong).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(!body.contains("quipu.asof"), "{body}");
    // The lock was released after each phase.
    assert!(!READING.load(Ordering::Acquire));
}

#[test]
fn the_route_is_write_class_until_read_authorization_exists() {
    assert!(quipu::http_auth::is_write_endpoint("/share/asof"));
    assert!(!quipu::http_auth::READ_ENDPOINTS.contains(&"/share/asof"));
}
