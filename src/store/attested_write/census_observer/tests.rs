//! Disposable stores only. Deliberately no production enrollment or live tests.
use super::*;
use crate::store::attested_write::tests::{footprint, proposal, ready, sign};
use crate::store::{Datum, Store};
use crate::types::{Op, Value};
use crate::verdict_schemes::webauthn_tests::Authenticator;
use rusqlite::trace::{TraceEvent, TraceEventCodes};
use std::cell::{Cell, RefCell};

thread_local! {
    static TRACE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static FIXED: Cell<bool> = const { Cell::new(false) };
    static SELECT_MUTANT: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn fixed_clock() -> bool {
    FIXED.with(Cell::get)
}

pub(in crate::store::attested_write) fn maybe_select(store: &Store) {
    if SELECT_MUTANT.with(Cell::get) {
        let _: i64 = store
            .conn
            .query_row("SELECT enabled FROM attest_switch WHERE id=1", [], |r| {
                r.get(0)
            })
            .unwrap();
    }
}

struct Armed;
impl Armed {
    fn new(observer: Arc<Observer>) -> Self {
        TEST_OBSERVER.with(|value| assert!(value.replace(Some(observer)).is_none()));
        Self
    }
}
impl Drop for Armed {
    fn drop(&mut self) {
        TEST_OBSERVER.with(|value| value.replace(None));
        FIXED.with(|value| value.set(false));
        SELECT_MUTANT.with(|value| value.set(false));
    }
}

fn next(observer: &Observer) -> Observation {
    let Read::Observation(row) = observer.try_take() else {
        panic!("missing observation")
    };
    assert!(
        row.preimages_unknown && row.base_unknown && row.profile_unknown && row.identity_unknown
    );
    row
}

#[test]
fn signed_commit_and_replay_preserve_native_receipts_and_missing_evidence() {
    let auth = Authenticator::es256();
    let mut store = ready(&auth);
    let c = store.attest_challenge(&proposal("private-value")).unwrap();
    let observer = Arc::new(Observer::default());
    let _armed = Armed::new(observer.clone());
    let receipt = store
        .attest_commit(&c.nonce, &sign(&auth, &c, 1).as_assertion())
        .unwrap();
    assert_eq!(receipt.challenge.nonce, c.nonce);
    let row = next(&observer);
    assert!(matches!(row.completion, Completion::NativeRelease(tx) if tx>0));
    let before = footprint(&store);
    assert!(store.attest_verify_receipt(&c.nonce).unwrap());
    assert!(
        store
            .attest_commit(&c.nonce, &sign(&auth, &c, 2).as_assertion())
            .is_err()
    );
    assert_eq!(footprint(&store), before);
    assert_eq!(observer.try_take(), Read::Empty);
    assert_eq!(observer.health().before_handoff, 1);
}

#[test]
fn authority_rollback_keeps_the_native_audit_and_never_commits_an_observation() {
    let auth = Authenticator::es256();
    let mut store = ready(&auth);
    let c = store.attest_challenge(&proposal("private-value")).unwrap();
    store.governance_config_mut().enforce_authority = true;
    store.set_principal_chain(vec!["nobody".into()]);
    let before = footprint(&store);
    let observer = Arc::new(Observer::default());
    let _armed = Armed::new(observer.clone());
    assert!(
        store
            .attest_commit(&c.nonce, &sign(&auth, &c, 1).as_assertion())
            .is_err()
    );
    assert_eq!(footprint(&store), before);
    assert_eq!(next(&observer).completion, Completion::RolledBack);
    assert_eq!(
        store
            .events_after(0, 1000, Some(&["write.refused".into()]), None)
            .unwrap()
            .len(),
        1
    );
}

fn trace(event: TraceEvent<'_>) {
    if let TraceEvent::Stmt(statement, _) = event {
        TRACE.with(|rows| rows.borrow_mut().push(statement.expanded_sql().unwrap()));
    }
}

fn traced_write(armed: bool, mutant: bool) -> (Vec<String>, usize, Option<Observation>) {
    let mut store = Store::open_in_memory_test_ns().unwrap();
    let graph = "https://example.org/observed";
    store.graph_create(graph).unwrap();
    let entity = store.intern("https://example.org/item").unwrap();
    let attribute = store.intern("https://example.org/value").unwrap();
    let observer = Arc::new(Observer::default());
    let _guard = armed.then(|| Armed::new(observer.clone()));
    FIXED.with(|value| value.set(true));
    SELECT_MUTANT.with(|value| value.set(mutant));
    TRACE.with(|rows| rows.borrow_mut().clear());
    store
        .conn
        .trace_v2(TraceEventCodes::SQLITE_TRACE_STMT, Some(trace));
    store
        .attest_atomic(|_, ts| {
            Ok((
                (),
                Some((
                    graph.into(),
                    "native-actor".into(),
                    vec![Datum {
                        entity,
                        attribute,
                        value: Value::Str("same-bound-value".into()),
                        valid_from: ts.into(),
                        valid_to: None,
                        op: Op::Assert,
                    }],
                )),
            ))
        })
        .unwrap();
    store.conn.trace_v2(TraceEventCodes::empty(), None);
    let rows = TRACE.with(RefCell::take);
    FIXED.with(|value| value.set(false));
    SELECT_MUTANT.with(|value| value.set(false));
    let graph_id = store.registered_graph_id(graph).unwrap().unwrap();
    let count = store.current_facts_in_graph(graph_id).unwrap().len();
    let observation = match observer.try_take() {
        Read::Observation(row) => Some(row),
        Read::Empty => None,
        Read::Unavailable => panic!("unavailable"),
    };
    (rows, count, observation)
}

#[test]
fn identical_native_statements_and_bindings_reject_a_bounded_select_mutant() {
    let (baseline, before, absent) = traced_write(false, false);
    let (enabled, after, row) = traced_write(true, false);
    assert!(!baseline.is_empty());
    assert_eq!(baseline, enabled, "observer changed native SQL or bindings");
    assert_eq!(before, after);
    assert!(absent.is_none());
    assert!(row.unwrap().preimages_unknown);
    let (mutant, same_data, _) = traced_write(true, true);
    assert_eq!(
        same_data, before,
        "mutant positive: native write still succeeds"
    );
    assert_ne!(
        baseline, mutant,
        "trace failed to detect the naive SELECT repair"
    );
}

#[test]
fn bounded_queue_and_busy_poison_abandonment_are_explicit() {
    let observer = Arc::new(Observer::default());
    for _ in 0..CAPACITY {
        Stage::capture(Some(observer.clone()), 1).finish(Completion::NoData);
    }
    Stage::capture(Some(observer.clone()), 1).finish(Completion::NativeRelease(0));
    assert_eq!(observer.health().dropped, 1);
    for _ in 0..CAPACITY {
        assert_eq!(next(&observer).completion, Completion::NoData);
    }
    let lock = observer.queue.lock().unwrap();
    assert_eq!(observer.try_take(), Read::Unavailable);
    Stage::capture(Some(observer.clone()), 1).finish(Completion::Unknown);
    drop(lock);
    Stage::capture(Some(observer.clone()), 65).finish(Completion::NoData);
    drop(Stage::capture(Some(observer.clone()), 1));
    assert_eq!(observer.health().dropped, 3);
    assert_eq!(observer.health().abandoned, 1);
    let copied = observer.clone();
    assert!(
        std::thread::spawn(move || {
            let _lock = copied.queue.lock().unwrap();
            panic!("poison control")
        })
        .join()
        .is_err()
    );
    Stage::capture(Some(observer.clone()), 1).finish(Completion::NoData);
    assert_eq!(observer.try_take(), Read::Unavailable);
    assert_eq!(observer.health().dropped, 4);
}

#[test]
fn native_commit_survives_full_busy_and_poisoned_observers() {
    for fault in ["full", "busy", "poison"] {
        let auth = Authenticator::es256();
        let mut store = ready(&auth);
        let c = store.attest_challenge(&proposal("private-value")).unwrap();
        let observer = Arc::new(Observer::default());
        if fault == "full" {
            for _ in 0..CAPACITY {
                Stage::capture(Some(observer.clone()), 1).finish(Completion::NoData);
            }
        }
        if fault == "poison" {
            let copy = observer.clone();
            assert!(
                std::thread::spawn(move || {
                    let _lock = copy.queue.lock().unwrap();
                    panic!("private poison control")
                })
                .join()
                .is_err()
            );
        }
        let _lock = (fault == "busy").then(|| observer.queue.lock().unwrap());
        let _armed = Armed::new(observer.clone());
        store
            .attest_commit(&c.nonce, &sign(&auth, &c, 1).as_assertion())
            .unwrap();
        assert!(store.attest_verify_receipt(&c.nonce).unwrap());
        let (receipts, consumed, facts) = footprint(&store);
        assert_eq!((receipts, consumed), (1, 1));
        assert!(facts > 0);
        assert_eq!(
            observer.health().dropped,
            1,
            "native success must expose observation loss"
        );
    }
}

#[test]
fn private_no_data_settlement_is_not_a_graph_commit() {
    let auth = Authenticator::es256();
    let mut store = ready(&auth);
    let before = footprint(&store);
    let observer = Arc::new(Observer::default());
    let _armed = Armed::new(observer.clone());
    store.attest_atomic(|_, _| Ok(((), None))).unwrap();
    assert_eq!(footprint(&store), before);
    assert_eq!(next(&observer).completion, Completion::NoData);
    Stage::capture(Some(observer.clone()), 1).finish(Completion::NativeRelease(0));
    assert_eq!(next(&observer).completion, Completion::Unknown);
}

#[test]
fn loss_counter_saturates_without_wrapping() {
    let count = AtomicU64::new(u64::MAX - 1);
    increment(&count);
    assert_eq!(count.load(Ordering::Relaxed), u64::MAX);
    increment(&count);
    assert_eq!(count.load(Ordering::Relaxed), u64::MAX);
}
