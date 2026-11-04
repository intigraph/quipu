//! /stats runs off the writer and recomputes once at a time (aegis-fclp).
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::extract::State;
use serde_json::json;

use super::base::{STATS_CACHE, STATS_RECOMPUTING, STATS_TEST_LOCK, StatsCache, stats};
use super::{ReadPool, SharedStore, StoreHandle};

/// A file-backed store served with one pooled reader, so `read()` is not the writer.
fn pooled(dir: &tempfile::TempDir) -> SharedStore {
    let db = dir.path().join("stats.db");
    let path = db.to_str().unwrap();
    let mut store = quipu::Store::open(path).unwrap();
    store.embedding_config_mut().auto_embed = false;
    let readers = ReadPool::open(path, &store, 1);
    let shared = Arc::new(StoreHandle::serving(
        store,
        readers,
        path,
        Default::default(),
    ));
    assert!(shared.resolves_off_writer(), "precondition: pooled reader");
    shared
}

/// The writer is held for the whole call (as a long write would hold it), and the
/// cache is cold, so the handler must scan. It must still answer, with the exact
/// store id: before aegis-fclp it took the writer and blocked here.
#[tokio::test]
#[allow(clippy::await_holding_lock)] // Intentionally hold the writer, as a long write would, while /stats runs.
async fn stats_answers_while_the_writer_is_held() {
    let _serial = STATS_TEST_LOCK.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let shared = pooled(&dir);
    let expected_id = shared.lock().store_id().unwrap();
    *STATS_CACHE.lock().unwrap() = None;

    let writer = shared.lock();
    let answer = tokio::time::timeout(Duration::from_secs(20), stats(State(shared.clone())))
        .await
        .expect("/stats must not wait for the writer")
        .expect("stats handler should succeed")
        .0;
    drop(writer);

    assert_eq!(
        answer["store_id"],
        json!(expected_id),
        "the signing audience is unchanged"
    );
    assert_eq!(
        answer["facts"].as_u64(),
        Some(0),
        "the cold cache was recomputed"
    );
}

/// A write must not queue behind /stats. The test holds the pool's only reader, so
/// /stats waits on it exactly as it would mid-scan; a write through the writer must
/// still commit, and /stats must then answer with counts for that write.
#[tokio::test]
#[allow(clippy::await_holding_lock)] // Intentionally hold the only reader, as a /stats scan would, while a write commits.
async fn a_write_commits_while_stats_holds_the_reader() {
    let _serial = STATS_TEST_LOCK.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let shared = pooled(&dir);
    *STATS_CACHE.lock().unwrap() = None;

    let reader = shared.read();
    let pending = tokio::spawn(stats(State(shared.clone())));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !pending.is_finished(),
        "precondition: /stats waits on the reader"
    );

    let writer = shared.clone();
    let wrote = tokio::time::timeout(
        Duration::from_secs(20),
        tokio::task::spawn_blocking(move || {
            let mut store = writer.lock();
            let datum = quipu::Datum {
                entity: store.intern("urn:fclp:write").unwrap(),
                attribute: store
                    .intern("http://www.w3.org/2000/01/rdf-schema#label")
                    .unwrap(),
                value: quipu::Value::Str("written while /stats held a reader".into()),
                valid_from: "2026-01-01T00:00:00Z".into(),
                valid_to: None,
                op: quipu::Op::Assert,
            };
            store
                .transact(&[datum], "2026-01-01T00:00:00Z", None, None)
                .map(|_| store.latest_tx_id().unwrap())
        }),
    )
    .await
    .expect("the write must not wait for /stats")
    .expect("write task")
    .expect("write commits");
    assert!(
        !pending.is_finished(),
        "/stats is still waiting on the reader"
    );

    drop(reader);
    let answer = tokio::time::timeout(Duration::from_secs(20), pending)
        .await
        .expect("/stats answers once the reader is free")
        .expect("stats task")
        .expect("stats handler should succeed")
        .0;
    assert_eq!(
        answer["facts"].as_u64(),
        Some(1),
        "counts include the write"
    );
    assert_eq!(answer["counts_as_of_tx"], json!(wrote));
    assert_eq!(answer["counts_stale"], json!(false));
}

