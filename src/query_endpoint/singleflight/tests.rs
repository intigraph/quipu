use super::*;
use std::sync::atomic::Ordering;

fn fixture(enabled: bool) -> (tempfile::TempDir, SharedStore, Arc<Flights>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flight.db");
    let store = quipu::Store::open(path.to_str().unwrap()).unwrap();
    let flights = Flights::open(path.to_str().unwrap()).unwrap();
    let mut handle = crate::StoreHandle::writer_only(store);
    handle.query_flights = enabled.then(|| flights.clone());
    (dir, Arc::new(handle), flights)
}
async fn call(store: SharedStore, text: &str) -> Bytes {
    let result = crate::query_endpoint::query(
        axum::extract::State(store),
        HeaderMap::new(),
        axum::Json(json!({"query":text})),
    )
    .await
    .unwrap();
    axum::body::to_bytes(result.into_body(), 1024 * 1024)
        .await
        .unwrap()
}
async fn wait_followers(flights: &Flights, count: usize) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if flights
                .active
                .lock()
                .values()
                .any(|f| f.done.receiver_count() == count)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("followers did not join");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn identical_burst_executes_once_and_is_not_a_completed_result_cache() {
    for enabled in [false, true] {
        let (_dir, store, flights) = fixture(enabled);
        let (held_tx, held_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let locked = store.clone();
        let blocker = std::thread::spawn(move || {
            let _guard = locked.lock();
            held_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        held_rx.await.unwrap();
        let mut tasks = Vec::new();
        // Stagger marker probes while the actual evaluator is deterministically
        // blocked: no dependence on a naturally slow query or a live store.
        for n in 0..8 {
            let shared = store.clone();
            tasks.push(tokio::spawn(async move { call(shared, "ASK {}").await }));
            if enabled {
                if n == 0 {
                    tokio::time::timeout(Duration::from_secs(3), async {
                        while flights.active.lock().is_empty() {
                            tokio::task::yield_now().await;
                        }
                    })
                    .await
                    .unwrap();
                } else {
                    wait_followers(&flights, n).await;
                }
            }
        }
        release_tx.send(()).unwrap();
        blocker.join().unwrap();
        let mut bodies = Vec::new();
        for task in tasks {
            bodies.push(task.await.unwrap());
        }
        assert!(bodies.iter().all(|b| b == &bodies[0]));
        assert_eq!(
            store.query_execution_count.load(Ordering::Relaxed),
            if enabled { 1 } else { 8 }
        );
        assert_eq!(flights.stats.lock().coalesced, if enabled { 7 } else { 0 });
        assert!(flights.active.lock().is_empty());
        call(store.clone(), "ASK {}").await;
        assert_eq!(
            store.query_execution_count.load(Ordering::Relaxed),
            if enabled { 2 } else { 9 }
        );
    }
}

#[test]
fn text_scope_format_authority_and_non_determinism_are_fenced() {
    let headers = HeaderMap::new();
    let input = json!({"query":"ASK {}", "graph":"urn:a"});
    let base = fingerprint(&input, &headers, true).unwrap();
    for other in [
        json!({"query":"ASK { ?s ?p ?o }", "graph":"urn:a"}),
        json!({"query":"ASK {}", "graph":"urn:b"}),
        json!({"query":"ASK {}", "graph":"urn:a", "tx":7}),
        json!({"query":"ASK {}", "graph":"urn:a", "verbose":true}),
    ] {
        assert_ne!(base, fingerprint(&other, &headers, true).unwrap());
    }
    for name in [
        "accept",
        "authorization",
        "x-quipu-require-substring",
        "x-quipu-operation",
    ] {
        let mut changed = HeaderMap::new();
        changed.insert(
            axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
            "different".parse().unwrap(),
        );
        assert_ne!(base, fingerprint(&input, &changed, true).unwrap());
    }
    assert_eq!(
        base,
        fingerprint(
            &json!({"query":" \nASK {} \t", "graph":"urn:a"}),
            &headers,
            true
        )
        .unwrap()
    );
    assert_ne!(
        fingerprint(&json!({"query":"ASK { ?s ?p \"a  b\" }"}), &headers, true),
        fingerprint(&json!({"query":"ASK { ?s ?p \"a b\" }"}), &headers, true)
    );
    for text in [
        "SELECT (NOW() AS ?n) WHERE {}",
        "SELECT (RAND() AS ?r) WHERE {}",
        "ASK { SERVICE <urn:remote> {} }",
        "SELECT (UUID() AS ?u) WHERE {}",
    ] {
        assert!(fingerprint(&json!({"query":text}), &headers, true).is_none());
    }
    assert!(fingerprint(&json!({"query":"ASK {}", "federated":true}), &headers, true).is_none());
    assert!(
        fingerprint(
            &json!({"query":"ASK {}", "execution_profile":true}),
            &headers,
            true
        )
        .is_none()
    );
}

#[test]
fn external_writes_change_generation_and_unknown_probe_bypasses() {
    let (_dir, store, flights) = fixture(true);
    let before = flights.generation().unwrap();
    quipu::tool_knot(
        &mut store.lock(),
        &json!({"turtle":"<urn:s> <urn:p> <urn:o> ."}),
    )
    .unwrap();
    let after = flights.generation().unwrap();
    assert_ne!(before, after);
    let input = json!({"query":"ASK {}"});
    let hash = fingerprint(&input, &HeaderMap::new(), true).unwrap();
    let _one = flights.join((hash, before)).unwrap();
    assert!(matches!(flights.join((hash, after)), Some(Join::Leader(_))));
    let _probe = flights.marker.lock();
    assert_eq!(flights.generation(), None);
}

#[tokio::test]
async fn bounded_followers_and_cancelled_leader_wake_without_an_answer() {
    let (_dir, _store, flights) = fixture(true);
    let key = ([0; 32], flights.generation().unwrap());
    let Some(Join::Leader(leader)) = flights.join(key) else {
        panic!()
    };
    let mut followers = Vec::new();
    for _ in 0..MAX_FOLLOWERS {
        let Some(Join::Follower(rx)) = flights.join(key) else {
            panic!()
        };
        followers.push(rx);
    }
    assert!(flights.join(key).is_none());
    followers.pop();
    assert!(matches!(flights.join(key), Some(Join::Follower(_))));
    drop(leader);
    followers[0].changed().await.unwrap();
    assert!(followers[0].borrow().is_none());
    assert!(flights.active.lock().is_empty());
    let mut leaders = Vec::new();
    for n in 0..MAX_FLIGHTS {
        leaders.push(flights.join(([n as u8; 32], key.1)).unwrap());
    }
    assert!(flights.join(([255; 32], key.1)).is_none());
    drop(leaders);
    assert!(flights.active.lock().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_during_leader_run_forces_followers_to_execute_again() {
    let (dir, store, flights) = fixture(true);
    let (held_tx, held_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let locked = store.clone();
    let blocker = std::thread::spawn(move || {
        let _guard = locked.lock();
        held_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    });
    held_rx.await.unwrap();
    let one = tokio::spawn(call(store.clone(), "ASK { <urn:s> <urn:p> <urn:o> }"));
    tokio::time::timeout(Duration::from_secs(3), async {
        while flights.active.lock().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let two = tokio::spawn(call(store.clone(), "ASK { <urn:s> <urn:p> <urn:o> }"));
    wait_followers(&flights, 1).await;
    let mut writer = quipu::Store::open(dir.path().join("flight.db").to_str().unwrap()).unwrap();
    quipu::tool_knot(&mut writer, &json!({"turtle":"<urn:s> <urn:p> <urn:o> ."})).unwrap();
    release_tx.send(()).unwrap();
    blocker.join().unwrap();
    assert_eq!(one.await.unwrap(), two.await.unwrap());
    assert_eq!(flights.stats.lock().coalesced, 0);
    assert_eq!(store.query_execution_count.load(Ordering::Relaxed), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn follower_times_out_without_cancelling_the_leader_or_executing_again() {
    let (_dir, shared, flights) = fixture(true);
    let mut handle = Arc::try_unwrap(shared).ok().unwrap();
    handle.search_config.request_timeout_ms = 30;
    let store = Arc::new(handle);
    let (held_tx, held_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let locked = store.clone();
    let blocker = std::thread::spawn(move || {
        let _guard = locked.lock();
        held_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    });
    held_rx.await.unwrap();
    let one = tokio::spawn(call(store.clone(), "ASK {}"));
    tokio::time::timeout(Duration::from_secs(3), async {
        while flights.active.lock().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let result = crate::query_endpoint::query(
        axum::extract::State(store.clone()),
        HeaderMap::new(),
        axum::Json(json!({"query":"ASK {}"})),
    )
    .await;
    assert!(result.is_err_and(|error| error.is_query_timeout()));
    assert_eq!(flights.active.lock().len(), 1);
    release_tx.send(()).unwrap();
    blocker.join().unwrap();
    one.await.unwrap();
    assert_eq!(store.query_execution_count.load(Ordering::Relaxed), 1);
    assert!(flights.active.lock().is_empty());
}

#[test]
fn default_is_off_and_memory_store_is_not_given_a_false_generation() {
    assert!(!quipu::config::ServerConfig::default().query_singleflight);
    assert!(Flights::open(":memory:").is_none());
}

/// Deliberate local A/B, not a timing assertion in the ordinary unit suite.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "bounded source-only burst A/B; run with --ignored --nocapture"]
async fn identical_read_burst_ab() {
    let mut evidence = Vec::new();
    for enabled in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("burst.db");
        let mut writer = quipu::Store::open(path.to_str().unwrap()).unwrap();
        let turtle: String = (0..400)
            .map(|n| format!("<urn:s{n}> <urn:p> <urn:o{n}> .\n"))
            .collect();
        quipu::tool_knot(&mut writer, &json!({"turtle":turtle})).unwrap();
        let conns = (0..2)
            .map(|_| {
                let mut reader = quipu::Store::open_read_only(path.to_str().unwrap()).unwrap();
                reader.adopt_read_config_from(&writer);
                parking_lot::FairMutex::new(reader)
            })
            .collect();
        let pool = crate::ReadPool {
            conns,
            next: std::sync::atomic::AtomicUsize::new(0),
            point_reserve: false,
        };
        let flights = Flights::open(path.to_str().unwrap()).unwrap();
        let mut handle = crate::StoreHandle::serving(
            writer,
            pool,
            path.to_str().unwrap(),
            quipu::config::FederationConfig::default(),
        );
        handle.query_flights = enabled.then(|| flights.clone());
        let store = Arc::new(handle);
        let query = "SELECT (COUNT(*) AS ?count) WHERE { ?a <urn:p> ?b . ?c <urn:p> ?d }";
        let (held_tx, held_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let locked = store.clone();
        let blocker = std::thread::spawn(move || {
            let _guards: Vec<_> = locked
                .readers
                .conns
                .iter()
                .map(|conn| conn.lock())
                .collect();
            held_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        held_rx.await.unwrap();
        let mut tasks = Vec::new();
        for n in 0..8 {
            let shared = store.clone();
            tasks.push(tokio::spawn(async move {
                let started = Instant::now();
                let body = call(shared, query).await;
                (started.elapsed().as_secs_f64(), body)
            }));
            if enabled {
                if n == 0 {
                    tokio::time::timeout(Duration::from_secs(3), async {
                        while flights.active.lock().is_empty() {
                            tokio::task::yield_now().await;
                        }
                    })
                    .await
                    .unwrap();
                } else {
                    wait_followers(&flights, n).await;
                }
            }
        }
        if !enabled {
            tokio::time::timeout(Duration::from_secs(3), async {
                while store.query_execution_count.load(Ordering::Relaxed) < 8 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
        }
        release_tx.send(()).unwrap();
        blocker.join().unwrap();
        let mut rows = Vec::new();
        for task in tasks {
            rows.push(task.await.unwrap());
        }
        assert!(rows.iter().all(|row| row.1 == rows[0].1));
        let executed = store.query_execution_count.load(Ordering::Relaxed);
        assert_eq!(executed, if enabled { 1 } else { 8 });
        rows.sort_by(|a, b| a.0.total_cmp(&b.0));
        let p95 = rows[7].0;
        eprintln!(
            "SINGLEFLIGHT_AB enabled={enabled} requests=8 executed={executed} coalesced={} p95_seconds={p95:.6}",
            flights.stats.lock().coalesced
        );
        evidence.push((p95, rows[0].1.clone()));
    }
    assert_eq!(evidence[0].1, evidence[1].1);
    assert!(
        evidence[1].0 < evidence[0].0,
        "local A/B did not improve p95"
    );
}

#[test]
fn independent_store_handles_do_not_share_a_registry() {
    let (_a, _sa, one) = fixture(true);
    let (_b, _sb, two) = fixture(true);
    let key = ([0; 32], (1, 0));
    let _leader = one.join(key).unwrap();
    assert!(matches!(two.join(key), Some(Join::Leader(_))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn aborting_an_http_leader_wakes_its_follower_and_does_not_poison_next_read() {
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
    let one = tokio::spawn(call(store.clone(), "ASK {}"));
    tokio::time::timeout(Duration::from_secs(3), async {
        while flights.active.lock().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let two = tokio::spawn(call(store.clone(), "ASK {}"));
    wait_followers(&flights, 1).await;
    one.abort();
    assert!(one.await.unwrap_err().is_cancelled());
    assert!(flights.active.lock().is_empty());
    release_tx.send(()).unwrap();
    blocker.join().unwrap();
    two.await.unwrap();
    assert_eq!(flights.stats.lock().coalesced, 0);
    call(store, "ASK {}").await;
    assert!(flights.active.lock().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn query_error_is_not_shared_or_cached_as_a_successful_empty_answer() {
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
    let run = |shared| async move {
        crate::query_endpoint::query(
            axum::extract::State(shared),
            HeaderMap::new(),
            axum::Json(json!({"query":"invalid SPARQL"})),
        )
        .await
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
    assert!(one.await.unwrap().is_err());
    assert!(two.await.unwrap().is_err());
    assert_eq!(flights.stats.lock().coalesced, 0);
    assert_eq!(store.query_execution_count.load(Ordering::Relaxed), 2);
    assert!(flights.active.lock().is_empty());
}

mod boundaries;
