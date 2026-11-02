//! Generation pin (aegis-s24.8 P1): a handle admitted under an exact
//! authority tuple must stop committing once the protected journal leaves
//! ACTIVE at that tuple. This is the retained old-A-writer shape, in process:
//! the handle is already open and keeps its connection when retirement lands.
use std::path::Path;

use super::Store;
use super::ownership_journal::{advance, create, write_atomic};
use crate::ownership_handoff_model::{
    Authority, AuthorityUnit, Certificate, Cut, Journal, Placement, Request,
};
use crate::store::Datum;
use crate::types::{Op, Value};

fn authority(generation: u64) -> Authority {
    Authority {
        store_id: "urn:example:lineage".into(),
        unit: AuthorityUnit::FullStore,
        generation,
        epoch: "epoch-A".into(),
        owner: Placement {
            principal: "A".into(),
            native: [1; 32],
        },
        policy: "existing-policy-v1".into(),
        predecessor: None,
    }
}

fn active() -> Journal {
    Journal::Active {
        authority: authority(0),
    }
}

fn retiring() -> Journal {
    Journal::Retiring {
        request: Request {
            operation: "op-B-0".into(),
            operator: "existing-operator".into(),
            successor_generation: 1,
            source: authority(0),
            destination: Placement {
                principal: "B".into(),
                native: [2; 32],
            },
            profile: "existing-profile-v1".into(),
        },
    }
}

fn retired() -> Journal {
    let Journal::Retiring { request } = retiring() else {
        unreachable!()
    };
    Journal::Retired {
        certificate: Certificate {
            request,
            cut: Cut {
                transaction: 1,
                facts: [4; 32],
                artifact: [5; 32],
            },
        },
    }
}

fn pending() -> Journal {
    let Journal::Retired { certificate } = retired() else {
        unreachable!()
    };
    let successor = certificate.successor("epoch-B").unwrap();
    Journal::Pending {
        certificate,
        successor,
    }
}

fn write(store: &mut Store, n: i64) -> crate::Result<i64> {
    let d = Datum {
        entity: store.intern("urn:pin:e")?,
        attribute: store.intern("urn:pin:n")?,
        value: Value::Int(n),
        valid_from: "2026-11-01".into(),
        valid_to: None,
        op: Op::Assert,
    };
    store.transact(&[d], "2026-11-01T00:00:00Z", None, None)
}

fn count(store: &Store, table: &str) -> i64 {
    store
        .conn
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

/// An open disk-store handle pinned to `authority(0)` at `journal`.
fn open_pinned(dir: &Path, journal: &Path) -> Store {
    let store = Store::open(dir.join("pinned.db").to_str().unwrap()).unwrap();
    store
        .ownership_fence
        .as_ref()
        .expect("a disk store has a commit fence")
        .pin_journal(journal.to_path_buf(), authority(0));
    store
}

#[test]
fn a_pinned_handle_writes_only_while_its_journal_is_active() {
    let dir = tempfile::tempdir().unwrap();
    let journal = dir.path().join("lineage.journal");
    create(&journal, &active()).unwrap();
    let mut store = open_pinned(dir.path(), &journal);

    write(&mut store, 1).unwrap();
    store
        .conn
        .execute_batch("CREATE TABLE direct_probe(x); INSERT INTO direct_probe VALUES (1);")
        .unwrap();
    let facts = count(&store, "transactions");

    // Retirement lands while the handle is open.
    advance(&journal, &active(), &retiring()).unwrap();

    // The first write after retirement is direct SQL: only the commit hook
    // sees it, so this proves the hook re-reads the journal.
    assert!(
        store
            .conn
            .execute("INSERT INTO direct_probe VALUES (2)", [])
            .is_err(),
        "a direct autocommit write must refuse after RETIRING"
    );
    assert!(write(&mut store, 2).is_err());
    let err = store.ensure_ownership_writable().unwrap_err().to_string();
    assert!(err.contains("RETIRING"), "{err}");
    assert_eq!(
        count(&store, "direct_probe"),
        1,
        "refused write left no row"
    );
    assert_eq!(
        count(&store, "transactions"),
        facts,
        "refused write left no fact"
    );
    assert!(
        store.conn.is_autocommit(),
        "no doomed transaction stays open"
    );
}

#[test]
fn restoring_older_journal_bytes_does_not_revive_a_revoked_handle() {
    let dir = tempfile::tempdir().unwrap();
    let journal = dir.path().join("lineage.journal");
    create(&journal, &active()).unwrap();
    let active_bytes = std::fs::read(&journal).unwrap();
    let mut store = open_pinned(dir.path(), &journal);
    write(&mut store, 1).unwrap();

    advance(&journal, &active(), &retiring()).unwrap();
    assert!(write(&mut store, 2).is_err());

    // An ordinary file restore puts the ACTIVE bytes back.
    std::fs::write(&journal, &active_bytes).unwrap();
    assert!(write(&mut store, 3).is_err(), "a revoked pin stays revoked");
    assert!(store.ensure_ownership_writable().is_err());
}

#[test]
fn every_non_admitting_journal_state_refuses_and_names_its_role() {
    // A valid later ACTIVE: the certified successor at generation 1.
    let Journal::Pending { successor, .. } = pending() else {
        unreachable!()
    };
    let other = Journal::Active {
        authority: successor,
    };
    let cases: Vec<(&str, Option<Journal>, &str)> = vec![
        ("missing", None, "UNKNOWN"),
        ("retiring", Some(retiring()), "RETIRING"),
        ("retired", Some(retired()), "RETIRED"),
        ("pending", Some(pending()), "PENDING"),
        ("other generation", Some(other), "generation 1"),
    ];
    for (label, state, expect) in cases {
        let dir = tempfile::tempdir().unwrap();
        let journal = dir.path().join("lineage.journal");
        if let Some(state) = &state {
            write_atomic(&journal, state).unwrap();
        }
        let mut store = open_pinned(dir.path(), &journal);
        assert!(write(&mut store, 1).is_err(), "{label}: write admitted");
        assert_eq!(count(&store, "transactions"), 0, "{label}: no fact");
        // A refused COMMIT reads as a generic SQLite error; the handle keeps
        // the role-specific reason.
        let err = store.ensure_ownership_writable().unwrap_err().to_string();
        assert!(err.contains(expect), "{label}: {err}");
    }

    // A corrupt journal is UNKNOWN too.
    let dir = tempfile::tempdir().unwrap();
    let journal = dir.path().join("lineage.journal");
    std::fs::write(&journal, b"corrupt").unwrap();
    let mut store = open_pinned(dir.path(), &journal);
    assert!(write(&mut store, 1).is_err());
    let err = store.ensure_ownership_writable().unwrap_err().to_string();
    assert!(err.contains("UNKNOWN"), "{err}");
}

#[test]
fn an_unpinned_legacy_handle_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(dir.path().join("legacy.db").to_str().unwrap()).unwrap();
    write(&mut store, 1).unwrap();
    assert_eq!(count(&store, "transactions"), 1);
}

