use super::*;

fn identity() -> crate::transaction_auth::Identity {
    crate::transaction_auth::Identity {
        principal: "urn:test:owner".into(),
        credential_id: Some("credential".into()),
        auth_class: "named_bearer".into(),
        attestation: Default::default(),
    }
}
fn binding(n: u64) -> Binding {
    Binding::for_request(format!("{n:064x}"), &input()).unwrap()
}
fn input() -> Value {
    json!({"turtle":"<urn:test:knot> <http://www.w3.org/2000/01/rdf-schema#label> \"kept\" ."})
}
fn write(store: &mut Store, ticket: &Ticket) -> Value {
    crate::mcp::knot::tool_knot_with_completion(store, &input(), ticket).unwrap()
}

#[test]
fn durable_completion_survives_reopen_without_reexecution() {
    crate::transaction_auth::with_identity(Some(identity()), || {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.db");
        let mut store = Store::open(path.to_str().unwrap()).unwrap();
        let b = binding(1);
        let t = store.admit_knot_attempt(b.clone()).unwrap();
        let result = write(&mut store, &t);
        assert_eq!(store.knot_attempt_status(&b).unwrap()["state"], "committed");
        store.finish_knot_attempt(&t, Some(&result)).unwrap();
        drop(store);
        let mut reopened = Store::open(path.to_str().unwrap()).unwrap();
        let status = reopened.knot_attempt_status(&b).unwrap();
        assert_eq!(status["state"], "finished");
        assert_eq!(status["receipt"]["tx_id"], result["tx_id"]);
        assert!(reopened.admit_knot_attempt(b).is_err());
    });
}

#[test]
fn lost_response_retains_atomic_commit_but_not_worker_completion() {
    crate::transaction_auth::with_identity(Some(identity()), || {
        let mut s = Store::open_in_memory().unwrap();
        let b = binding(2);
        let t = s.admit_knot_attempt(b.clone()).unwrap();
        let result = write(&mut s, &t);
        let status = s.knot_attempt_status(&b).unwrap();
        assert_eq!(status["state"], "committed");
        assert_eq!(status["commit"]["tx_id"], result["tx_id"]);
        assert!(status.get("receipt").is_none());
        assert!(s.seal_knot_attempt(&t).is_err());
        assert!(s.admit_knot_attempt(b).is_err());
    });
}

