//! fcntl locks are per process: only a second process can observe whether a
//! lease dropped this process's SQLite write lock. The child re-runs this test
//! binary on the ignored probe below.
use super::*;
use std::process::Command;

const PROBE_ENV: &str = "QUIPU_WRITER_LEASE_PROBE_DB";

/// Child side: try to take the database write lock without waiting.
#[test]
#[ignore = "run only as the child process of lease_drops_never_release_sqlite_locks"]
fn lease_lock_probe_child() {
    let Ok(db) = std::env::var(PROBE_ENV) else {
        return;
    };
    let conn = Connection::open(db).unwrap();
    conn.busy_timeout(std::time::Duration::ZERO).unwrap();
    match conn.execute_batch("BEGIN IMMEDIATE; ROLLBACK;") {
        Ok(()) => println!("PROBE-ACQUIRED"),
        Err(_) => println!("PROBE-REFUSED"),
    }
}

fn probe(db: &Path) -> &'static str {
    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "store::writer_lease::tests::lease_lock_probe_child",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(PROBE_ENV, db)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    if stdout.contains("PROBE-ACQUIRED") {
        "acquired"
    } else if stdout.contains("PROBE-REFUSED") {
        "refused"
    } else {
        panic!(
            "probe child did not report: {stdout} {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn lease_drops_never_release_sqlite_locks() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("store.db");
    // Rollback-journal mode keeps the write lock on the database file itself,
    // where a stray descriptor close is directly observable. In WAL mode the
    // same close drops the shared database-file lock other processes use to
    // decide they are the last connection (and may checkpoint and unlink the
    // WAL/shm under a live writer); the mechanism is identical.
    Connection::open(&db)
        .unwrap()
        .execute_batch("PRAGMA journal_mode=DELETE; CREATE TABLE seed(x);")
        .unwrap();

    // Connection A holds an open write transaction for the whole test.
    let a = Connection::open(&db).unwrap();
    a.execute_batch("BEGIN IMMEDIATE; INSERT INTO seed VALUES (1);")
        .unwrap();
    assert_eq!(probe(&db), "refused", "control: A's write lock is visible");

    // Claim and drop a lease on another in-process connection.
    let b = Connection::open(&db).unwrap();
    drop(Lease::claim(&b).unwrap().expect("native lease"));
    drop(b);
    assert_eq!(probe(&db), "refused", "a dropped lease released A's lock");

    // The Owner failure path: an exclusive upgrade refused by a shared
    // holder, then the refused lease is dropped.
    let c = Connection::open(&db).unwrap();
    let held = Lease::claim(&c).unwrap().unwrap();
    let d = Connection::open(&db).unwrap();
    let refused = Lease::claim(&d).unwrap().unwrap();
    assert!(refused.claim_exclusive().is_err(), "shared holder present");
    drop(refused);
    drop(d);
    assert_eq!(probe(&db), "refused", "a failed upgrade released A's lock");
    drop(held);
    drop(c);

    // Positive control: once A ends, the second process gets the lock.
    a.execute_batch("ROLLBACK;").unwrap();
    assert_eq!(probe(&db), "acquired");
}

#[test]
fn lease_lives_on_a_private_sibling_never_the_database() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("store.db");
    drop(crate::store::Store::open(db.to_str().unwrap()).unwrap());
    let conn = Connection::open(&db).unwrap();
    let lease = Lease::claim(&conn).unwrap().unwrap();
    let sibling = dir.path().join("store.db.writer-lease");
    let held = lease.file.metadata().unwrap();
    let on_disk = std::fs::symlink_metadata(&sibling).unwrap();
    assert_eq!((held.dev(), held.ino()), (on_disk.dev(), on_disk.ino()));
    assert_ne!(held.ino(), std::fs::metadata(&db).unwrap().ino());
    assert_eq!(on_disk.permissions().mode() & 0o077, 0, "owner-only");
    // A symlinked lease file is refused rather than followed.
    drop(lease);
    std::fs::remove_file(&sibling).unwrap();
    std::os::unix::fs::symlink(dir.path().join("elsewhere"), &sibling).unwrap();
    assert!(Lease::claim(&conn).is_err());
}
