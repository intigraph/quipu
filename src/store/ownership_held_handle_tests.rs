//! A handle opened BEFORE ownership becomes invalid must refuse the same
//! writes a freshly reopened handle refuses. Each case first proves an
//! ordinary write on the held handle, then compares it with a fresh open.
use super::{OwnershipState, Store};
use crate::store::Datum;
use crate::types::{Op, Value};

fn datum(store: &Store, n: i64) -> Datum {
    Datum {
        entity: store.intern("urn:held:e").unwrap(),
        attribute: store.intern("urn:held:n").unwrap(),
        value: Value::Int(n),
        valid_from: "2026-10-26".into(),
        valid_to: None,
        op: Op::Assert,
    }
}

fn write(store: &mut Store, n: i64) -> crate::Result<i64> {
    let d = datum(store, n);
    store.transact(&[d], "2026-10-26T00:00:00Z", None, None)
}

fn transactions(store: &Store) -> i64 {
    store
        .conn
        .query_row("SELECT count(*) FROM transactions", [], |r| r.get(0))
        .unwrap()
}

/// Every refusal must also hold on the other committing entrypoints.
fn assert_all_writes_refused(store: &mut Store) {
    let before = transactions(store);
    assert!(write(store, 99).is_err(), "transact must refuse");
    let d = datum(store, 98);
    assert!(
        store
            .transact_graph_batches_tx(&[(0, vec![d])], "2026-10-26T00:00:01Z", None, None)
            .is_err(),
        "batch transact must refuse"
    );
    assert!(
        store
            .graph_create_recorded("urn:held:graph", "2026-10-26T00:00:02Z", None)
            .is_err(),
        "recorded graph creation must refuse"
    );
    assert_eq!(transactions(store), before, "no refused write committed");
}

#[test]
fn held_handle_refuses_writes_after_failed_adoption_leaves_pending_marker() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pending.db");
    let p = path.to_str().unwrap();
    let mut held = Store::open(p).unwrap();
    assert_eq!(held.ownership_state().unwrap(), OwnershipState::Inactive);
    write(&mut held, 1).unwrap();

    // A directory at the sidecar path makes the record install fail after the
    // pending marker committed.
    let sidecar = path.with_extension("db.ownership.json");
    std::fs::create_dir(&sidecar).unwrap();
    assert!(Store::adopt(p).is_err());
    std::fs::remove_dir(&sidecar).unwrap();
    assert!(held.ownership_state().is_err());

    let mut fresh = Store::open(p).unwrap();
    assert!(write(&mut fresh, 2).is_err(), "fresh open refuses");
    assert_all_writes_refused(&mut held);
}

#[test]
fn held_handle_refuses_writes_after_enforced_binding_is_lost() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("enforced.db");
    let p = path.to_str().unwrap();
    drop(Store::open(p).unwrap());
    Store::adopt(p).unwrap();
    let mut held = Store::open(p).unwrap();
    assert_eq!(held.ownership_state().unwrap(), OwnershipState::Enforced);
    write(&mut held, 1).unwrap();

    std::fs::remove_file(path.with_extension("db.ownership.json")).unwrap();
    assert!(held.ownership_state().is_err());

    let mut fresh = Store::open(p).unwrap();
    assert!(write(&mut fresh, 2).is_err(), "fresh open refuses");
    assert_all_writes_refused(&mut held);
}

#[test]
fn held_inactive_legacy_handles_keep_writing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    let p = path.to_str().unwrap();
    let mut first = Store::open(p).unwrap();
    let mut second = Store::open(p).unwrap();
    write(&mut first, 1).unwrap();
    write(&mut second, 2).unwrap();
    write(&mut first, 3).unwrap();
    assert_eq!(first.ownership_state().unwrap(), OwnershipState::Inactive);
    assert!(first.require_ownership().is_err());
    assert_eq!(transactions(&first), 3);
}
