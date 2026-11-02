//! Real SQLite boundary controls; no runtime enrollment or witness is supplied.
use super::Store;
use rusqlite::Connection;

const MARK: &str = "CREATE TABLE ownership_handoff_enrollment(payload TEXT)";

fn database() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db").to_str().unwrap().to_owned();
    (dir, path)
}

fn schema(conn: &Connection) -> Vec<(String, String)> {
    conn.prepare("SELECT name, coalesce(sql,'') FROM main.sqlite_master ORDER BY name")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn enrolled_disk_startup_refuses_before_migrations_and_adoption() {
    for marker in [
        MARK,
        "CREATE VIEW ownership_handoff_enrollment AS SELECT 'ACTIVE' AS phase",
        "CREATE TABLE OWNERSHIP_HANDOFF_ENROLLMENT(unknown_schema BLOB)",
    ] {
        let (_dir, path) = database();
        drop(Store::open(&path).unwrap());
        let raw = Connection::open(&path).unwrap();
        raw.execute_batch(marker).unwrap();
        let before = schema(&raw);
        assert!(Store::open(&path).is_err(), "no enrolled writable startup");
        assert!(Store::open_with_attachments(&path, &[]).is_err());
        assert!(
            Store::adopt(&path).is_err(),
            "adoption cannot erase enrollment"
        );
        let target = format!("{path}.restored");
        assert!(Store::restore(&path, &target).is_err());
        assert!(
            !std::path::Path::new(&target).exists(),
            "restore refuses before copying"
        );
        assert_eq!(schema(&raw), before, "no migration or epoch mint");
        let read = Store::open_read_only(&path).unwrap();
        assert_eq!(schema(&read.conn), before, "inspection remains available");
        assert!(
            read.conn
                .execute("CREATE TABLE forbidden(value TEXT)", [])
                .is_err()
        );
    }
}

#[test]
fn enrolled_serialized_and_direct_init_refuse_before_migrations() {
    let raw = Connection::open_in_memory().unwrap();
    raw.execute_batch(MARK).unwrap();
    let bytes = raw.serialize(rusqlite::MAIN_DB).unwrap().to_vec();
    assert!(Store::open_from_bytes(&bytes).is_err());
    assert!(Store::init(raw).is_err());
    // No enrollment marker is introduced on the normal compatibility path.
    let ordinary = Store::open_in_memory().unwrap();
    let copy = Store::open_from_bytes(&ordinary.serialize_db().unwrap()).unwrap();
    assert_eq!(ordinary.store_id().unwrap(), copy.store_id().unwrap());
}

#[test]
fn held_handle_enrollment_refuses_commit_release_and_autocommit() {
    for mode in ["autocommit", "commit", "release"] {
        let (_dir, path) = database();
        let held = Store::open(&path).unwrap();
        held.conn
            .execute_batch("CREATE TABLE probe(value TEXT)")
            .unwrap();
        held.conn
            .execute("INSERT INTO probe VALUES ('before')", [])
            .unwrap();
        // An independent fixture connection models a later enrollment marker.
        let external = Connection::open(&path).unwrap();
        external.execute_batch(MARK).unwrap();
        let result = match mode {
            "autocommit" => held
                .conn
                .execute_batch("INSERT INTO probe VALUES ('after')"),
            "commit" => held
                .conn
                .execute_batch("BEGIN; INSERT INTO probe VALUES ('after'); COMMIT"),
            "release" => {
                held.conn
                    .execute_batch("SAVEPOINT p; INSERT INTO probe VALUES ('after')")
                    .unwrap();
                super::release::release(&held.conn, "p")
            }
            _ => unreachable!(),
        };
        assert!(result.is_err(), "{mode} must refuse");
        assert!(held.conn.is_autocommit(), "{mode} leaves no doomed scope");
        assert_eq!(
            held.conn
                .query_row("SELECT count(*) FROM probe", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert!(held.ensure_ownership_writable().is_err());
        // Restoring old database bytes cannot revive an already-revoked pin.
        external
            .execute_batch("DROP TABLE ownership_handoff_enrollment")
            .unwrap();
        assert!(
            held.conn
                .execute("INSERT INTO probe VALUES ('rollback')", [])
                .is_err()
        );
        assert!(held.ensure_ownership_writable().is_err());
    }
}

#[test]
fn dropping_a_writer_with_a_shared_fence_cleans_wal_sidecars() {
    let (_dir, path) = database();
    let store = Store::open(&path).unwrap();
    store
        .conn
        .execute_batch("CREATE TABLE probe(value TEXT)")
        .unwrap();
    drop(store);
    for suffix in ["-wal", "-shm"] {
        assert!(!std::path::Path::new(&format!("{path}{suffix}")).exists());
    }
}

#[test]
fn stage_a_ownership_cannot_authorize_a_marked_handoff_store() {
    let (_dir, path) = database();
    drop(Store::open(&path).unwrap());
    Store::adopt(&path).unwrap();
    let held = Store::open(&path).unwrap();
    held.require_ownership().unwrap();
    let external = Connection::open(&path).unwrap();
    external.execute_batch(MARK).unwrap();
    assert!(held.require_ownership().is_err());
    assert!(held.ensure_ownership_writable().is_err());
    assert!(Store::open(&path).is_err());
}

#[test]
fn entrypoint_observation_revokes_the_pin_before_a_commit_hook_runs() {
    for observation in ["state", "writable", "owner"] {
        let (_dir, path) = database();
        drop(Store::open(&path).unwrap());
        Store::adopt(&path).unwrap();
        let held = Store::open(&path).unwrap();
        held.require_ownership().unwrap();
        held.conn
            .execute_batch("CREATE TABLE probe(value TEXT)")
            .unwrap();
        let external = Connection::open(&path).unwrap();
        external.execute_batch(MARK).unwrap();
        match observation {
            "state" => assert!(held.ownership_state().is_err()),
            "writable" => assert!(held.ensure_ownership_writable().is_err()),
            "owner" => assert!(held.require_ownership().is_err()),
            _ => unreachable!(),
        }
        external
            .execute_batch("DROP TABLE ownership_handoff_enrollment")
            .unwrap();
        assert!(
            held.conn
                .execute("INSERT INTO probe VALUES ('revived')", [])
                .is_err(),
            "{observation} must revoke before any commit hook runs"
        );
        assert!(held.require_ownership().is_err());
        assert_eq!(
            held.conn
                .query_row("SELECT count(*) FROM probe", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}

#[test]
fn check_latch_contention_refuses_without_deadlock_or_partial_write() {
    let (_dir, path) = database();
    let store = Store::open(&path).unwrap();
    store
        .conn
        .execute_batch("CREATE TABLE probe(value TEXT)")
        .unwrap();
    let pin = store.ownership_fence.as_ref().unwrap();
    pin.with_check_latch(|| {
        assert!(
            store
                .conn
                .execute("INSERT INTO probe VALUES ('blocked')", [])
                .is_err()
        );
    });
    assert!(store.conn.is_autocommit());
    assert_eq!(
        store
            .conn
            .query_row("SELECT count(*) FROM probe", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    store
        .conn
        .execute("INSERT INTO probe VALUES ('valid')", [])
        .unwrap();
}

#[cfg(feature = "server")]
#[test]
fn wake_reinstallation_keeps_the_revoked_pin_and_latch() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let (_dir, path) = database();
    let held = Store::open(&path).unwrap();
    held.conn
        .execute_batch("CREATE TABLE probe(value TEXT)")
        .unwrap();
    let wakes = Arc::new(AtomicUsize::new(0));
    let install = || {
        let count = wakes.clone();
        held.set_commit_wake(move || {
            count.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
    };
    held.ownership_fence.as_ref().unwrap().with_check_latch(|| {
        install();
        assert!(
            held.conn
                .execute("INSERT INTO probe VALUES ('contended')", [])
                .is_err()
        );
    });
    assert_eq!(wakes.load(Ordering::SeqCst), 0);
    install();
    held.conn
        .execute("INSERT INTO probe VALUES ('before')", [])
        .unwrap();
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    let external = Connection::open(&path).unwrap();
    external.execute_batch(MARK).unwrap();
    assert!(
        held.conn
            .execute("INSERT INTO probe VALUES ('blocked')", [])
            .is_err()
    );
    external
        .execute_batch("DROP TABLE ownership_handoff_enrollment")
        .unwrap();
    install();
    assert!(
        held.conn
            .execute("INSERT INTO probe VALUES ('rollback')", [])
            .is_err()
    );
    assert_eq!(
        wakes.load(Ordering::SeqCst),
        1,
        "refused commits never wake"
    );
    held.ownership_fence.as_ref().unwrap().with_check_latch(|| {
        install();
        assert!(
            held.conn
                .execute("INSERT INTO probe VALUES ('contended')", [])
                .is_err()
        );
    });
}
