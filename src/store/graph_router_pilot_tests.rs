use super::*;
use std::{sync::Mutex, time::Instant};

fn enabled() -> IngestPilot {
    IngestPilot {
        enabled: true,
        graph: Some(GRAPH.into()),
    }
}

#[test]
fn default_off_never_prepares_or_changes_the_legacy_store() {
    let f = fixture();
    let before = f.legacy.conn.total_changes();
    let catalog = catalog_for(&f.route);
    let admitted = IngestPilot::default()
        .admit(catalog, &f.legacy, || {
            panic!("disabled pilot prepared an owner")
        })
        .unwrap();
    assert!(admitted.is_none());
    assert_eq!(f.legacy.conn.total_changes(), before);
    assert!(f.legacy.routed_graph_fence_rows().unwrap().is_empty());
}

#[test]
fn enabled_pilot_checks_selection_and_fence_before_preparation() {
    let f = fixture();
    let catalog = catalog_for(&f.route);
    let wrong = IngestPilot {
        enabled: true,
        graph: Some("urn:wrong".into()),
    };
    assert!(
        wrong
            .admit(catalog.clone(), &f.legacy, || panic!("wrong graph"))
            .is_err()
    );
    assert!(
        enabled()
            .admit(catalog.clone(), &f.legacy, || panic!("unfenced"))
            .is_err()
    );
    f.legacy.fence_routed_graph(GRAPH, 7, 4).unwrap();
    let router = enabled()
        .admit(catalog.clone(), &f.legacy, || Ok(owner(&f, &catalog)))
        .unwrap()
        .unwrap();
    assert!(router.owner(GRAPH).is_some());
}

#[test]
fn multiple_routes_are_refused_before_preparation() {
    let f = fixture();
    let mut second = f.route.clone();
    second.graph = "urn:second".into();
    second.file_id = "second".into();
    second.store_id = "second-store".into();
    let catalog = Arc::new(
        Catalog::new(
            7,
            "legacy-store",
            vec![f.route.clone(), second],
            Budget {
                max_owners: 2,
                queued_per_owner: 1,
                readers_per_owner: 1,
                max_queued: 2,
                max_connections: 6,
            },
        )
        .unwrap(),
    );
    assert!(
        enabled()
            .admit(catalog, &f.legacy, || panic!("multiple graphs"))
            .is_err()
    );
}

#[test]
fn legacy_selection_and_invalid_pins_never_write_an_owner() {
    let f = fixture();
    let catalog = catalog_for(&f.route);
    f.legacy.fence_routed_graph(GRAPH, 7, 4).unwrap();
    let router = enabled()
        .admit(catalog.clone(), &f.legacy, || Ok(owner(&f, &catalog)))
        .unwrap()
        .unwrap();
    let before = owner_facts(&router);
    assert!(
        router
            .routed_knot(&request(None), &input(None, "legacy"))
            .unwrap()
            .is_none()
    );
    let mut stale = request(Some(GRAPH));
    stale.owner_generation = Some(3);
    assert!(
        router
            .routed_knot(&stale, &input(Some(GRAPH), "stale"))
            .is_err()
    );
    stale = request(Some(GRAPH));
    stale.signed = true;
    assert!(
        router
            .routed_knot(&stale, &input(Some(GRAPH), "signed"))
            .is_err()
    );
    stale.signed = false;
    stale.cross_graph_context = true;
    assert!(
        router
            .routed_knot(&stale, &input(Some(GRAPH), "context"))
            .is_err()
    );
    assert!(
        router
            .routed_knot(&request(None), &input(Some(GRAPH), "mismatch"))
            .is_err()
    );
    assert_eq!(owner_facts(&router), before);
}

// Each call replaces one regenerable producer snapshot, then checks the exact
// value through the owner's dedicated read pool (not the legacy connection).
fn cycle(router: &Router, n: usize) -> u128 {
    let started = Instant::now();
    let value = format!("value-{n}");
    let envelope = json!({
        "graph": GRAPH, "source": "pilot-test", "snapshot": "pilot-producer",
        "replace_snapshot": true,
        "turtle": format!("<urn:pilot:s> <urn:pilot:p> {value:?} ."),
    });
    assert!(
        router
            .routed_knot(&request(Some(GRAPH)), &envelope)
            .unwrap()
            .is_some()
    );
    let read = router
        .owner(GRAPH)
        .unwrap()
        .read(|store| {
            crate::tool_query(
                store,
                &json!({
                    "graph": GRAPH, "query": "SELECT ?v WHERE { <urn:pilot:s> <urn:pilot:p> ?v }",
                }),
            )
        })
        .unwrap();
    assert_eq!(
        read["count"], 1,
        "snapshot replacement or scoped read changed"
    );
    assert!(
        read.to_string().contains(&value),
        "point read did not return current value"
    );
    started.elapsed().as_micros()
}

fn summary(mut samples: Vec<u128>) -> (u128, u128) {
    samples.sort_unstable();
    (
        samples[samples.len() / 2],
        samples[(samples.len() * 95).div_ceil(100) - 1],
    )
}

#[test]
fn routed_snapshot_cycles_complete_while_legacy_mutex_and_sqlite_writer_are_held() {
    let f = fixture();
    let catalog = catalog_for(&f.route);
    f.legacy.fence_routed_graph(GRAPH, 7, 4).unwrap();
    let router = enabled()
        .admit(catalog.clone(), &f.legacy, || Ok(owner(&f, &catalog)))
        .unwrap()
        .unwrap();
    let idle: Vec<_> = (0..20).map(|n| cycle(&router, n)).collect();
    let legacy = Mutex::new(f.legacy);
    let held = legacy.lock().unwrap();
    held.conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    let changes = held.conn.total_changes();
    let data_version: i64 = held
        .conn
        .query_row("PRAGMA data_version", [], |r| r.get(0))
        .unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let completed = std::thread::scope(|scope| {
        scope.spawn(|| {
            let samples: Vec<_> = (20..40).map(|n| cycle(&router, n)).collect();
            tx.send(samples).unwrap();
        });
        let result = rx.recv_timeout(std::time::Duration::from_secs(5));
        // Observe completion before releasing either legacy lock.
        let changes_after = held.conn.total_changes();
        let version_after: i64 = held
            .conn
            .query_row("PRAGMA data_version", [], |r| r.get(0))
            .unwrap();
        held.conn.execute_batch("ROLLBACK").unwrap();
        drop(held);
        assert_eq!(changes, changes_after, "routed writes mutated legacy");
        assert_eq!(
            data_version, version_after,
            "another connection mutated legacy"
        );
        result
    })
    .expect("routed work waited on the legacy writer");
    println!(
        "pilot synthetic write+read us: idle_raw={idle:?} held_raw={completed:?} idle_p50_p95={:?} held_p50_p95={:?}; legacy_changes_delta=0 legacy_data_version_delta=0",
        summary(idle.clone()),
        summary(completed.clone())
    );
    cycle(&router, 39); // Replay the last snapshot; still exactly one current value.
}
