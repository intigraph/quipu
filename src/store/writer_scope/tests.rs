use super::*;
use crate::transaction_auth::{Identity, with_identity};
use crate::writer_scope::WriteOperation::*;

fn identity(name: &str) -> Identity {
    Identity {
        principal: format!("urn:test:{name}"),
        credential_id: Some(name.into()),
        auth_class: "scoped_named_bearer".into(),
        attestation: Default::default(),
    }
}

fn grant(store: &Store, name: &str, graph: &str) -> WriteGrant {
    WriteGrant {
        version: 1,
        id: name.into(),
        principal: format!("urn:test:{name}"),
        credential_id: name.into(),
        instance: store.writer_graph_instance(graph).unwrap(),
        operations: BTreeSet::from([Update, Episode, Knot, Set]),
        valid_from: 0,
        valid_until: crate::time::epoch_secs() + 3600,
        revision: 1,
        revoked: false,
        mint_scope: None,
    }
}

fn ready() -> Store {
    let s = Store::open_in_memory_test_ns().unwrap();
    for (name, graph) in [("alice", "urn:g:a"), ("bob", "urn:g:b")] {
        s.graph_create(graph).unwrap();
        s.writer_grant_apply(&grant(&s, name, graph)).unwrap();
    }
    s
}

fn transact(s: &mut Store, graph: &str) -> Result<i64> {
    let id = s.committed_graph_id(graph)?.unwrap();
    s.transact_to_graph(&[], &crate::time::now_iso(), None, None, id)
}

#[test]
fn two_principals_both_directions_independent_of_authority_flag() {
    for authority in [false, true] {
        let mut s = ready();
        s.governance_config_mut().enforce_authority = authority;
        for (name, allowed, foreign) in [
            ("alice", "urn:g:a", "urn:g:b"),
            ("bob", "urn:g:b", "urn:g:a"),
        ] {
            with_identity(Some(identity(name)), || {
                let tx = s
                    .with_writer_scope(Update, &BTreeSet::from([allowed.into()]), |s| {
                        transact(s, allowed)
                    })
                    .unwrap();
                assert_eq!(s.transaction_auth(tx).unwrap().unwrap(), identity(name));
                let before = s.latest_tx_id().unwrap();
                for targets in [
                    BTreeSet::from([foreign.into()]),
                    BTreeSet::from([allowed.into(), foreign.into()]),
                    BTreeSet::from([String::new()]),
                ] {
                    assert!(
                        s.with_writer_scope(Update, &targets, |s| transact(s, foreign))
                            .is_err()
                    );
                    assert_eq!(s.latest_tx_id().unwrap(), before);
                }
                // No owned capability, no low-level fallback.
                assert!(transact(&mut s, allowed).is_err());
                // Declared attribution cannot select the private server-audit
                // exception. Even an empty ROOT transaction must be refused.
                assert!(
                    s.with_writer_scope(Update, &BTreeSet::from([allowed.into()]), |s| {
                        s.transact(
                            &[],
                            &crate::time::now_iso(),
                            Some("quipu"),
                            Some("write-gate verdict"),
                        )
                    })
                    .is_err()
                );
                // The low-level gate checks the actual graph too, even if an
                // adapter erroneously declared only the allowed graph.
                assert!(
                    s.with_writer_scope(Update, &BTreeSet::from([allowed.into()]), |s| {
                        transact(s, allowed)?;
                        transact(s, foreign)
                    })
                    .is_err()
                );
                assert_eq!(s.latest_tx_id().unwrap(), before);
            });
        }
    }
}

#[test]
fn credential_operation_store_expiry_revocation_and_revision_are_binding() {
    let mut s = ready();
    let original = grant(&s, "alice", "urn:g:a");
    let targets = BTreeSet::from(["urn:g:a".into()]);
    let mut wrong_credential = identity("alice");
    wrong_credential.credential_id = Some("bob".into());
    with_identity(Some(wrong_credential), || {
        assert!(
            s.with_writer_scope(Update, &targets, |s| transact(s, "urn:g:a"))
                .is_err()
        );
    });
    for mode in ["operation", "expired", "future", "revoked"] {
        let mut g = original.clone();
        g.revision = match mode {
            "operation" => 2,
            "expired" => 3,
            "future" => 4,
            _ => 5,
        };
        match mode {
            "operation" => g.operations = BTreeSet::from([Knot]),
            "expired" => {
                g.valid_from = 0;
                g.valid_until = 1;
            }
            "future" => {
                g.valid_from = crate::time::epoch_secs() + 10;
            }
            _ => g.revoked = true,
        }
        s.writer_grant_apply(&g).unwrap();
        with_identity(Some(identity("alice")), || {
            assert!(
                s.with_writer_scope(Update, &targets, |s| transact(s, "urn:g:a"))
                    .is_err(),
                "{mode}"
            );
        });
    }
    assert!(s.writer_grant_apply(&original).is_err());
    let mut other = ready();
    assert_ne!(s.store_id().unwrap(), other.store_id().unwrap());
    assert!(other.writer_grant_apply(&original).is_err());
    // A copied JSON grant cannot confer authority in the other store.
    other.conn.execute("DELETE FROM writer_grants", []).unwrap();
    with_identity(Some(identity("alice")), || {
        assert!(
            other
                .with_writer_scope(Update, &targets, |s| transact(s, "urn:g:a"))
                .is_err()
        );
    });
}

