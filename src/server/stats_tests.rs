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

/// aegis-67p0lj: the streaming COUNT aggregate equals the previous algorithm
/// (select every `?s ?p ?o` row, count rows and distinct Ref subjects and
/// predicates) on a store with repeated subjects, a shared predicate, literal
/// and IRI objects, and an rdf:type.
#[tokio::test]
async fn stats_count_aggregate_equals_the_materialising_count() {
    let _serial = STATS_TEST_LOCK.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let shared = pooled(&dir);
    {
        let mut writer = shared.lock();
        quipu::tool_knot(
            &mut writer,
            &json!({ "turtle": "@prefix ex: <http://example.org/> .\n\
                ex:a a ex:Thing ; ex:name \"A\" ; ex:link ex:b .\n\
                ex:b a ex:Thing ; ex:name \"B\" .\n\
                ex:c ex:link ex:a , ex:b .\n" }),
        )
        .expect("fixture knot");
    }
    // Oracle: the previous materialising algorithm, kept here verbatim.
    let (facts, entities, predicates) = {
        let reader = shared.read();
        let rows = quipu::sparql_query(&reader, "SELECT ?s ?p ?o WHERE { ?s ?p ?o }").unwrap();
        let mut e = std::collections::HashSet::new();
        let mut p = std::collections::HashSet::new();
        for row in rows.rows() {
            if let Some(quipu::Value::Ref(id)) = row.get("s") {
                e.insert(*id);
            }
            if let Some(quipu::Value::Ref(id)) = row.get("p") {
                p.insert(*id);
            }
        }
        (rows.rows().len() as u64, e.len() as u64, p.len() as u64)
    };
    assert!(
        facts >= 7,
        "precondition: the fixture landed ({facts} rows)"
    );
    *STATS_CACHE.lock().unwrap() = None;
    STATS_RECOMPUTING.store(false, Ordering::SeqCst);
    let got = stats(State(shared.clone()))
        .await
        .expect("stats handler should succeed")
        .0;
    assert_eq!(got["facts"].as_u64(), Some(facts), "facts");
    assert_eq!(got["entities"].as_u64(), Some(entities), "entities");
    assert_eq!(got["predicates"].as_u64(), Some(predicates), "predicates");
}
