//! Stage A defaults preserve legacy behavior without an ownership capability.
use super::{OwnershipState, Store};

#[test]
fn legacy_restart_and_configured_open_remain_inactive_without_adoption() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    let p = path.to_str().unwrap();
    let original = Store::open(p).unwrap();
    let epoch = original.log_epoch().unwrap();
    let id = original.store_id().unwrap();
    assert_eq!(
        original.ownership_state().unwrap(),
        OwnershipState::Inactive
    );
    assert!(original.require_ownership().is_err());
    original
        .conn
        .execute_batch("CREATE TABLE legacy_probe(x); INSERT INTO legacy_probe VALUES(1);")
        .unwrap();
    drop(original);
    for _ in 0..2 {
        let reopened = Store::open_with_attachments(p, &[]).unwrap();
        assert_eq!(
            reopened.ownership_state().unwrap(),
            OwnershipState::Inactive
        );
        assert_eq!(reopened.log_epoch().unwrap(), epoch);
        assert_eq!(reopened.store_id().unwrap(), id);
        reopened
            .conn
            .execute("INSERT INTO legacy_probe VALUES(2)", [])
            .unwrap();
        let marked: i64 = reopened
            .conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='ownership_enforcement'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(marked, 0);
    }
    assert!(!path.with_extension("db.ownership.json").exists());
}

#[test]
fn sticky_marker_cannot_downgrade_through_missing_record_or_deserialization() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("owned.db");
    let p = path.to_str().unwrap();
    drop(Store::open(p).unwrap());
    Store::adopt(p).unwrap();
    let store = Store::open(p).unwrap();
    assert_eq!(store.ownership_state().unwrap(), OwnershipState::Enforced);
    assert!(Store::open_from_bytes(&store.serialize_db().unwrap()).is_err());
    std::fs::remove_file(path.with_extension("db.ownership.json")).unwrap();
    let refused = Store::open_with_attachments(p, &[]).unwrap();
    assert!(refused.ownership_state().is_err());
    assert!(
        refused
            .conn
            .execute_batch("CREATE TABLE downgrade(x)")
            .is_err()
    );
}

#[test]
fn partial_or_malformed_records_cannot_become_legacy() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("partial.db");
    let p = path.to_str().unwrap();
    let store = Store::open(p).unwrap();
    std::fs::write(path.with_extension("db.ownership.json"), b"{}").unwrap();
    assert!(store.ownership_state().is_err());
    let blocked = Store::open(p).unwrap();
    assert!(
        blocked
            .conn
            .execute_batch("CREATE TABLE downgrade(x)")
            .is_err()
    );
    std::fs::remove_file(path.with_extension("db.ownership.json")).unwrap();
    store
        .conn
        .execute_batch("CREATE TABLE ownership_enforcement(id,version);")
        .unwrap();
    assert!(store.ownership_state().is_err());
    let blocked = Store::open(p).unwrap();
    assert!(
        blocked
            .conn
            .execute_batch("CREATE TABLE downgrade(x)")
            .is_err()
    );
}

#[test]
fn native_enforcement_never_falls_back_on_an_unavailable_or_wrong_provider() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("provider.db");
    let p = path.to_str().unwrap();
    drop(Store::open(p).unwrap());
    Store::adopt(p).unwrap();
    let store = Store::open(p).unwrap();
    assert!(
        store
            .verify_binding_with(|| Err(crate::Error::PolicyDenied("provider unavailable".into())))
            .is_err()
    );
    assert!(
        store
            .verify_binding_with(|| Ok("not the admitted host".into()))
            .is_err()
    );
    store.require_ownership().unwrap();
}