#[test]
fn sealed_late_worker_cannot_commit_or_restore_admission() {
    crate::transaction_auth::with_identity(Some(identity()), || {
        let mut s = Store::open_in_memory().unwrap();
        let b = binding(3);
        let t = s.admit_knot_attempt(b.clone()).unwrap();
        s.seal_knot_attempt(&t).unwrap();
        let before: i64 = s
            .conn
            .query_row("SELECT COUNT(*) FROM facts", [], |r| r.get(0))
            .unwrap();
        assert!(crate::mcp::knot::tool_knot_with_completion(&mut s, &input(), &t).is_err());
        let after: i64 = s
            .conn
            .query_row("SELECT COUNT(*) FROM facts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(before, after);
        assert_eq!(s.knot_attempt_status(&b).unwrap()["state"], "sealed");
        assert!(
            s.finish_knot_attempt(&t, Some(&json!({"tx_id":1})))
                .is_err()
        );
        assert!(s.admit_knot_attempt(b).is_err());
    });
}

#[test]
fn binding_drift_and_foreign_store_are_unknown_or_refused() {
    crate::transaction_auth::with_identity(Some(identity()), || {
        let mut s = Store::open_in_memory().unwrap();
        let b = binding(4);
        let t = s.admit_knot_attempt(b.clone()).unwrap();
        let mut foreign = b.clone();
        foreign.owner = "foreign".into();
        assert!(s.knot_attempt_status(&foreign).is_err());
        foreign = b.clone();
        foreign.body_sha256 = "b".repeat(64);
        assert_eq!(s.knot_attempt_status(&foreign).unwrap()["state"], "unknown");
        let mut other = Store::open_in_memory().unwrap();
        assert_eq!(other.knot_attempt_status(&b).unwrap()["state"], "unknown");
        assert!(crate::mcp::knot::tool_knot_with_completion(&mut other, &input(), &t).is_err());
        s.mint_log_epoch("test changed lineage").unwrap();
        assert_eq!(s.knot_attempt_status(&b).unwrap()["state"], "unknown");
        assert!(crate::mcp::knot::tool_knot_with_completion(&mut s, &input(), &t).is_err());
    });
}

#[test]
fn missing_or_failed_worker_never_manufactures_commit() {
    crate::transaction_auth::with_identity(Some(identity()), || {
        let mut s = Store::open_in_memory().unwrap();
        let b = binding(5);
        assert_eq!(s.knot_attempt_status(&b).unwrap()["state"], "unknown");
        let t = s.admit_knot_attempt(b.clone()).unwrap();
        assert!(
            s.finish_knot_attempt(&t, Some(&json!({"tx_id":10,"conforms":true})))
                .is_err()
        );
        s.finish_knot_attempt(&t, None).unwrap();
        let result = s.knot_attempt_status(&b).unwrap();
        assert_eq!(result["state"], "stopped");
        assert!(result.get("commit").is_none());
        assert!(s.admit_knot_attempt(b).is_err());
    });
}

#[test]
fn durable_ids_survive_more_than_process_cache_capacity() {
    crate::transaction_auth::with_identity(Some(identity()), || {
        let mut s = Store::open_in_memory().unwrap();
        let b = binding(6);
        let t = s.admit_knot_attempt(b.clone()).unwrap();
        for n in 100..620 {
            s.admit_knot_attempt(binding(n)).unwrap();
        }
        let result = write(&mut s, &t);
        s.finish_knot_attempt(&t, Some(&result)).unwrap();
        assert_eq!(s.knot_attempt_status(&b).unwrap()["state"], "finished");
        assert!(s.admit_knot_attempt(b).is_err());
    });
}

#[cfg(feature = "reactive-reasoner")]
#[test]
fn hook_timing_stays_after_real_commit_and_derived_write_has_no_ticket() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct Observer {
        binding: Binding,
        seen: Arc<AtomicUsize>,
    }
    impl super::super::TransactObserver for Observer {
        fn after_commit(&self, store: &mut Store, delta: &super::super::Delta) -> Result<()> {
            if delta.source.as_deref() == Some("test:derived") {
                return Ok(());
            }
            assert!(
                store.conn.is_autocommit(),
                "hook must not run inside an outer savepoint"
            );
            let proof = store.knot_attempt_status(&self.binding)?;
            assert_eq!(proof["state"], "committed");
            assert_eq!(proof["commit"]["tx_id"], delta.tx);
            self.seen.fetch_add(1, Ordering::SeqCst);
            store.transact(&[], &crate::time::now_iso(), None, Some("test:derived"))?;
            assert_eq!(
                store.knot_attempt_status(&self.binding)?["commit"]["tx_id"],
                delta.tx
            );
            Ok(())
        }
    }
    crate::transaction_auth::with_identity(Some(identity()), || {
        let mut s = Store::open_in_memory().unwrap();
        let b = binding(7);
        let t = s.admit_knot_attempt(b.clone()).unwrap();
        let seen = Arc::new(AtomicUsize::new(0));
        s.add_observer(Arc::new(Observer {
            binding: b.clone(),
            seen: seen.clone(),
        }));
        let result = write(&mut s, &t);
        assert_eq!(seen.load(Ordering::SeqCst), 1);
        assert_eq!(
            s.knot_attempt_status(&b).unwrap()["commit"]["tx_id"],
            result["tx_id"]
        );
    });
}

