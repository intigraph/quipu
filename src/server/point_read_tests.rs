use super::*;

#[test]
fn point_reservation_needs_two_existing_readers_and_is_opt_in() {
    assert!(!quipu::config::ServerConfig::default().reserve_point_reads);
    for count in 0..=4 {
        let (_dir, mut handle) = pooled_handle(count);
        assert!(!handle.readers.point_reserve);
        assert_eq!(handle.readers.ordinary_len(), count);
        assert_eq!(handle.readers.reserve_point_reads(true), count >= 2);
        assert_eq!(handle.readers.len(), count, "no extra connections");
        assert_eq!(
            handle.readers.ordinary_len(),
            count - usize::from(count >= 2)
        );
        handle.readers.reserve_point_reads(false);
        assert_eq!(handle.readers.ordinary_len(), count);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn point_query_serves_true_and_false_while_ordinary_readers_are_held() {
    for reserve in [false, true] {
        let (_dir, mut h) = pooled_handle(2);
        h.readers.reserve_point_reads(reserve);
        quipu::tool_knot(
            &mut h.lock(),
            &json!({"turtle":
            "<https://example.test/s> <https://example.test/p> <https://example.test/o> ."}),
        )
        .unwrap();
        let h = Arc::new(h);
        let held = h.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let _guards: Vec<_> = held.readers.conns[usize::from(reserve)..]
                .iter()
                .map(parking_lot::FairMutex::lock)
                .collect();
            ready_tx.send(()).unwrap();
            let _ = release_rx.recv();
        });
        ready_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        let ordinary_store = h.clone();
        let ordinary = tokio::spawn(async move {
            query(State(ordinary_store), Default::default(), axum::Json(json!({
                "query":"SELECT ?s WHERE { ?s <https://example.test/p> <https://example.test/o> }"
            }))).await
        });
        let point_store = h.clone();
        let mut point = tokio::spawn(async move {
            let mut truths = Vec::new();
            for subject in ["s", "absent"] {
                let response = query(State(point_store.clone()), Default::default(), axum::Json(json!({
                    "query":format!("ASK {{ <https://example.test/{subject}> <https://example.test/p> <https://example.test/o> }}")
                }))).await.unwrap();
                assert_eq!(response.status(), axum::http::StatusCode::OK);
                let bytes = axum::body::to_bytes(response.into_body(), 4096)
                    .await
                    .unwrap();
                truths.push(
                    serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["result"].clone(),
                );
            }
            truths
        });
        let early = tokio::time::timeout(std::time::Duration::from_millis(200), &mut point).await;
        assert!(
            !ordinary.is_finished(),
            "ordinary work must not steal the reserved connection"
        );
        release_tx.send(()).unwrap();
        holder.join().unwrap();
        if reserve {
            assert_eq!(early.unwrap().unwrap(), vec![json!(true), json!(false)]);
        } else {
            assert!(
                early.is_err(),
                "control: unreserved readers are all occupied"
            );
            assert_eq!(point.await.unwrap(), vec![json!(true), json!(false)]);
        }
        let ordinary = ordinary.await.unwrap().unwrap();
        assert_eq!(ordinary.status(), axum::http::StatusCode::OK);
    }
}
