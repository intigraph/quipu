//! Progress proofs come from complete snapshots, not keepalive delivery.
use super::super::{Cursor, Params};
use super::tests::{BOARD, OTHER, settled, shared};
use super::*;
use std::sync::atomic::Ordering;

fn cursor(shared: &SharedStore, since: i64) -> Cursor {
    let params: Params = serde_json::from_value(json!({
        "since": since, "graph": BOARD, "predicate": "urn:p:title", "progress": true
    }))
    .unwrap();
    Cursor::open(shared.clone(), since, params, true, None)
}

async fn frame(cursor: &mut Cursor) -> super::super::FeedFrame {
    tokio::time::timeout(Duration::from_secs(3), cursor.next_frame())
        .await
        .expect("no progress or data frame")
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quiet_progress_is_opt_in_and_reuses_one_shared_observation() {
    let shared = shared();
    let hub = Hub::of(&shared);
    settled(&shared, &hub).await;
    let head = shared.read().latest_tx_id().unwrap();
    let mut a = cursor(&shared, head);
    let first = frame(&mut a).await;
    assert_eq!(first.event, "quipu.changes.progress");
    let body: Value = serde_json::from_str(&first.data).unwrap();
    assert_eq!(body["since"], head);
    assert_eq!(body["next_tx"], head);
    assert_eq!(
        body["scope"],
        json!({"graphs":[BOARD], "predicate":"urn:p:title"})
    );
    assert_eq!(body["complete"], true);
    assert!(body["observed_at"].as_f64().unwrap() > 0.0);
    // No subscriber reads while serving a fresh ring observation.
    let count = hub.observations.load(Ordering::Relaxed);
    for _ in 0..10 {
        let mut b = cursor(&shared, head);
        assert_eq!(frame(&mut b).await.event, "quipu.changes.progress");
    }
    assert!(
        hub.observations.load(Ordering::Relaxed) <= count + 1,
        "one shared first-subscriber wake, never one read per subscriber"
    );
    // A repeated cached checkpoint cannot refresh this connection.
    assert!(
        tokio::time::timeout(Duration::from_millis(50), a.next_frame())
            .await
            .is_err()
    );
    let params = serde_json::from_value(json!({"since":head,"graph":BOARD})).unwrap();
    let mut ordinary = Cursor::open(shared.clone(), head, params, true, None);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), ordinary.next_frame())
            .await
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn filtered_ranges_advance_only_after_matching_transaction_delivery() {
    let shared = shared();
    let hub = Hub::of(&shared);
    settled(&shared, &hub).await;
    let head = shared.read().latest_tx_id().unwrap();
    let mut c = cursor(&shared, head);
    frame(&mut c).await;
    super::tests::write(&shared, OTHER, "urn:unrelated");
    settled(&shared, &hub).await;
    let proof: Value = serde_json::from_str(&frame(&mut c).await.data).unwrap();
    assert!(proof["next_tx"].as_i64().unwrap() > head);
    super::tests::write(&shared, BOARD, "urn:matching");
    settled(&shared, &hub).await;
    let matching = frame(&mut c).await;
    assert_eq!(matching.event, "quipu.changes");
    assert!(matching.data.contains("urn:matching"));
    let proof = frame(&mut c).await;
    assert_eq!(proof.event, "quipu.changes.progress");
    assert_eq!(proof.id, matching.id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn incomplete_ring_and_changed_identity_never_attest_progress() {
    let shared = shared();
    let hub = Hub::of(&shared);
    settled(&shared, &hub).await;
    let head = shared.read().latest_tx_id().unwrap();
    let source = hub.ring.lock().unwrap().source.clone();
    assert!(hub.progress(head - 1, &source).is_none());
    assert!(
        hub.progress(head, &json!({"store_id":"wrong","log_epoch":"wrong"}))
            .is_none()
    );
    {
        let mut ring = hub.ring.lock().unwrap();
        ring.observed_at = None;
    }
    assert!(hub.progress(head, &source).is_none());
    {
        let mut ring = hub.ring.lock().unwrap();
        ring.observed_at = Some(1.0);
        ring.ready = false;
    }
    assert!(hub.progress(head, &source).is_none());
}

#[test]
fn a_sqlite_snapshot_excludes_a_racing_writer_until_the_next_observation() {
    use quipu::{
        Store,
        store::Datum,
        types::{Op, Value as FactValue},
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("progress.db");
    let mut writer = Store::open(path.to_str().unwrap()).unwrap();
    let graph = writer.graph_create(BOARD).unwrap();
    let reader = Store::open_read_only(path.to_str().unwrap()).unwrap();
    let before = reader.latest_tx_id().unwrap();
    reader
        .with_change_snapshot(|read| {
            assert_eq!(read.latest_tx_id()?, before); // establish the WAL snapshot
            let d = Datum {
                entity: writer.intern("urn:racing")?,
                attribute: writer.intern("urn:p:title")?,
                value: FactValue::Str("race".into()),
                valid_from: super::tests::T.into(),
                valid_to: None,
                op: Op::Assert,
            };
            writer.transact_to_graph(&[d], super::tests::T, None, None, graph)?;
            let page = read.changes_after(
                before,
                1,
                quipu::store::changes::Capture::OldAndNewValues,
                None,
            )?;
            assert!(page.records.is_empty());
            assert_eq!(page.watermark_tx, before);
            Ok(())
        })
        .unwrap();
    let page = reader
        .with_change_snapshot(|read| {
            read.changes_after(
                before,
                1,
                quipu::store::changes::Capture::OldAndNewValues,
                None,
            )
        })
        .unwrap();
    assert_eq!(page.records.len(), 1);
    assert!(page.watermark_tx > before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn catchup_and_multipart_never_skip_matching_data_for_progress() {
    let shared = shared();
    let hub = Hub::of(&shared);
    settled(&shared, &hub).await;
    let mut c = cursor(&shared, 0); // predates the initial ring floor
    assert_eq!(frame(&mut c).await.event, "quipu.changes");
    assert_eq!(frame(&mut c).await.event, "quipu.changes.progress");
    let since = c.offset();
    {
        let mut store = shared.lock();
        let graph = store.registered_graph_id(BOARD).unwrap().unwrap();
        let d = quipu::store::Datum {
            entity: store.intern("urn:large").unwrap(),
            attribute: store.intern("urn:p:title").unwrap(),
            value: quipu::types::Value::Str("x".repeat(200_000)),
            valid_from: super::tests::T.into(),
            valid_to: None,
            op: quipu::types::Op::Assert,
        };
        store
            .transact_to_graph(&[d], super::tests::T, None, None, graph)
            .unwrap();
    }
    settled(&shared, &hub).await;
    let f = frame(&mut c).await;
    assert_ne!(f.event, "quipu.changes.progress");
    if f.event == "quipu.error" {
        assert_eq!(c.offset(), since);
    }
    // Pending multipart frames always drain before any progress frame.
    while !c.pending.is_empty() {
        assert_ne!(frame(&mut c).await.event, "quipu.changes.progress");
    }
}

#[test]
fn unsupported_progress_scopes_are_refused() {
    use super::super::scope_ok;
    for value in [
        json!({"progress":true}),
        json!({"progress":true,"graph":BOARD}),
        json!({"progress":true,"graph":BOARD,"predicate":"urn:p:a,urn:p:b"}),
        json!({"progress":true,"graph":BOARD,"predicate":"urn:p:a","subject":"urn:s"}),
    ] {
        let p: Params = serde_json::from_value(value).unwrap();
        assert!(!scope_ok(&p, true));
    }
    let p: Params =
        serde_json::from_value(json!({"progress":true,"graph":BOARD,"predicate":"urn:p:a"}))
            .unwrap();
    assert!(scope_ok(&p, true));
    assert!(!scope_ok(&p, false));
}
