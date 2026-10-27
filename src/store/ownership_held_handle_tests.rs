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

fn count(store: &Store, table: &str) -> i64 {
    store
        .conn
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

const SHAPES: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .\n\
    <urn:held:Shape> a sh:NodeShape ; sh:targetClass <urn:held:C> .\n";

/// Every refusal must also hold on the other committing entrypoints,
/// including the direct writers outside the transact pipeline.
fn assert_all_writes_refused(store: &mut Store, overlay: i64) {
    let before = transactions(store);
    let (graphs, shapes, ontologies) = (
        count(store, "graphs"),
        count(store, "shapes"),
        count(store, "ontologies"),
    );
    let (e, a) = (
        store.intern("urn:held:e").unwrap(),
        store.intern("urn:held:n").unwrap(),
    );
    assert!(
        store
            .overlay_write(
                overlay,
                Op::Tombstone,
                e,
                a,
                Value::Int(1),
                "2026-10-26T00:00:03Z"
            )
            .is_err(),
        "overlay tombstone must refuse"
    );
    assert!(
        store.overlay_create("urn:held:overlay2", 0).is_err(),
        "overlay create must refuse"
    );
    assert!(
        store
            .load_shapes("held", SHAPES, "2026-10-26T00:00:04Z")
            .is_err(),
        "shape load must refuse"
    );
    assert!(
        store
            .load_ontology("held", SHAPES, "2026-10-26T00:00:05Z")
            .is_err(),
        "ontology load must refuse"
    );
    assert_eq!(count(store, "graphs"), graphs);
    assert_eq!(count(store, "shapes"), shapes);
    assert_eq!(count(store, "ontologies"), ontologies);
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
    let overlay = held.overlay_create("urn:held:overlay", 0).unwrap();

    // A directory at the sidecar path makes the record install fail after the
    // pending marker committed.
    let sidecar = path.with_extension("db.ownership.json");
    std::fs::create_dir(&sidecar).unwrap();
    assert!(Store::adopt(p).is_err());
    std::fs::remove_dir(&sidecar).unwrap();
    assert!(held.ownership_state().is_err());

    let mut fresh = Store::open(p).unwrap();
    assert!(write(&mut fresh, 2).is_err(), "fresh open refuses");
    assert_all_writes_refused(&mut held, overlay);
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
    let overlay = held.overlay_create("urn:held:overlay", 0).unwrap();

    std::fs::remove_file(path.with_extension("db.ownership.json")).unwrap();
    assert!(held.ownership_state().is_err());

    let mut fresh = Store::open(p).unwrap();
    assert!(write(&mut fresh, 2).is_err(), "fresh open refuses");
    assert_all_writes_refused(&mut held, overlay);
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

/// The commit fence is the backstop for direct SQL writers that no entrypoint
/// check covers: on a held handle it refuses every outer commit, and a refused
/// commit leaves nothing behind.
#[test]
fn commit_fence_refuses_direct_writers_on_a_held_handle() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("direct.db");
    let p = path.to_str().unwrap();
    drop(Store::open(p).unwrap());
    Store::adopt(p).unwrap();
    let held = Store::open(p).unwrap();
    held.conn
        .execute("CREATE TABLE direct_probe(x)", [])
        .unwrap();
    held.conn
        .execute("INSERT INTO direct_probe VALUES (1)", [])
        .unwrap();

    std::fs::remove_file(path.with_extension("db.ownership.json")).unwrap();
    assert!(
        held.conn
            .execute("INSERT INTO direct_probe VALUES (2)", [])
            .is_err(),
        "autocommit write must be refused"
    );
    assert!(held.intern("urn:held:never-interned").is_err());
    // A bookkeeping writer with no entrypoint check of its own.
    assert!(
        held.commit_consumer("held-consumer", 1, "2026-10-26T00:00:06Z")
            .is_err()
    );
    held.conn.execute_batch("SAVEPOINT probe").unwrap();
    held.conn
        .execute("INSERT INTO direct_probe VALUES (3)", [])
        .unwrap();
    assert!(
        crate::store::release::release(&held.conn, "probe").is_err(),
        "outermost release must be refused"
    );
    // SQLite alone would leave this connection inside a dead transaction;
    // the shared release helper ends it before returning the refusal.
    assert!(
        held.conn.is_autocommit(),
        "no transaction stays open after a refused commit"
    );
    assert_eq!(count(&held, "direct_probe"), 1);
    assert_eq!(count(&held, "consumers"), 0, "refused consumer offset");
    assert_eq!(
        held.lookup("urn:held:never-interned").unwrap(),
        None,
        "refused intern left no term"
    );
}

/// A held ENFORCED handle with one overlay, whose binding is then removed.
fn stale_enforced_handle(dir: &std::path::Path) -> (Store, i64, i64, i64) {
    let path = dir.join("stale.db");
    let p = path.to_str().unwrap();
    drop(Store::open(p).unwrap());
    Store::adopt(p).unwrap();
    let mut held = Store::open(p).unwrap();
    write(&mut held, 1).unwrap();
    let overlay = held.overlay_create("urn:held:overlay", 0).unwrap();
    let (e, a) = (
        held.intern("urn:held:e").unwrap(),
        held.intern("urn:held:n").unwrap(),
    );
    std::fs::remove_file(path.with_extension("db.ownership.json")).unwrap();
    assert!(held.ownership_state().is_err());
    (held, overlay, e, a)
}

#[test]
fn stale_handle_refuses_overlay_tombstone() {
    let dir = tempfile::tempdir().unwrap();
    let (mut held, overlay, e, a) = stale_enforced_handle(dir.path());
    let before = transactions(&held);
    let result = held.overlay_write(overlay, Op::Tombstone, e, a, Value::Int(1), "2026-10-27");
    assert!(result.is_err(), "overlay tombstone must refuse");
    assert_eq!(transactions(&held), before);
}

#[test]
fn stale_handle_refuses_overlay_create() {
    let dir = tempfile::tempdir().unwrap();
    let (held, ..) = stale_enforced_handle(dir.path());
    let graphs = count(&held, "graphs");
    assert!(held.overlay_create("urn:held:overlay2", 0).is_err());
    assert_eq!(count(&held, "graphs"), graphs);
}

#[test]
fn stale_handle_refuses_shape_load() {
    let dir = tempfile::tempdir().unwrap();
    let (held, ..) = stale_enforced_handle(dir.path());
    assert!(
        held.load_shapes("held", SHAPES, "2026-10-27T00:00:00Z")
            .is_err()
    );
    assert!(held.ensure_ownership_writable().is_err());
    assert_eq!(count(&held, "shapes"), 0);
}

/// Arm a one-shot trip: the next transaction row removes the binding, after
/// the entry check has passed, so only the commit fence can refuse that write.
/// Returns the record bytes to restore afterwards.
fn trip_fence_once(held: &Store, path: &std::path::Path) -> Vec<u8> {
    let sidecar = path.with_extension("db.ownership.json");
    let saved = std::fs::read(&sidecar).unwrap();
    let armed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    held.conn
        .create_scalar_function(
            "trip_ownership_once",
            0,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8,
            move |_| {
                if armed.swap(false, std::sync::atomic::Ordering::SeqCst) {
                    let _ = std::fs::remove_file(&sidecar);
                }
                Ok(0)
            },
        )
        .unwrap();
    held.conn
        .execute_batch(
            "CREATE TEMP TRIGGER trip_ownership AFTER INSERT ON main.transactions \
             BEGIN SELECT trip_ownership_once(); END;",
        )
        .unwrap();
    saved
}

/// After the fence refuses one outer commit and ownership is valid again,
/// later writes on the same handle persist or fail; none reports Ok and is
/// then discarded, and no term id is handed out twice.
#[test]
fn writes_after_a_refused_outer_commit_persist_or_fail() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("trip.db");
    let p = path.to_str().unwrap();
    drop(Store::open(p).unwrap());
    Store::adopt(p).unwrap();
    let mut held = Store::open(p).unwrap();
    write(&mut held, 1).unwrap();
    let saved = trip_fence_once(&held, &path);
    let before = transactions(&held);
    assert!(write(&mut held, 2).is_err(), "the fence refuses the commit");
    assert!(held.conn.is_autocommit(), "no dead transaction is left");
    std::fs::write(path.with_extension("db.ownership.json"), saved).unwrap();
    held.ensure_ownership_writable().unwrap();
    assert_eq!(transactions(&held), before);

    // A direct bookkeeping write now lands for real.
    held.commit_consumer("c-probe", 7, "2026-10-27T00:00:00Z")
        .unwrap();
    let other = rusqlite::Connection::open(p).unwrap();
    let offset: i64 = other
        .query_row(
            "SELECT committed_offset FROM consumers WHERE consumer_id = 'c-probe'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(offset, 7);

    // intern then transact keeps its ids, and a later intern never reuses one.
    let e = held.intern("urn:after:e").unwrap();
    let a = held.intern("urn:after:a").unwrap();
    held.transact(
        &[Datum {
            entity: e,
            attribute: a,
            value: Value::Int(3),
            valid_from: "2026-10-27".into(),
            valid_to: None,
            op: Op::Assert,
        }],
        "2026-10-27T00:00:01Z",
        None,
        None,
    )
    .unwrap();
    let z = held.intern("urn:after:z").unwrap();
    assert!(z != e && z != a, "a committed term id was handed out again");
    let persisted: i64 = other
        .query_row(
            "SELECT count(*) FROM terms WHERE iri IN ('urn:after:e', 'urn:after:a', 'urn:after:z')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(persisted, 3);
}
