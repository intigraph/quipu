use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_http_listener_observes_socket_not_forwarded_headers() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn inspect(request: axum::extract::Request) -> String {
        let peer = request
            .extensions()
            .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
            .map(|info| info.0);
        crate::request_usage::with_request_context("{}".into(), peer, None)
    }

    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let server = tokio::spawn(async move {
        serve(
            Router::new().route("/peer", axum::routing::get(inspect)),
            &[],
            vec![],
            &address.to_string(),
            "unused",
        )
        .await;
    });
    let mut socket = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Ok(socket) = tokio::net::TcpStream::connect(address).await {
                break socket;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("private listener did not start");
    let expected = socket.local_addr().unwrap().to_string();
    socket
        .write_all(b"GET /peer HTTP/1.1\r\nHost: fixture\r\nX-Forwarded-For: claimed-peer\r\nX-Quipu-Host: claimed-host\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        socket.read_to_string(&mut response),
    )
    .await
    .unwrap()
    .unwrap();
    server.abort();
    let body = response.split_once("\r\n\r\n").unwrap().1;
    let record: Value = serde_json::from_str(body).unwrap();
    assert_eq!(record["transport_peer"], expected);
    assert_eq!(record["transport_peer_source"], "socket");
    assert!(!body.contains("claimed-peer"));
    assert!(!body.contains("claimed-host"));
}

#[test]
fn every_advertised_tool_has_a_classified_registered_route() {
    let source = include_str!("../server.rs");
    for tool in crate::tool_definitions() {
        let name = tool["name"].as_str().unwrap();
        let (_, path) = endpoint(name);
        assert!(
            crate::http_auth::WRITE_ENDPOINTS.contains(&path.as_str())
                || crate::http_auth::READ_ENDPOINTS.contains(&path.as_str()),
            "{name}: {path}"
        );
        // Alignment routes live in their own router module.
        assert!(
            source.contains(&format!("\"{path}\""))
                || include_str!("../server/align.rs").contains(&format!("\"{path}\""))
                || include_str!("../server/mapping.rs").contains(&format!("\"{path}\""))
                || include_str!("../server/merge_decisions.rs").contains(&format!("\"{path}\""))
                || include_str!("../server/snapshot_upload.rs").contains(&format!("\"{path}\""))
                || include_str!("../server/publication.rs").contains(&format!("\"{path}\""))
                || include_str!("../server/base.rs").contains(&format!("\"{path}\""))
                || include_str!("../query_endpoint/routes.rs").contains(&format!("\"{path}\"")),
            "{name}: {path}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn readiness_callback_runs_after_bind_and_does_not_block_http() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let bound = Arc::new(AtomicBool::new(false));
    let witnessed = bound.clone();
    let server = tokio::spawn(async move {
        serve_with_ready(
            Router::new().route("/ready", axum::routing::get(|| async { "ready" })),
            &[],
            vec![],
            &address.to_string(),
            "unused",
            move || {
                assert!(
                    std::net::TcpStream::connect(address).is_ok(),
                    "maintenance callback ran before bind"
                );
                witnessed.store(true, Ordering::SeqCst);
            },
        )
        .await;
    });
    let mut socket = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Ok(socket) = tokio::net::TcpStream::connect(address).await {
                break socket;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    socket
        .write_all(b"GET /ready HTTP/1.1\r\nHost: fixture\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        socket.read_to_string(&mut response),
    )
    .await
    .unwrap()
    .unwrap();
    server.abort();
    assert!(bound.load(Ordering::SeqCst));
    assert!(
        response.ends_with("ready"),
        "HTTP must serve after callback"
    );
}

#[tokio::test]
async fn invalid_stdio_credential_refuses_before_readiness_work() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("empty-token");
    std::fs::write(&path, "").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let mut called = false;
    let result = McpServer::new(Router::new())
        .stdio_with_ready(Some(&path), || called = true)
        .await;
    assert!(result.is_err());
    assert!(!called, "invalid credential must not start readiness work");
}
