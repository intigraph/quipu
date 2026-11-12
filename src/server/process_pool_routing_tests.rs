use super::*;
use std::sync::{Arc, atomic::AtomicUsize};
use std::time::Duration;
use tokio::sync::Semaphore;

fn routing(workers: usize) -> Routing {
    Routing {
        configured: workers,
        workers: (0..workers)
            .map(|index| Worker {
                url: "http://127.0.0.1:9".into(),
                pid: 42 + index as u32,
                identity: "fixture".into(),
                available: Arc::new(Semaphore::new(1)),
                heavy_available: Arc::new(Semaphore::new(usize::from(index == 0))),
            })
            .collect(),
        key: "fixture-private-channel".into(),
        next: AtomicUsize::new(0),
        timeout: Duration::from_secs(3),
        response_limit: 65536,
        heavy_permits: 1,
    }
}

#[test]
fn heavy_lane_is_global_and_does_not_take_ordinary_capacity() {
    let routing = routing(3);
    let (worker, heavy) = routing.select(true).unwrap();
    assert_eq!(worker.pid, 42);
    assert!(routing.select(true).is_none());
    let ordinary: Vec<_> = (0..3).map(|_| routing.select(false).unwrap()).collect();
    assert_eq!(
        ordinary
            .iter()
            .map(|(worker, _)| worker.pid)
            .collect::<Vec<_>>(),
        [42, 43, 44]
    );
    assert!(routing.select(false).is_none());
    drop(heavy);
    assert!(routing.select(true).is_some());
}

#[test]
fn only_declared_query_owner_rows_use_the_heavy_lane() {
    let mut routing = routing(1);
    assert!(routing.heavy("/query", Some("owner-rows")));
    for operation in [None, Some("unknown"), Some("query")] {
        assert!(!routing.heavy("/query", operation));
    }
    assert!(!routing.heavy("/search", Some("owner-rows")));
    routing.heavy_permits = 0;
    assert!(!routing.heavy("/query", Some("owner-rows")));
}

#[tokio::test]
async fn proxy_does_not_turn_duplicate_operation_headers_into_a_valid_declaration() {
    use axum::{Router, body::Body, http::Request};
    use std::io::{Read, Write};
    use tower::ServiceExt;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let backend = std::thread::spawn(move || {
        let until = std::time::Instant::now() + Duration::from_secs(3);
        let mut socket = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < until,
                        "proxy never reached backend"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("accept: {error}"),
            }
        };
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut headers = Vec::new();
        while !headers.ends_with(b"\r\n\r\n") {
            assert!(headers.len() < 65536);
            let mut byte = [0];
            socket.read_exact(&mut byte).unwrap();
            headers.push(byte[0]);
        }
        let mut body = [0; 2];
        socket.read_exact(&mut body).unwrap();
        socket.write_all(b"HTTP/1.1 200 OK\r\nx-quipu-read-worker-pid: 42\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").unwrap();
        String::from_utf8(headers).unwrap().to_ascii_lowercase()
    });
    let mut routing = routing(1);
    routing.workers[0].url = format!("http://{address}");
    let pool = crate::process_pool::ProcessPool {
        children: Vec::new(),
        routing: Some(Arc::new(routing)),
        internal_worker: false,
    };
    let app: Router = crate::process_pool::route(Router::new(), &pool);
    let mut request = Request::post("/query").body(Body::from("{}")).unwrap();
    request
        .headers_mut()
        .append("x-quipu-operation", "owner-rows".parse().unwrap());
    request
        .headers_mut()
        .append("x-quipu-operation", "owner-rows".parse().unwrap());
    let response = tokio::time::timeout(Duration::from_secs(4), app.oneshot(request))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(!backend.join().unwrap().contains("x-quipu-operation:"));
}