#[test]
fn request_body_drift_refuses_before_any_effect() {
    crate::transaction_auth::with_identity(Some(identity()), || {
        let mut s = Store::open_in_memory().unwrap();
        let b = binding(8);
        let t = s.admit_knot_attempt(b.clone()).unwrap();
        let changed = json!({"turtle":"<urn:different> <urn:p> <urn:o> ."});
        assert!(crate::mcp::knot::tool_knot_with_completion(&mut s, &changed, &t).is_err());
        assert_eq!(s.knot_attempt_status(&b).unwrap()["state"], "admitted");
        let count: i64 = s
            .conn
            .query_row("SELECT COUNT(*) FROM facts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    });
}

#[test]
fn failed_commit_rolls_back_witness_together_with_effect() {
    crate::transaction_auth::with_identity(Some(identity()), || {
        let mut s = Store::open_in_memory().unwrap();
        let b = binding(9);
        let t = s.admit_knot_attempt(b.clone()).unwrap();
        s.conn.commit_hook(Some(|| true)).unwrap();
        assert!(crate::mcp::knot::tool_knot_with_completion(&mut s, &input(), &t).is_err());
        s.conn.commit_hook(None::<fn() -> bool>).unwrap();
        let status = s.knot_attempt_status(&b).unwrap();
        assert_eq!(status["state"], "admitted");
        assert!(status.get("commit").is_none());
        let n: i64 = s
            .conn
            .query_row("SELECT COUNT(*) FROM facts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
        assert!(s.conn.is_autocommit());
    });
}

#[test]
fn malformed_retained_result_is_unreadable_not_empty_or_replayable() {
    crate::transaction_auth::with_identity(Some(identity()), || {
        let mut s = Store::open_in_memory().unwrap();
        let b = binding(10);
        let t = s.admit_knot_attempt(b.clone()).unwrap();
        let result = write(&mut s, &t);
        s.finish_knot_attempt(&t, Some(&result)).unwrap();
        s.conn
            .execute(
                "UPDATE knot_attempts SET result='corrupt' WHERE write_id=?1",
                [&b.write_id],
            )
            .unwrap();
        assert!(s.knot_attempt_status(&b).is_err());
        assert!(s.admit_knot_attempt(b).is_err());
    });
}

#[test]
fn full_ledger_refuses_new_admission_without_pruning_old_ids() {
    crate::transaction_auth::with_identity(Some(identity()), || {
        let mut s = Store::open_in_memory().unwrap();
        let b = binding(11);
        s.admit_knot_attempt(b.clone()).unwrap();
        s.conn.execute_batch("WITH RECURSIVE n(x) AS (SELECT 1000 UNION ALL SELECT x+1 FROM n WHERE x<66534) \
            INSERT INTO knot_attempts(write_id,owner,body,store_id,epoch,nonce,phase) \
            SELECT printf('%064x',x),'fixture','fixture','fixture','fixture','fixture','admitted' FROM n").unwrap();
        assert!(s.admit_knot_attempt(binding(12)).is_err());
        assert_eq!(s.knot_attempt_status(&b).unwrap()["state"], "admitted");
        let count: i64 = s
            .conn
            .query_row("SELECT COUNT(*) FROM knot_attempts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, MAX_ATTEMPTS);
    });
}

#[cfg(feature = "reactive-reasoner")]
#[test]
fn hook_error_after_commit_preserves_atomic_witness_without_finished_claim() {
    struct Fails;
    impl super::super::TransactObserver for Fails {
        fn after_commit(&self, store: &mut Store, _: &super::super::Delta) -> Result<()> {
            assert!(store.conn.is_autocommit());
            Err(refused("fixture hook failed"))
        }
    }
    crate::transaction_auth::with_identity(Some(identity()), || {
        let mut s = Store::open_in_memory().unwrap();
        let b = binding(13);
        let t = s.admit_knot_attempt(b.clone()).unwrap();
        s.add_observer(std::sync::Arc::new(Fails));
        assert!(crate::mcp::knot::tool_knot_with_completion(&mut s, &input(), &t).is_err());
        assert_eq!(s.knot_attempt_status(&b).unwrap()["state"], "committed");
        s.finish_knot_attempt(&t, None).unwrap();
        let proof = s.knot_attempt_status(&b).unwrap();
        assert_eq!(proof["state"], "stopped");
        assert!(proof.get("commit").is_some());
        assert!(proof.get("receipt").is_none());
        assert!(s.admit_knot_attempt(b).is_err());
    });
}

#[test]
fn reopened_receiver_can_seal_and_exclude_a_stale_live_ticket() {
    crate::transaction_auth::with_identity(Some(identity()), || {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.db");
        let mut old = Store::open(path.to_str().unwrap()).unwrap();
        let binding = binding(14);
        let ticket = old.admit_knot_attempt(binding.clone()).unwrap();
        let mut reopened = Store::open(path.to_str().unwrap()).unwrap();
        assert_eq!(
            reopened.seal_knot_binding(&binding).unwrap()["state"],
            "sealed"
        );
        assert!(crate::mcp::knot::tool_knot_with_completion(&mut old, &input(), &ticket).is_err());
        assert_eq!(
            old.knot_attempt_status(&binding).unwrap()["state"],
            "sealed"
        );
        let n: i64 = old
            .conn
            .query_row("SELECT COUNT(*) FROM facts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
        assert!(reopened.admit_knot_attempt(binding).is_err());
    });
}
