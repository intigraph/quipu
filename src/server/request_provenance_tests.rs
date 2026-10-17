//! Cancellation of the awaiting HTTP task must not detach metadata from commit.
use super::*;
use quipu::{Datum, Op, Store, Value, write_provenance::RequestProvenance};
use std::{
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn abandoned_await_preserves_commit_or_rollback_provenance() {
    for reject in [false, true] {
        let store = Arc::new(Mutex::new(Store::open_in_memory().unwrap()));
        let entered = Arc::new(tokio::sync::Notify::new());
        let (release_tx, release_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let p = Arc::new(
            RequestProvenance::classify(
                "cancel-control",
                "/knot",
                [
                    Some("agent"),
                    Some("codex"),
                    Some("host"),
                    Some("session"),
                    Some("model"),
                ],
            )
            .with_context(Some("task"), None),
        );
        let worker_store = store.clone();
        let worker_entered = entered.clone();
        let task = tokio::spawn(async move {
            REQUEST_WRITE_PROVENANCE
                .scope(Some(p), async move {
                    super::super::base::blocking(move || {
                        worker_entered.notify_one();
                        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                        let mut s = worker_store.lock().unwrap();
                        let datum = Datum {
                            entity: s.intern("urn:cancel:item")?,
                            attribute: s.intern("urn:cancel:label")?,
                            value: Value::Str("v".into()),
                            op: Op::Assert,
                            valid_from: "2026-01-01T00:00:00Z".into(),
                            valid_to: None,
                        };
                        let result = if reject {
                            // A transaction guard refusal exercised by a forbidden target.
                            let reserved = s.intern("urn:cancel:reserved#inferred")?;
                            s.transact_to_graph(
                                &[datum],
                                "2026-01-01T00:00:00Z",
                                None,
                                None,
                                reserved,
                            )
                        } else {
                            s.transact(&[datum], "2026-01-01T00:00:00Z", None, None)
                        };
                        done_tx.send(result).unwrap();
                        Ok(())
                    })
                    .await
                })
                .await
        });
        tokio::time::timeout(Duration::from_secs(3), entered.notified())
            .await
            .unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        release_tx.send(()).unwrap();
        let result = tokio::task::spawn_blocking(move || {
            done_rx.recv_timeout(Duration::from_secs(5)).unwrap()
        })
        .await
        .unwrap();
        let s = store.lock().unwrap();
        if reject {
            assert!(result.is_err());
            assert!(
                s.provenance_transactions(0, 10, Some("session"), None)
                    .unwrap()
                    .is_empty()
            );
        } else {
            let p = s.transaction_provenance(result.unwrap()).unwrap().unwrap();
            assert_eq!(p.declared.task.as_deref(), Some("task"));
            assert_eq!(p.declared.session.as_deref(), Some("session"));
        }
    }
}
