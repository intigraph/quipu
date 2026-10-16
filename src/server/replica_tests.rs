use super::*;
use std::sync::Arc;

fn config(primary: &str) -> QuipuConfig {
    let mut config = QuipuConfig::default();
    config.server.replica = Some(ReplicaConfig {
        primary_url: primary.into(),
    });
    config
}

#[test]
fn replica_open_keeps_file_and_refuses_planted_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("follower.db");
    let path = path.to_str().unwrap();
    let mut primary = quipu::Store::open(path).unwrap();
    let seed = serde_json::json!({"turtle":"<urn:positive> <urn:predicate> \"present\" ."});
    quipu::tool_knot(&mut primary, &seed).unwrap();
    primary.checkpoint_wal_truncate().unwrap();
    drop(primary);
    let bytes = std::fs::read(path).unwrap();
    let modified = std::fs::metadata(path).unwrap().modified().unwrap();
    let mut reader = open_store(path, &config("http://primary.example")).unwrap();
    for _ in 0..100 {
        let result = quipu::tool_query(
            &reader,
            &serde_json::json!({"query":"ASK { <urn:positive> <urn:predicate> \"present\" }"}),
        )
        .unwrap();
        assert_eq!(result["result"], true);
    }
    let planted = serde_json::json!({"turtle":"<urn:planted> <urn:predicate> \"new\" ."});
    let mut control = quipu::Store::open_in_memory().unwrap();
    quipu::tool_knot(&mut control, &planted).unwrap();
    let error = quipu::tool_knot(&mut reader, &planted).unwrap_err();
    assert!(
        error.to_string().contains("readonly") || error.to_string().contains("read-only"),
        "wrong refusal: {error}"
    );
    drop(reader);
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    assert_eq!(
        std::fs::metadata(path).unwrap().modified().unwrap(),
        modified
    );
}

#[test]
fn replica_open_cannot_create_or_drop_configured_providers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing.db");
    assert!(open_store(path.to_str().unwrap(), &config("http://primary.example")).is_err());
    assert!(!path.exists());
    for url in [
        "not a url",
        "file:///tmp/primary",
        "http://user:secret@example.test",
        "http://example.test/?token=bad",
    ] {
        assert!(open_store(path.to_str().unwrap(), &config(url)).is_err());
        assert!(!path.exists());
    }
}

#[test]
fn lag_is_unknown_before_observation_and_after_staleness() {
    let status = ReplicaStatus::default();
    assert_eq!(status.lag(), None);
    assert!(
        status
            .metrics()
            .contains("quipu_replica_lag_transactions -1")
    );
    *status.observation.lock() = Some((Instant::now(), 3));
    assert_eq!(status.lag(), Some(3));
    *status.observation.lock() = Some((Instant::now() - Duration::from_secs(31), 0));
    assert_eq!(status.lag(), None, "stale zero is not fresh");
}

fn primary(body: String) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let join = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut request = [0u8; 4096];
        let n = stream.read(&mut request).unwrap();
        let request = String::from_utf8_lossy(&request[..n]).to_lowercase();
        assert!(request.starts_with("get /watermark http/1.1"));
        assert!(request.contains("x-quipu-client: replica-metadata"));
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });
    (url, join)
}

#[test]
fn observation_checks_lineage_and_primary_position_not_mtime() {
    let store = quipu::Store::open_in_memory().unwrap();
    let id = store.store_id().unwrap();
    let local = store.latest_tx_id().unwrap();
    let handle = Arc::new(super::super::StoreHandle::writer_only(store));
    for (store_id, watermark_tx, expected) in [
        (id.as_str(), local + 7, Some(7)),
        (id.as_str(), local, Some(0)),
        ("unrelated", local + 7, None),
        (id.as_str(), -1, None),
    ] {
        let (url, join) = primary(
            serde_json::json!({"store_id":store_id,"watermark_tx":watermark_tx}).to_string(),
        );
        assert_eq!(observe(&handle, &url), expected);
        join.join().unwrap();
    }
    let (url, join) = primary("not JSON".into());
    assert_eq!(observe(&handle, &url), None);
    join.join().unwrap();
}

#[tokio::test]
async fn write_refusal_is_503_and_names_primary_without_redirecting() {
    let config = config("https://primary.example").server.replica.unwrap();
    for path in quipu::http_auth::WRITE_ENDPOINTS {
        let response = write_refusal(&config, path);
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(!response.headers().contains_key("location"));
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["primary"], "https://primary.example");
        assert_eq!(value["endpoint"], *path);
    }
}