/// While another caller holds the recompute slot, a stale cache is returned as is
/// (no second scan); once the slot is free, the same stale cache is recomputed.
#[tokio::test]
async fn stats_does_not_start_a_second_scan_while_one_runs() {
    let _serial = STATS_TEST_LOCK.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let shared = pooled(&dir);
    let generation = shared.lock().latest_tx_id().unwrap();
    let poison = || StatsCache {
        generation: generation - 1,
        value: Arc::new(json!({ "facts": 42, "entities": 7, "predicates": 3 })),
        computed_at: None,
    };

    *STATS_CACHE.lock().unwrap() = Some(poison());
    STATS_RECOMPUTING.store(true, Ordering::SeqCst);
    let busy = stats(State(shared.clone())).await;
    STATS_RECOMPUTING.store(false, Ordering::SeqCst);
    let busy = busy.expect("stats handler should succeed").0;
    assert_eq!(
        busy["facts"].as_u64(),
        Some(42),
        "a running scan is not duplicated"
    );
    assert!(busy["store_id"].is_string(), "store_id is still read fresh");
    assert_eq!(
        busy["counts_stale"],
        json!(true),
        "older counts are labelled"
    );
    assert_eq!(busy["counts_as_of_tx"], json!(generation - 1));

    *STATS_CACHE.lock().unwrap() = Some(poison());
    let free = stats(State(shared.clone()))
        .await
        .expect("stats handler should succeed")
        .0;
    assert_eq!(
        free["facts"].as_u64(),
        Some(0),
        "a free slot recomputes the stale cache"
    );
    assert!(
        !STATS_RECOMPUTING.load(Ordering::SeqCst),
        "the recompute slot is released after the scan"
    );
}

/// aegis-67p0lj: a stale cache recomputed within `STATS_MIN_RECOMPUTE` is
/// answered as is and labelled stale (no scan), even with the slot free. Once
/// it is older than the interval, the next call recomputes. Every write moves the
/// generation, so without this limit a busy store rescans on almost every call.
#[tokio::test]
async fn stats_recompute_is_rate_limited_after_a_recent_scan() {
    let _serial = STATS_TEST_LOCK.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let shared = pooled(&dir);
    let generation = shared.lock().latest_tx_id().unwrap();
    let poison = |computed_at| StatsCache {
        generation: generation - 1,
        value: Arc::new(json!({ "facts": 42, "entities": 7, "predicates": 3 })),
        computed_at,
    };
    STATS_RECOMPUTING.store(false, Ordering::SeqCst);

    // Recent: the slot is free and the generation moved, yet no scan runs.
    *STATS_CACHE.lock().unwrap() = Some(poison(Some(std::time::Instant::now())));
    let recent = stats(State(shared.clone()))
        .await
        .expect("stats handler should succeed")
        .0;
    assert_eq!(
        recent["facts"].as_u64(),
        Some(42),
        "a recent aggregate answers without rescanning"
    );
    assert_eq!(recent["counts_stale"], json!(true), "and is labelled stale");
    assert_eq!(recent["counts_as_of_tx"], json!(generation - 1));
    assert!(
        recent["store_id"].is_string(),
        "store_id is still read fresh"
    );

    // Aged past the interval: the next call recomputes the true (empty) counts.
    let aged = std::time::Instant::now()
        .checked_sub(super::base::STATS_MIN_RECOMPUTE + Duration::from_secs(1))
        .expect("monotonic clock far enough from its origin");
    *STATS_CACHE.lock().unwrap() = Some(poison(Some(aged)));
    let fresh = stats(State(shared.clone()))
        .await
        .expect("stats handler should succeed")
        .0;
    assert_eq!(
        fresh["facts"].as_u64(),
        Some(0),
        "an aggregate older than the interval is recomputed"
    );
    assert_eq!(fresh["counts_stale"], json!(false));
}
