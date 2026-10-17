use super::*;
use crate::{
    Datum, Op, Store, Value,
    write_provenance::{RequestProvenance, scoped},
};
use std::sync::Arc;

const NOW: &str = "2026-01-01T00:00:00Z";
fn declaration(agent: &str) -> Arc<RequestProvenance> {
    Arc::new(
        RequestProvenance::classify(
            "test-client",
            "/knot",
            [
                Some(agent),
                Some("codex"),
                Some("host"),
                Some("same-carrier"),
                Some("model"),
            ],
        )
        .with_context(Some("task-id"), Some("1.2.3")),
    )
}
fn datum(s: &Store, name: &str) -> Datum {
    Datum {
        entity: s.intern(name).unwrap(),
        attribute: s.intern("urn:test:p").unwrap(),
        value: Value::Str("value".into()),
        op: Op::Assert,
        valid_from: NOW.into(),
        valid_to: None,
    }
}
fn write(s: &mut Store, name: &str, p: Arc<RequestProvenance>) -> i64 {
    let d = datum(s, name);
    scoped(Some(p), || s.transact(&[d], NOW, None, None)).unwrap()
}

#[test]
fn committed_declarations_survive_reopen_and_remain_distinct_from_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    let path = path.to_str().unwrap();
    let tx = {
        let mut s = Store::open(path).unwrap();
        let identity = crate::transaction_auth::Identity {
            principal: "verified".into(),
            credential_id: None,
            auth_class: "signed".into(),
            attestation: Default::default(),
        };
        crate::transaction_auth::with_identity(Some(identity), || {
            write(&mut s, "urn:test:one", declaration("claimed"))
        })
    };
    let reader = Store::open_read_only(path).unwrap();
    let p = reader.transaction_provenance(tx).unwrap().unwrap();
    assert_eq!(p.declared.agent.as_deref(), Some("claimed"));
    assert_eq!(p.declared.task.as_deref(), Some("task-id"));
    assert_eq!(p.declared.client_version.as_deref(), Some("1.2.3"));
    assert_eq!(
        reader.transaction_auth(tx).unwrap().unwrap().principal,
        "verified"
    );
    assert_eq!(
        reader
            .provenance_transactions(0, 10, Some("same-carrier"), Some("claimed"))
            .unwrap(),
        vec![p]
    );
}

#[test]
fn rejection_and_audit_storage_failure_roll_back_both_facts_and_provenance() {
    let mut s = Store::open_in_memory().unwrap();
    let first = write(&mut s, "urn:test:one", declaration("claimed"));
    s.conn.execute_batch("CREATE TRIGGER refuse_fact BEFORE INSERT ON facts BEGIN SELECT RAISE(ABORT, 'refused'); END;").unwrap();
    let d = datum(&s, "urn:test:two");
    assert!(
        scoped(Some(declaration("claimed")), || s.transact(
            std::slice::from_ref(&d),
            NOW,
            None,
            None
        ))
        .is_err()
    );
    assert_eq!(
        s.provenance_transactions(0, 10, None, Some("claimed"))
            .unwrap()
            .len(),
        1
    );
    s.conn.execute_batch("DROP TRIGGER refuse_fact; CREATE TRIGGER refuse_audit BEFORE INSERT ON transaction_provenance BEGIN SELECT RAISE(ABORT, 'audit unavailable'); END;").unwrap();
    assert!(
        scoped(Some(declaration("claimed")), || s.transact(
            &[d],
            NOW,
            None,
            None
        ))
        .is_err()
    );
    let facts: i64 = s
        .conn
        .query_row("SELECT count(*) FROM facts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(facts, 1);
    assert_eq!(s.list_transactions().unwrap().last().unwrap().id, first);
}

#[test]
fn noops_unscoped_and_internal_writes_have_no_caller_row() {
    let mut s = Store::open_in_memory().unwrap();
    let p = declaration("claimed");
    let tx = write(&mut s, "urn:test:one", p.clone());
    let noop = write(&mut s, "urn:test:one", p.clone());
    assert_ne!(tx, noop);
    assert!(s.transaction_provenance(noop).unwrap().is_none());
    let d = datum(&s, "urn:test:two");
    let internal = scoped(Some(p), || {
        crate::write_kind::scoped(Some(crate::write_kind::WriteKind::Verdict), || {
            s.transact(&[d], NOW, None, None)
        })
    })
    .unwrap();
    assert!(s.transaction_provenance(internal).unwrap().is_none());
    let d = datum(&s, "urn:test:three");
    let plain = s.transact(&[d], NOW, None, None).unwrap();
    assert!(s.transaction_provenance(plain).unwrap().is_none());
}

#[test]
fn same_session_carrier_does_not_merge_agents_and_filter_precedes_limit() {
    let mut s = Store::open_in_memory().unwrap();
    write(&mut s, "urn:test:a", declaration("first"));
    let second = write(&mut s, "urn:test:b", declaration("second"));
    let rows = s
        .provenance_transactions(0, 1, Some("same-carrier"), Some("second"))
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].tx, second);
    assert!(
        s.provenance_transactions(second, 1, Some("same-carrier"), None)
            .unwrap()
            .is_empty()
    );
    assert!(s.provenance_transactions(0, 1, None, None).is_err());
}

#[test]
fn absent_and_partial_values_are_honest_and_old_readers_return_unknown() {
    let mut s = Store::open_in_memory().unwrap();
    let absent = write(
        &mut s,
        "urn:test:absent",
        Arc::new(RequestProvenance::classify("client", "/knot", [None; 5])),
    );
    let p = s.transaction_provenance(absent).unwrap().unwrap();
    assert_eq!(p.completeness, "absent");
    assert_eq!(p.declared, Declared::default());
    let partial = write(
        &mut s,
        "urn:test:partial",
        Arc::new(RequestProvenance::classify(
            "client",
            "/knot",
            [Some("agent"), Some("codex"), Some("host"), None, None],
        )),
    );
    assert_eq!(
        s.transaction_provenance(partial)
            .unwrap()
            .unwrap()
            .completeness,
        "partial"
    );
    s.conn
        .execute_batch("DROP TABLE transaction_provenance")
        .unwrap();
    assert!(s.transaction_provenance(absent).unwrap().is_none());
    assert!(
        s.provenance_transactions(0, 10, Some("missing"), None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_pruned_reconstruction_has_unknown_scope_until_writable_open() {
    let s = Store::open_in_memory().unwrap();
    s.conn
        .execute("DELETE FROM transaction_provenance_state", [])
        .unwrap();
    assert!(s.provenance_state().unwrap().is_none());
}
