//! Private HTTP transport controls: no production endpoint or credential.
use super::*;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

const TOKEN: &str = "relay-upstream-private-fixture";

struct Source {
    url: String,
    calls: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Source {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn protected_source() -> Source {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let policy = quipu::http_auth::BearerPolicy::new(Some(TOKEN.into()), None, None, 1).unwrap();
    let app = axum::Router::new()
        .route(
            "/replicate/stream",
            axum::routing::post(|| async {
                (
                    [("content-type", "text/event-stream")],
                    "event: fixture\ndata: {}\n\n",
                )
            }),
        )
        .route(
            "/replicate/challenge",
            axum::routing::post(|| async { StatusCode::ACCEPTED }),
        )
        .layer(axum::middleware::from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                let policy = policy.clone();
                let seen = seen.clone();
                async move {
                    seen.fetch_add(1, Ordering::SeqCst);
                    let decision = quipu::http_auth::authorize_http_bearers(
                        request.uri().path(),
                        request.method().as_str(),
                        false,
                        &policy,
                        request
                            .headers()
                            .get("authorization")
                            .and_then(|v| v.to_str().ok()),
                        2,
                    );
                    if decision.decision != quipu::http_auth::AccessDecision::Allow {
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                    next.run(request).await
                }
            },
        ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Source { url, calls, task }
}

fn credential(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

fn configured(url: &str, file: Option<&Path>) -> Arc<Relay> {
    use ring::signature::Ed25519KeyPair;
    let mut value = serde_json::json!({
        "upstream_url": url, "upstream_store_id": "fixture-store",
        "upstream_key": "ab".repeat(32), "graphs": ["urn:graph:fixture"],
    });
    if let Some(file) = file {
        value["upstream_credential_file"] = serde_json::json!(file);
    }
    let config = serde_json::from_value(value)
        .expect("relay credential-file configuration must be supported");
    let mut store = quipu::Store::open_in_memory().unwrap();
    let doc = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
    store.set_signing_identity(Arc::new(quipu::signing::SigningIdentity::new(
        Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap(),
        "quipu",
    )));
    super::open(&store, &config).unwrap()
}

fn request() -> StreamRequest {
    StreamRequest {
        graphs: vec!["urn:graph:fixture".into()],
        context_graphs: vec![],
        since: 0,
        expected_store_id: "fixture-store".into(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn credential_file_authenticates_both_upstream_operations_without_fallback() {
    let source = protected_source().await;
    // Real policy refuses the old transport; this is not an empty-source result.
    let anonymous = configured(&source.url, None);
    assert!(anonymous.upstream.open(request()).await.is_none());
    assert!(
        !anonymous
            .upstream
            .challenge("fixture".into(), [1; 16])
            .await
    );

    // Paired positive: the same actual source accepts its fixture bearer.
    let url = source.url.clone();
    tokio::task::spawn_blocking(move || {
        for (path, expected) in [("stream", 200), ("challenge", 202)] {
            let response = ureq::post(&format!("{url}/replicate/{path}"))
                .set("Authorization", &format!("Bearer {TOKEN}"))
                .send_string("{}")
                .unwrap();
            assert_eq!(response.status(), expected);
        }
    })
    .await
    .unwrap();

    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("credential");
    credential(&file, &format!("{TOKEN}\n"));
    let relay = configured(&source.url, Some(&file));
    let mut stream = relay
        .upstream
        .open(request())
        .await
        .expect("authenticated stream");
    assert!(stream.next().await.unwrap().contains("event: fixture"));
    assert!(relay.upstream.challenge("fixture".into(), [2; 16]).await);

    // Rotation is reloaded for both operations; rejection never retries anonymously.
    credential(&file, "wrong-private-fixture");
    let before = source.calls.load(Ordering::SeqCst);
    assert!(relay.upstream.open(request()).await.is_none());
    assert!(!relay.upstream.challenge("fixture".into(), [4; 16]).await);
    assert_eq!(source.calls.load(Ordering::SeqCst), before + 2);
    credential(&file, TOKEN);
    assert!(relay.upstream.open(request()).await.is_some());
    assert!(relay.upstream.challenge("fixture".into(), [5; 16]).await);

    // An unreadable/removed configured file is not an anonymous or cached-token retry.
    let before = source.calls.load(Ordering::SeqCst);
    std::fs::remove_file(&file).unwrap();
    assert!(relay.upstream.open(request()).await.is_none());
    assert!(!relay.upstream.challenge("fixture".into(), [3; 16]).await);
    assert_eq!(source.calls.load(Ordering::SeqCst), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn redirects_and_non_stream_success_are_refused() {
    let sink_calls = Arc::new(AtomicUsize::new(0));
    let seen = sink_calls.clone();
    let sink = axum::Router::new().fallback(move || {
        let seen = seen.clone();
        async move {
            seen.fetch_add(1, Ordering::SeqCst);
            StatusCode::ACCEPTED
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let sink_url = format!("http://{}", listener.local_addr().unwrap());
    let sink_task = tokio::spawn(async move { axum::serve(listener, sink).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("credential");
    credential(&file, TOKEN);
    for redirect in [true, false] {
        let location = sink_url.clone();
        let app = axum::Router::new().fallback(move || {
            let location = location.clone();
            async move {
                if redirect {
                    (
                        StatusCode::TEMPORARY_REDIRECT,
                        [("location", location)],
                        "reflected-private-fixture",
                    )
                        .into_response()
                } else {
                    (
                        StatusCode::OK,
                        [("content-type", "application/json".to_string())],
                        "{}",
                    )
                        .into_response()
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let relay = configured(&url, Some(&file));
        assert!(relay.upstream.open(request()).await.is_none());
        assert!(!relay.upstream.challenge("fixture".into(), [6; 16]).await);
        task.abort();
    }
    assert_eq!(sink_calls.load(Ordering::SeqCst), 0);
    sink_task.abort();
}

#[test]
fn inline_upstream_token_configuration_is_refused() {
    let result = serde_json::from_value::<quipu::config::RelayConfig>(serde_json::json!({
        "upstream_url": "http://fixture.invalid", "upstream_store_id": "fixture",
        "upstream_key": "ab".repeat(32), "graphs": [], "upstream_token": "private-fixture"
    }));
    assert!(result.is_err());
}
