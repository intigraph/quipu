use super::*;

const CONTEXT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn identity() -> crate::transaction_auth::Identity {
    crate::transaction_auth::Identity {
        principal: "urn:test:receipt-owner".into(),
        credential_id: Some("receipt-key".into()),
        auth_class: "named_bearer".into(),
        attestation: Default::default(),
    }
}
fn input() -> Value {
    json!({"turtle":"<urn:owner:s> <urn:owner:p> \"value\" ."})
}
fn binding(n: u64) -> Binding {
    Binding::for_request(format!("{n:064x}"), &input()).unwrap()
}

#[test]
fn owner_context_is_atomic_with_the_attempt_and_never_rekeys_its_id() {
    crate::transaction_auth::with_identity(Some(identity()), || {
        let mut store = Store::open_in_memory().unwrap();
        let bound = binding(1);
        let ticket = store
            .admit_owner_knot_attempt(bound.clone(), CONTEXT)
            .unwrap();
        let stored: String = store
            .conn
            .query_row("SELECT owner_context FROM knot_attempts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stored, CONTEXT);
        assert_eq!(
            store.knot_attempt_status(&bound).unwrap()["state"],
            "unknown"
        );
        assert_eq!(
            store.owner_knot_attempt_status(&bound, OTHER).unwrap()["state"],
            "unknown"
        );
        assert!(store.seal_knot_binding(&bound).is_err());
        let result =
            crate::mcp::knot::tool_knot_with_completion(&mut store, &input(), &ticket).unwrap();
        assert_eq!(
            store.owner_knot_attempt_status(&bound, CONTEXT).unwrap()["state"],
            "committed"
        );
        store.finish_knot_attempt(&ticket, Some(&result)).unwrap();
        assert_eq!(
            store.owner_knot_attempt_status(&bound, CONTEXT).unwrap()["state"],
            "finished"
        );
        assert!(
            store
                .admit_owner_knot_attempt(bound.clone(), OTHER)
                .is_err(),
            "context drift cannot make an old ID fresh"
        );
        assert!(
            store.admit_knot_attempt(bound).is_err(),
            "legacy cannot reuse the ID"
        );
        assert_eq!(
            store
                .conn
                .query_row("SELECT count(*) FROM knot_attempts", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    });
}

#[test]
fn old_ledger_reads_are_read_only_and_admission_upgrades_without_losing_legacy_rows() {
    crate::transaction_auth::with_identity(Some(identity()), || {
        let mut store = Store::open_in_memory().unwrap();
        store.conn.execute_batch("CREATE TABLE knot_attempts (write_id TEXT PRIMARY KEY, owner TEXT NOT NULL, body TEXT NOT NULL, store_id TEXT NOT NULL, epoch TEXT NOT NULL, nonce TEXT NOT NULL, phase TEXT NOT NULL, tx INTEGER, result TEXT)").unwrap();
        let old = binding(2);
        store.conn.execute("INSERT INTO knot_attempts(write_id,owner,body,store_id,epoch,nonce,phase) VALUES(?1,?2,?3,?4,?5,'retained','admitted')",
            params![old.write_id,old.owner,old.body_sha256,store.store_id().unwrap(),store.log_epoch().unwrap()]).unwrap();
        let before = store.conn.total_changes();
        assert_eq!(
            store.knot_attempt_status(&old).unwrap()["state"],
            "admitted"
        );
        assert_eq!(
            store.owner_knot_attempt_status(&old, CONTEXT).unwrap()["state"],
            "unknown"
        );
        assert_eq!(store.conn.total_changes(), before);
        assert!(
            !store.has_knot_owner_context().unwrap(),
            "status must not migrate"
        );
        let new = binding(3);
        store
            .admit_owner_knot_attempt(new.clone(), CONTEXT)
            .unwrap();
        assert!(store.has_knot_owner_context().unwrap());
        assert_eq!(
            store.knot_attempt_status(&old).unwrap()["state"],
            "admitted"
        );
        assert_eq!(
            store.owner_knot_attempt_status(&new, CONTEXT).unwrap()["state"],
            "admitted"
        );
        assert_eq!(store.knot_attempt_status(&new).unwrap()["state"], "unknown");
    });
}

#[test]
fn admission_fault_rolls_back_the_bound_attempt_and_keeps_the_connection_usable() {
    crate::transaction_auth::with_identity(Some(identity()), || {
        let mut store = Store::open_in_memory().unwrap();
        store.admit_knot_attempt(binding(4)).unwrap();
        store.conn.execute_batch("CREATE TRIGGER reject_owner_admit BEFORE INSERT ON knot_attempts WHEN NEW.owner_context IS NOT NULL BEGIN SELECT RAISE(ABORT,'fixture fault'); END").unwrap();
        let failed = binding(5);
        assert!(
            store
                .admit_owner_knot_attempt(failed.clone(), CONTEXT)
                .is_err()
        );
        assert!(store.conn.is_autocommit());
        assert_eq!(
            store
                .conn
                .query_row("SELECT count(*) FROM knot_attempts", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            store.owner_knot_attempt_status(&failed, CONTEXT).unwrap()["state"],
            "unknown"
        );
        store.admit_knot_attempt(binding(6)).unwrap();
    });
}

#[test]
fn scoped_seal_retains_the_consumed_id_and_blocks_a_late_effect() {
    crate::transaction_auth::with_identity(Some(identity()), || {
        let mut store = Store::open_in_memory().unwrap();
        let bound = binding(7);
        let ticket = store
            .admit_owner_knot_attempt(bound.clone(), CONTEXT)
            .unwrap();
        assert_eq!(
            store.seal_owner_knot_binding(&bound, CONTEXT).unwrap()["state"],
            "sealed"
        );
        assert!(
            crate::mcp::knot::tool_knot_with_completion(&mut store, &input(), &ticket).is_err()
        );
        assert!(store.finish_knot_attempt(&ticket, None).is_err());
        assert!(store.admit_owner_knot_attempt(bound, CONTEXT).is_err());
    });
}
