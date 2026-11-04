//! Real disk copies exercise the ownership boundary, not mocked filenames.
use super::Store;

fn snapshot(store: &Store, path: &std::path::Path) {
    store
        .conn
        .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
        .unwrap();
}

#[test]
fn ownership_copy_refuses_sql_writes_until_explicit_adopt() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("original.db");
    let copy = dir.path().join("copy.db");
    drop(Store::open(original.to_str().unwrap()).unwrap());
    Store::adopt(original.to_str().unwrap()).unwrap();
    let store = Store::open(original.to_str().unwrap()).unwrap();
    let epoch = store.log_epoch().unwrap();
    store.require_ownership().unwrap();
    snapshot(&store, &copy);
    let copied = Store::open(copy.to_str().unwrap()).unwrap();
    assert!(copied.require_ownership().is_err());
    assert!(
        copied
            .conn
            .execute("UPDATE log_epoch SET reason='bad'", [])
            .is_err()
    );
    assert_eq!(copied.log_epoch().unwrap(), epoch);
    let adopted = Store::adopt(copy.to_str().unwrap()).unwrap();
    assert_ne!(adopted, epoch);
    let owned = Store::open(copy.to_str().unwrap()).unwrap();
    owned.require_ownership().unwrap();
    owned
        .conn
        .execute("UPDATE log_epoch SET reason='owned'", [])
        .unwrap();
    assert_eq!(store.log_epoch().unwrap(), epoch);
    assert_eq!(store.store_id().unwrap(), owned.store_id().unwrap());
}

#[test]
fn ownership_copying_the_sidecar_cannot_launder_a_new_path_or_host() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.db");
    let b = dir.path().join("b.db");
    drop(Store::open(a.to_str().unwrap()).unwrap());
    Store::adopt(a.to_str().unwrap()).unwrap();
    let store = Store::open(a.to_str().unwrap()).unwrap();
    snapshot(&store, &b);
    std::fs::copy(
        a.with_extension("db.ownership.json"),
        b.with_extension("db.ownership.json"),
    )
    .unwrap();
    assert!(
        Store::open(b.to_str().unwrap())
            .unwrap()
            .require_ownership()
            .is_err()
    );
    let file = a.with_extension("db.ownership.json");
    let mut binding: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    binding["host"] = "another host".into();
    std::fs::write(file, serde_json::to_vec(&binding).unwrap()).unwrap();
    let wrong_host = Store::open(a.to_str().unwrap()).unwrap();
    assert!(wrong_host.require_ownership().is_err());
    assert!(
        wrong_host
            .conn
            .execute("UPDATE log_epoch SET reason='bad'", [])
            .is_err()
    );
}

#[test]
fn ownership_restore_is_consistent_new_epoch_and_never_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.db");
    let target = dir.path().join("target.db");
    let store = Store::open(source.to_str().unwrap()).unwrap();
    let old = store.log_epoch().unwrap();
    let new = Store::restore(source.to_str().unwrap(), target.to_str().unwrap()).unwrap();
    assert_ne!(new, old);
    let restored = Store::open(target.to_str().unwrap()).unwrap();
    restored.require_ownership().unwrap();
    assert_eq!(restored.store_id().unwrap(), store.store_id().unwrap());
    assert!(Store::restore(source.to_str().unwrap(), target.to_str().unwrap()).is_err());
    assert_eq!(restored.log_epoch().unwrap(), new);
    assert_eq!(store.log_epoch().unwrap(), old);
}

#[test]
fn ownership_adoption_is_explicit_each_time_and_missing_record_is_not_migrated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    drop(Store::open(path.to_str().unwrap()).unwrap());
    Store::adopt(path.to_str().unwrap()).unwrap();
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let before = store.log_epoch().unwrap();
    std::fs::remove_file(path.with_extension("db.ownership.json")).unwrap();
    let legacy = Store::open(path.to_str().unwrap()).unwrap();
    assert!(legacy.require_ownership().is_err());
    assert!(!path.with_extension("db.ownership.json").exists());
    let first = Store::adopt(path.to_str().unwrap()).unwrap();
    let second = Store::adopt(path.to_str().unwrap()).unwrap();
    assert_ne!(first, before);
    assert_ne!(first, second);
    assert!(Store::adopt(dir.path().join("missing.db").to_str().unwrap()).is_err());
}

#[test]
fn ownership_configured_open_and_immutable_replication_cannot_bypass_binding() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("original.db");
    let b = dir.path().join("copy.db");
    drop(Store::open(a.to_str().unwrap()).unwrap());
    Store::adopt(a.to_str().unwrap()).unwrap();
    let original = Store::open_with_attachments(a.to_str().unwrap(), &[]).unwrap();
    original.require_ownership().unwrap();
    snapshot(&original, &b);
    let configured = Store::open_with_attachments(b.to_str().unwrap(), &[]).unwrap();
    assert!(configured.require_ownership().is_err());
    assert!(
        configured
            .conn
            .execute("UPDATE log_epoch SET reason='bad'", [])
            .is_err()
    );
    assert!(
        Store::open_immutable(b.to_str().unwrap())
            .unwrap()
            .require_ownership()
            .is_err()
    );
}

#[test]
fn ownership_invalid_record_keeps_existing_database_bytes_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    {
        drop(Store::open(path.to_str().unwrap()).unwrap());
        Store::adopt(path.to_str().unwrap()).unwrap();
        let store = Store::open(path.to_str().unwrap()).unwrap();
        store.checkpoint_wal_truncate().unwrap();
    }
    std::fs::write(path.with_extension("db.ownership.json"), b"not valid json").unwrap();
    let before = std::fs::read(&path).unwrap();
    let inspected = Store::open(path.to_str().unwrap()).unwrap();
    assert!(inspected.require_ownership().is_err());
    assert!(
        inspected
            .conn
            .execute("CREATE TABLE forbidden(x)", [])
            .is_err()
    );
    drop(inspected);
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn ownership_failed_sidecar_install_never_claims_adoption_success() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    drop(Store::open(path.to_str().unwrap()).unwrap());
    Store::adopt(path.to_str().unwrap()).unwrap();
    let original = Store::open(path.to_str().unwrap()).unwrap();
    let old = original.log_epoch().unwrap();
    let record = path.with_extension("db.ownership.json");
    std::fs::remove_file(&record).unwrap();
    std::fs::create_dir(&record).unwrap();
    assert!(Store::adopt(path.to_str().unwrap()).is_err());
    // The epoch commit precedes the file install. The failure is partial, not
    // evidence of no change and never permission for an automatic retry.
    let partial_epoch = original.log_epoch().unwrap();
    assert_ne!(partial_epoch, old);
    assert!(Store::adopt(path.to_str().unwrap()).is_err());
    assert_eq!(original.log_epoch().unwrap(), partial_epoch);
    let inspected = Store::open(path.to_str().unwrap()).unwrap();
    assert!(inspected.require_ownership().is_err());
    assert!(
        inspected
            .conn
            .execute("UPDATE log_epoch SET reason='bad'", [])
            .is_err()
    );
    assert!(record.is_dir());
}