#[test]
fn latch_contention_refuses_a_commit_without_revoking() {
    let dir = tempfile::tempdir().unwrap();
    let journal = dir.path().join("lineage.journal");
    create(&journal, &active()).unwrap();
    let store = open_pinned(dir.path(), &journal);
    store
        .conn
        .execute_batch("CREATE TABLE direct_probe(x)")
        .unwrap();

    let retirement =
        super::ownership_latch::exclusive(&super::ownership_latch::path_for(&journal)).unwrap();
    assert!(
        store
            .conn
            .execute("INSERT INTO direct_probe VALUES (1)", [])
            .is_err(),
        "a commit under retirement's latch must refuse"
    );
    assert_eq!(count(&store, "direct_probe"), 0);
    // Retirement aborted without writing the journal: the handle is intact.
    drop(retirement);
    store
        .conn
        .execute("INSERT INTO direct_probe VALUES (2)", [])
        .unwrap();
    assert_eq!(count(&store, "direct_probe"), 1);
}

/// Lock order: retirement takes the latch, then waits for SQLite's writer
/// lock. An open writer's COMMIT meets the latch without waiting, refuses,
/// and releases SQLite's lock, so retirement proceeds and nothing deadlocks.
#[test]
fn retirement_and_an_open_writer_cannot_deadlock() {
    use std::sync::mpsc;
    use std::time::Duration;

    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("pinned.db");
    let journal = dir.path().join("lineage.journal");
    create(&journal, &active()).unwrap();
    let store = open_pinned(dir.path(), &journal);
    store
        .conn
        .execute_batch("CREATE TABLE direct_probe(x); BEGIN; INSERT INTO direct_probe VALUES (1);")
        .unwrap();

    let (latched, latched_rx) = mpsc::channel();
    let (done, done_rx) = mpsc::channel();
    let retire_journal = journal.clone();
    let retirement = std::thread::spawn(move || {
        let _latch =
            super::ownership_latch::exclusive(&super::ownership_latch::path_for(&retire_journal))
                .unwrap();
        latched.send(()).unwrap();
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.busy_timeout(Duration::from_secs(10)).unwrap();
        // Waits for the open writer's SQLite lock.
        conn.execute_batch("BEGIN IMMEDIATE").unwrap();
        advance(&retire_journal, &active(), &retiring()).unwrap();
        conn.execute_batch("COMMIT").unwrap();
        done.send(()).unwrap();
    });

    latched_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(
        done_rx.recv_timeout(Duration::from_millis(300)).is_err(),
        "retirement must wait for the open writer"
    );
    let commit = store.conn.execute_batch("COMMIT");
    assert!(
        super::release::settle(&store.conn, commit).is_err(),
        "the writer's COMMIT refuses under retirement's latch"
    );
    assert!(store.conn.is_autocommit());
    done_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("retirement completes: no deadlock");
    retirement.join().unwrap();

    assert_eq!(count(&store, "direct_probe"), 0, "the refused row is gone");
    assert!(write(&mut { store }, 1).is_err(), "RETIRING now refuses");
}