#[test]
fn graph_recreation_and_relabel_invalidate_even_with_same_timestamp() {
    let mut s = ready();
    let old = s.writer_graph_instance("urn:g:a").unwrap();
    let id = s.committed_graph_id("urn:g:a").unwrap().unwrap();
    s.graph_unregister(id).unwrap();
    s.graph_create("urn:g:a").unwrap();
    assert_ne!(s.writer_graph_instance("urn:g:a").unwrap(), old);
    let targets = BTreeSet::from(["urn:g:a".into()]);
    with_identity(Some(identity("alice")), || {
        assert!(
            s.with_writer_scope(Update, &targets, |s| transact(s, "urn:g:a"))
                .is_err()
        );
    });
    let mut g = grant(&s, "alice", "urn:g:a");
    g.revision = 2;
    s.writer_grant_apply(&g).unwrap();
    let before = s.writer_graph_instance("urn:g:a").unwrap();
    s.conn
        .execute("UPDATE graphs SET created_at=created_at WHERE g=?1", [id])
        .unwrap();
    assert_ne!(s.writer_graph_instance("urn:g:a").unwrap(), before);
    with_identity(Some(identity("alice")), || {
        assert!(
            s.with_writer_scope(Update, &targets, |s| transact(s, "urn:g:a"))
                .is_err()
        );
    });
}

#[test]
fn unwind_rolls_back_and_does_not_leave_worker_authority() {
    let mut s = ready();
    let before = s.latest_tx_id().unwrap();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_identity(Some(identity("alice")), || {
            let _: Result<()> =
                s.with_writer_scope(Update, &BTreeSet::from(["urn:g:a".into()]), |s| {
                    transact(s, "urn:g:a")?;
                    panic!("fixture");
                });
        });
    }));
    assert!(s.conn.is_autocommit());
    assert_eq!(s.latest_tx_id().unwrap(), before);
    assert!(!crate::writer_scope::is_scoped_request());
    assert!(transact(&mut s, "urn:g:b").is_ok());
}

#[test]
fn completed_revocation_cannot_be_overtaken_by_a_queued_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.db");
    let path = path.to_str().unwrap();
    let s = Store::open(path).unwrap();
    s.graph_create("urn:g:a").unwrap();
    let g = grant(&s, "alice", "urn:g:a");
    s.writer_grant_apply(&g).unwrap();
    let mut queued = Store::open(path).unwrap();
    let mut revoked = g.clone();
    revoked.revision = 2;
    revoked.revoked = true;
    s.writer_grant_apply(&revoked).unwrap();
    with_identity(Some(identity("alice")), || {
        let result = queued.with_writer_scope(Update, &BTreeSet::from(["urn:g:a".into()]), |s| {
            transact(s, "urn:g:a")
        });
        assert!(result.is_err());
    });
    assert_eq!(s.latest_tx_id().unwrap(), 0);
}

#[test]
fn overlapping_revocation_waits_for_the_data_commit_barrier() {
    use std::sync::mpsc;
    use std::time::Duration;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("barrier.db");
    let path = path.to_str().unwrap().to_owned();
    let mut writer = Store::open(&path).unwrap();
    writer.graph_create("urn:g:a").unwrap();
    let mut revoke = grant(&writer, "alice", "urn:g:a");
    writer.writer_grant_apply(&revoke).unwrap();
    revoke.revision = 2;
    revoke.revoked = true;
    let owner = Store::open(&path).unwrap();
    let (enter_tx, enter_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let pending = std::thread::spawn(move || {
        with_identity(Some(identity("alice")), || {
            writer
                .with_writer_scope(Update, &BTreeSet::from(["urn:g:a".into()]), |s| {
                    enter_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    transact(s, "urn:g:a")
                })
                .unwrap()
        })
    });
    enter_rx.recv().unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let revoker = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        owner.writer_grant_apply(&revoke).unwrap();
        done_tx.send(()).unwrap();
    });
    started_rx.recv().unwrap();
    assert!(done_rx.recv_timeout(Duration::from_millis(50)).is_err());
    release_tx.send(()).unwrap();
    let tx = pending.join().unwrap();
    done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    revoker.join().unwrap();
    let mut after = Store::open(&path).unwrap();
    assert_eq!(after.latest_tx_id().unwrap(), tx);
    with_identity(Some(identity("alice")), || {
        assert!(
            after
                .with_writer_scope(Update, &BTreeSet::from(["urn:g:a".into()]), |s| transact(
                    s, "urn:g:a"
                ))
                .is_err()
        );
    });
}

