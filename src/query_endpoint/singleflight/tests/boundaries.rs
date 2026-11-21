use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn simultaneous_unstaggered_arrivals_and_completions_coalesce() {
    let (_dir, store, flights) = fixture(true);
    let (held_tx, held_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let locked = store.clone();
    let blocker = std::thread::spawn(move || {
        let _guard = locked.lock();
        held_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    });
    held_rx.await.unwrap();
    let start = Arc::new(tokio::sync::Barrier::new(8));
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let store = store.clone();
        let start = start.clone();
        tasks.push(tokio::spawn(async move {
            start.wait().await;
            call(store, "ASK {}").await
        }));
    }
    wait_followers(&flights, 7).await;
    release_tx.send(()).unwrap();
    blocker.join().unwrap();
    let mut bodies = Vec::new();
    for task in tasks {
        bodies.push(task.await.unwrap());
    }
    assert!(bodies.iter().all(|body| body == &bodies[0]));
    assert_eq!(store.query_execution_count.load(Ordering::Relaxed), 1);
    assert_eq!(flights.stats.lock().coalesced, 7);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oversized_answer_preserves_bytes_but_is_not_shared() {
    let (_dir, store, flights) = fixture(true);
    let value = "x".repeat(MAX_RESULT_BYTES as usize);
    quipu::tool_knot(
        &mut store.lock(),
        &json!({"turtle":format!("<urn:s> <urn:p> \"{value}\" .")}),
    )
    .unwrap();
    let (held_tx, held_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let locked = store.clone();
    let blocker = std::thread::spawn(move || {
        let _guard = locked.lock();
        held_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    });
    held_rx.await.unwrap();
    let run = |shared| async move {
        let response = crate::query_endpoint::query(
            axum::extract::State(shared),
            HeaderMap::new(),
            axum::Json(json!({"query":"SELECT ?v WHERE { <urn:s> <urn:p> ?v }"})),
        )
        .await
        .unwrap();
        axum::body::to_bytes(response.into_body(), 2 * MAX_RESULT_BYTES as usize)
            .await
            .unwrap()
    };
    let one = tokio::spawn(run(store.clone()));
    tokio::time::timeout(Duration::from_secs(3), async {
        while flights.active.lock().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let two = tokio::spawn(run(store.clone()));
    wait_followers(&flights, 1).await;
    release_tx.send(()).unwrap();
    blocker.join().unwrap();
    let one = one.await.unwrap();
    let two = two.await.unwrap();
    assert_eq!(one, two);
    assert!(one.len() > MAX_RESULT_BYTES as usize);
    assert_eq!(flights.stats.lock().coalesced, 0);
    assert_eq!(store.query_execution_count.load(Ordering::Relaxed), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pinned_old_followers_share_during_writes_but_new_watermark_never_joins_them() {
    let (dir, store, flights) = fixture(true);
    let (pinned_tx, pinned_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    *flights.pin_hook.lock() = Some(Box::new(move || {
        pinned_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    }));
    let text = "ASK { <urn:new> <urn:p> <urn:o> }";
    let one = tokio::spawn(call(store.clone(), text));
    pinned_rx.await.unwrap();
    let two = tokio::spawn(call(store.clone(), text));
    wait_followers(&flights, 1).await;
    let mut writer = quipu::Store::open(dir.path().join("flight.db").to_str().unwrap()).unwrap();
    quipu::tool_knot(
        &mut writer,
        &json!({"turtle":"<urn:new> <urn:p> <urn:o> ."}),
    )
    .unwrap();
    let three = tokio::spawn(call(store.clone(), text));
    tokio::time::timeout(Duration::from_secs(3), async {
        while flights.active.lock().len() != 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    release_tx.send(()).unwrap();
    let old = one.await.unwrap();
    assert_eq!(old, two.await.unwrap());
    assert_ne!(old, three.await.unwrap());
    assert_eq!(flights.stats.lock().coalesced, 1);
    assert_eq!(store.query_execution_count.load(Ordering::Relaxed), 2);
    assert!(flights.active.lock().is_empty());
}

#[test]
fn snapshot_nesting_errors_and_unwinds_preserve_transaction_ownership() {
    let (dir, store, _flights) = fixture(true);
    let guard = store.lock();
    let initial = guard.latest_tx_id().unwrap();
    let mut peer = quipu::Store::open(dir.path().join("flight.db").to_str().unwrap()).unwrap();
    guard
        .query_snapshot(|outer| {
            assert_eq!(outer, initial);
            quipu::tool_knot(&mut peer, &json!({"turtle":"<urn:new> <urn:p> <urn:o> ."})).unwrap();
            guard
                .query_snapshot(|inner| assert_eq!(inner, outer))
                .unwrap();
            // The nested helper must not end the caller's pinned transaction.
            assert_eq!(guard.latest_tx_id().unwrap(), initial);
        })
        .unwrap();
    assert!(guard.latest_tx_id().unwrap() > initial);
    let result: Result<(), &str> = guard
        .query_snapshot(|_| Err("synthetic read failure"))
        .unwrap();
    assert!(result.is_err());
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = guard.query_snapshot(|_| panic!("synthetic read unwind"));
    }));
    assert!(panic.is_err());
    // A fresh read sees the current head, proving no transaction escaped either path.
    assert_eq!(
        guard.query_snapshot(|head| head).unwrap(),
        guard.latest_tx_id().unwrap()
    );
}

#[tokio::test]
async fn verified_principals_and_credentials_separate_identical_wire_inputs() {
    let input = json!({"query":"ASK {}"});
    let headers = HeaderMap::new();
    let identity = |principal: &str, credential: &str| quipu::transaction_auth::Identity {
        principal: principal.into(),
        credential_id: Some(credential.into()),
        auth_class: "synthetic-test".into(),
        attestation: Default::default(),
    };
    let one = crate::auth::run_identified(Some(identity("urn:test:p", "key-a")), async {
        fingerprint(&input, &headers, true)
    })
    .await;
    let other_principal =
        crate::auth::run_identified(Some(identity("urn:test:q", "key-a")), async {
            fingerprint(&input, &headers, true)
        })
        .await;
    let other_credential =
        crate::auth::run_identified(Some(identity("urn:test:p", "key-b")), async {
            fingerprint(&input, &headers, true)
        })
        .await;
    assert_ne!(one, other_principal);
    assert_ne!(one, other_credential);
    assert_ne!(one, fingerprint(&input, &headers, true));
}

#[test]
fn known_names_are_shareable_but_commented_non_deterministic_functions_are_not() {
    let headers = HeaderMap::new();
    for query in [
        "SELECT ?known WHERE { ?known <urn:known> ?o }",
        "SELECT ?service WHERE { ?service <urn:service> ?o }",
        "SELECT ?s WHERE { ?s <urn:p> \"UNKNOWN\" }",
    ] {
        assert!(
            fingerprint(&json!({"query":query}), &headers, true).is_some(),
            "ordinary name was mistaken for NOW"
        );
    }
    for query in [
        "SELECT (NOW # comment\n() AS ?n) WHERE {}",
        "SELECT (RAND # comment\n() AS ?r) WHERE {}",
        "SELECT (STRUUID() AS ?u) WHERE {}",
    ] {
        assert!(fingerprint(&json!({"query":query}), &headers, true).is_none());
    }
}