#[test]
fn write_grant_shared_subset_check_refuses_a_mixed_foreign_set() {
    let s = ready();
    let g = grant(&s, "alice", "urn:g:a");
    let now = crate::time::epoch_secs();
    assert!(g.permits("urn:test:alice", &BTreeSet::from(["urn:g:a".into()]), now));
    assert!(!g.permits(
        "urn:test:alice",
        &BTreeSet::from(["urn:g:a".into(), "urn:g:b".into()]),
        now
    ));
}

#[test]
fn simultaneous_reopens_do_not_upgrade_a_deferred_schema_snapshot() {
    use std::sync::{Arc, Barrier};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("parallel.db");
    Store::open(path.to_str().unwrap()).unwrap();
    for _ in 0..8 {
        let start = Arc::new(Barrier::new(2));
        let threads: Vec<_> = (0..2)
            .map(|_| {
                let start = start.clone();
                let path = path.clone();
                std::thread::spawn(move || {
                    start.wait();
                    Store::open(path.to_str().unwrap()).map(|_| ())
                })
            })
            .collect();
        // Join BOTH even on a failure; no stranded barrier participant can
        // hide the original open error behind a deadlocked fixture.
        let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        for result in results {
            result.unwrap();
        }
    }
}

#[test]
fn scoped_authority_and_its_triggers_do_not_travel_in_a_reconstruction() {
    let store = ready();
    let old = store.writer_graph_instance("urn:g:a").unwrap();
    let count: i64 = store
        .conn
        .query_row("SELECT count(*) FROM writer_grants", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2, "non-vacuous owner-enrolled policy");
    let dir = tempfile::tempdir().unwrap();
    let opts = crate::pack::PackOptions {
        destination: crate::share::ShareDestination::Internal,
        ..Default::default()
    };
    let out = dir.path().join("text");
    let manifest = crate::pack_full_text::pack_full_text(
        &store,
        out.to_str().unwrap(),
        &opts,
        &crate::time::now_iso(),
    )
    .unwrap();
    let rebuilt = dir.path().join("rebuilt.db");
    let (_, hash) =
        crate::pack_full_text::rebuild(out.to_str().unwrap(), rebuilt.to_str().unwrap()).unwrap();
    assert_eq!(hash, manifest.content_hash);
    let conn = Connection::open(&rebuilt).unwrap();
    for table in ["writer_grants", "writer_graph_instances"] {
        let rows: i64 = conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0, "{table} must not be reconstructed authority");
    }
    let triggers:i64=conn.query_row("SELECT count(*) FROM sqlite_master WHERE type='trigger' AND name LIKE 'writer_graph_%'",[],|r|r.get(0)).unwrap();
    assert_eq!(triggers, 0);
    drop(conn);
    Store::adopt(rebuilt.to_str().unwrap()).unwrap();
    let mut restored = Store::open(rebuilt.to_str().unwrap()).unwrap();
    assert_ne!(restored.writer_graph_instance("urn:g:a").unwrap(), old);
    with_identity(Some(identity("alice")), || {
        assert!(
            restored
                .with_writer_scope(Update, &BTreeSet::from(["urn:g:a".into()]), |s| transact(
                    s, "urn:g:a"
                ))
                .is_err()
        );
    });
}

#[test]
fn explicit_ownership_adoption_invalidates_old_grants_with_same_lineage_id() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("adopt.db");
    let path = path.to_str().unwrap();
    let store = Store::open(path).unwrap();
    store.graph_create("urn:g:a").unwrap();
    let original = grant(&store, "alice", "urn:g:a");
    store.writer_grant_apply(&original).unwrap();
    drop(store);
    Store::adopt(path).unwrap();
    let mut after = Store::open(path).unwrap();
    let current = after.writer_graph_instance("urn:g:a").unwrap();
    assert_eq!(current.store_id, original.instance.store_id);
    assert_ne!(current.generation, original.instance.generation);
    with_identity(Some(identity("alice")), || {
        assert!(
            after
                .with_writer_scope(Update, &BTreeSet::from(["urn:g:a".into()]), |s| transact(
                    s, "urn:g:a"
                ))
                .is_err()
        );
    });
}

#[test]
fn a_grant_expiring_during_work_cannot_commit_the_staged_transaction() {
    use std::time::{Duration, Instant};
    let mut store = ready();
    let mut g = grant(&store, "alice", "urn:g:a");
    g.revision = 2;
    g.valid_until = crate::time::epoch_secs() + 2;
    store.writer_grant_apply(&g).unwrap();
    let before = store.latest_tx_id().unwrap();
    let mut entered = false;
    with_identity(Some(identity("alice")), || {
        let result = store.with_writer_scope(Update, &BTreeSet::from(["urn:g:a".into()]), |s| {
            entered = true;
            let tx = transact(s, "urn:g:a")?;
            let deadline = Instant::now() + Duration::from_secs(4);
            while crate::time::epoch_secs() < g.valid_until {
                assert!(Instant::now() < deadline, "fixture clock did not advance");
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok::<_, Error>(tx)
        });
        assert!(result.is_err());
    });
    assert!(entered, "test must reach the staged write before expiry");
    assert_eq!(store.latest_tx_id().unwrap(), before);
}
