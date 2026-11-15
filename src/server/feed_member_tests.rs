//! `member` change-stream filters (aegis-n0wc.3, stream-filters.md §3.1):
//! the acceptance control, graph scope, exact-term matching, ring/store
//! parity across catch-up and late registration, per-commit read cost, and
//! the condition limit and release.
use std::sync::atomic::Ordering;
use std::time::Duration;

use quipu::store::Datum;
use quipu::store::changes::Capture;
use quipu::store::changes::member::MemberSpec;
use quipu::types::{Op, Value};
use serde_json::Value as Json;

use super::tests::{BOARD, OTHER, T, quiet, settled, shared, stable};
use super::{Hub, MAX_MEMBER_CONDITIONS, RecordFilter, Serve};
use crate::SharedStore;

const STATUS: &str = "urn:p:status";
const ASSIGNEE: &str = "urn:p:assignee";
const A: &str = "<urn:p:assignee> \"A\"";

/// One transaction in `graph`: `(entity, attribute, value, op)` rows.
fn tx(shared: &SharedStore, graph: &str, rows: &[(&str, &str, Value, Op)]) -> i64 {
    let mut store = shared.lock();
    let data: Vec<Datum> = rows
        .iter()
        .map(|(e, a, v, op)| Datum {
            entity: store.intern(e).unwrap(),
            attribute: store.intern(a).unwrap(),
            value: v.clone(),
            valid_from: T.into(),
            valid_to: None,
            op: *op,
        })
        .collect();
    let g = match store.registered_graph_id(graph).unwrap() {
        Some(g) => g,
        None => store.graph_create(graph).unwrap(),
    };
    store.transact_to_graph(&data, T, None, None, g).unwrap()
}

fn s(v: &str) -> Value {
    Value::Str(v.into())
}

fn spec(text: &str) -> MemberSpec {
    MemberSpec::parse(text).unwrap()
}

/// The store's page at `cursor` for this filter, as GET /changes reads it.
fn store_page(
    shared: &SharedStore,
    cursor: i64,
    graph: Option<&str>,
    predicate: Option<&str>,
    member: &MemberSpec,
) -> Json {
    let store = shared.read();
    let scope = quipu::store::changes::TermScope {
        attributes: predicate.map(|p| store.lookup(p).unwrap().into_iter().collect()),
        entities: None,
    };
    let g = graph.map(|iri| store.lookup(iri).unwrap().unwrap());
    match store.member_condition(member).unwrap() {
        Some(cond) => {
            store.changes_after_member(cursor, 1, Capture::OldAndNewValues, g, &scope, &cond)
        }
        None => {
            let nothing = quipu::store::changes::TermScope {
                attributes: Some(Vec::new()),
                entities: None,
            };
            store.changes_after_scoped(cursor, 1, Capture::OldAndNewValues, g, &nothing)
        }
    }
    .unwrap()
    .to_json()
}

/// Follow the hub from `start` as a stream does, checking every ring page
/// against the store page at the same cursor, and paging the store on
/// `CatchUp`. Returns `(tx, records)` per delivered page and the catch-ups.
fn follow(
    hub: &Hub,
    shared: &SharedStore,
    start: i64,
    graph: Option<&str>,
    predicate: Option<&str>,
    member: &MemberSpec,
) -> (Vec<(i64, Vec<Json>)>, usize) {
    let filter = RecordFilter {
        graph,
        predicates: predicate.map(|p| [p].into()),
        member: Some(member),
        ..RecordFilter::default()
    };
    let (mut cursor, mut scanned, mut catchups) = (start, 0, 0);
    let mut pages = Vec::new();
    loop {
        let want = store_page(shared, cursor, graph, predicate, member);
        let case = format!("{graph:?} {predicate:?} {member:?} at {cursor}");
        let page = match hub.serve_filtered(cursor, &filter, &mut scanned) {
            Serve::Page(page) => {
                assert_eq!(stable(&page), stable(&want), "ring vs store: {case}");
                page
            }
            Serve::Wait => {
                assert!(
                    want["records"].as_array().unwrap().is_empty(),
                    "{case}: {want}"
                );
                return (pages, catchups);
            }
            Serve::CatchUp => {
                catchups += 1;
                if want["records"].as_array().unwrap().is_empty() {
                    // As the stream does: nothing in scope up to the watermark.
                    scanned = scanned.max(want["watermark_tx"].as_i64().unwrap());
                    continue;
                }
                want
            }
        };
        cursor = page["next_tx"].as_i64().unwrap();
        pages.push((cursor, page["records"].as_array().unwrap().clone()));
    }
}

/// Acceptance (aegis-n0wc.3): `predicate=<status>` + `member=<assignee> "A"`
/// gets the status change on A's item and nothing for B's; the transaction
/// that reassigns A's item to B arrives once, and no later change on it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn member_acceptance_status_changes_on_one_assignees_items() {
    let shared = shared();
    let hub = Hub::of(&shared);
    let _lease = hub.lease_member(spec(A)).unwrap();
    settled(&shared, &hub).await;
    let start = shared.read().latest_tx_id().unwrap();
    let assign = tx(
        &shared,
        BOARD,
        &[
            ("urn:item:a", ASSIGNEE, s("A"), Op::Assert),
            ("urn:item:b", ASSIGNEE, s("B"), Op::Assert),
        ],
    );
    let a_open = tx(
        &shared,
        BOARD,
        &[("urn:item:a", STATUS, s("open"), Op::Assert)],
    );
    let b_open = tx(
        &shared,
        BOARD,
        &[("urn:item:b", STATUS, s("open"), Op::Assert)],
    );
    let reassign = tx(
        &shared,
        BOARD,
        &[
            ("urn:item:a", ASSIGNEE, s("A"), Op::Retract),
            ("urn:item:a", ASSIGNEE, s("B"), Op::Assert),
            ("urn:item:a", STATUS, s("open"), Op::Retract),
            ("urn:item:a", STATUS, s("handed-over"), Op::Assert),
        ],
    );
    let after = tx(
        &shared,
        BOARD,
        &[("urn:item:a", STATUS, s("handed-over"), Op::Retract)],
    );
    settled(&shared, &hub).await;
    let (pages, catchups) = follow(&hub, &shared, start, None, Some(STATUS), &spec(A));
    assert_eq!(
        catchups, 0,
        "a condition held before the writes is served from the ring"
    );
    let txs: Vec<i64> = pages.iter().map(|p| p.0).collect();
    assert_eq!(
        txs,
        [a_open, reassign],
        "not {assign} (no status), {b_open} (B) or {after}"
    );
    assert_eq!(pages[0].1.len(), 1);
    assert_eq!(pages[0].1[0]["entity"], "urn:item:a");
    let moved: Vec<&str> = pages[1]
        .1
        .iter()
        .map(|r| r["op"].as_str().unwrap())
        .collect();
    assert_eq!(
        moved,
        ["retract", "assert"],
        "both status records of the move-out"
    );
    // Without the predicate the move-out carries the assignee retract too.
    let (pages, _) = follow(&hub, &shared, assign, None, None, &spec(A));
    let moved = &pages.iter().find(|p| p.0 == reassign).unwrap().1;
    assert!(
        moved
            .iter()
            .any(|r| r["attribute"] == ASSIGNEE && r["old_value"] == "A")
    );
    assert!(pages.iter().all(|p| p.0 != after && p.0 != b_open));
}

/// A membership fact in graph Y never admits a subject for `graph=X`, nor
/// for a record in X without `graph`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn member_is_evaluated_in_the_records_graph_only() {
    let shared = shared();
    let hub = Hub::of(&shared);
    let _lease = hub.lease_member(spec(A)).unwrap();
    settled(&shared, &hub).await;
    let start = shared.read().latest_tx_id().unwrap();
    let in_y = tx(
        &shared,
        OTHER,
        &[("urn:item:g", ASSIGNEE, s("A"), Op::Assert)],
    );
    tx(
        &shared,
        BOARD,
        &[("urn:item:g", STATUS, s("open"), Op::Assert)],
    );
    settled(&shared, &hub).await;
    let (pages, _) = follow(&hub, &shared, start, Some(BOARD), None, &spec(A));
    assert!(pages.is_empty(), "graph=X: {pages:?}");
    let (pages, _) = follow(&hub, &shared, start, None, Some(STATUS), &spec(A));
    assert!(pages.is_empty(), "a record in X: {pages:?}");
    // Positive control: in Y the fact admits its own transaction.
    let (pages, _) = follow(&hub, &shared, start, Some(OTHER), None, &spec(A));
    assert_eq!(pages.iter().map(|p| p.0).collect::<Vec<_>>(), [in_y]);
}

/// `"A"` does not match `"A"^^<urn:t>`; the typed condition does.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn member_plain_literal_does_not_match_a_typed_one() {
    let shared = shared();
    let hub = Hub::of(&shared);
    let typed_spec = spec("<urn:p:assignee> \"A\"^^<urn:t>");
    let _plain = hub.lease_member(spec(A)).unwrap();
    let _typed = hub.lease_member(typed_spec.clone()).unwrap();
    settled(&shared, &hub).await;
    let start = shared.read().latest_tx_id().unwrap();
    let typed = Value::Typed {
        lexical: "A".into(),
        datatype: "urn:t".into(),
    };
    tx(
        &shared,
        BOARD,
        &[("urn:item:t", ASSIGNEE, typed, Op::Assert)],
    );
    let status = tx(
        &shared,
        BOARD,
        &[("urn:item:t", STATUS, s("open"), Op::Assert)],
    );
    settled(&shared, &hub).await;
    let (pages, _) = follow(&hub, &shared, start, None, Some(STATUS), &spec(A));
    assert!(pages.is_empty(), "\"A\" matched a typed literal: {pages:?}");
    let (pages, _) = follow(&hub, &shared, start, None, Some(STATUS), &typed_spec);
    assert_eq!(pages.iter().map(|p| p.0).collect::<Vec<_>>(), [status]);
}

/// Ring and store pages agree for member filters with graph, predicate and
/// an unknown predicate; a condition registered after the ring read its
/// transactions catches up from the store over them, then rides the ring.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn member_ring_and_store_pages_agree_across_catch_up() {
    let shared = shared();
    let hub = Hub::of(&shared);
    settled(&shared, &hub).await;
    let start = shared.read().latest_tx_id().unwrap();
    let write_round = |round: usize| {
        tx(
            &shared,
            BOARD,
            &[
                ("urn:item:1", ASSIGNEE, s("A"), Op::Assert),
                ("urn:item:2", ASSIGNEE, s("B"), Op::Assert),
            ],
        );
        tx(
            &shared,
            BOARD,
            &[("urn:item:1", STATUS, s(&format!("s{round}")), Op::Assert)],
        );
        tx(
            &shared,
            OTHER,
            &[("urn:item:2", STATUS, s(&format!("s{round}")), Op::Assert)],
        );
        tx(
            &shared,
            BOARD,
            &[
                ("urn:item:1", ASSIGNEE, s("A"), Op::Retract),
                ("urn:item:2", ASSIGNEE, s("B"), Op::Retract),
            ],
        );
    };
    write_round(0);
    settled(&shared, &hub).await;
    // Registered after the ring read round 0: those entries hold no
    // membership for these conditions.
    let b = spec("<urn:p:assignee> \"B\"");
    let never = spec("<urn:p:never> \"A\"");
    let _leases = [spec(A), b.clone(), never.clone()].map(|s| hub.lease_member(s).unwrap());
    let (pages, catchups) = follow(&hub, &shared, start, None, None, &spec(A));
    assert!(catchups > 0, "round 0 must catch up from the store");
    assert_eq!(pages.len(), 3, "assign, status, retract: {pages:?}");
    let caught_up = shared.read().latest_tx_id().unwrap();
    write_round(1);
    settled(&shared, &hub).await;
    let cases: [(Option<&str>, Option<&str>, &MemberSpec); 5] = [
        (None, None, &spec(A)),
        (Some(BOARD), Some(STATUS), &spec(A)),
        (None, Some(STATUS), &b),
        (Some(OTHER), None, &b),
        (None, None, &never),
    ];
    for (graph, predicate, member) in cases {
        let (_, catchups) = follow(&hub, &shared, caught_up, graph, predicate, member);
        assert_eq!(
            catchups, 0,
            "round 1 rides the ring: {graph:?} {predicate:?} {member:?}"
        );
        follow(&hub, &shared, start, graph, predicate, member);
    }
    let (pages, _) = follow(&hub, &shared, caught_up, None, None, &never);
    assert!(pages.is_empty());
}

/// With no member subscriber a commit costs one hub read and no membership
/// read; with one condition, one batched membership read per commit; once
/// its last lease is dropped, none again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn member_reads_cost_nothing_without_a_condition_and_stop_on_release() {
    let shared = shared();
    let hub = Hub::of(&shared);
    settled(&shared, &hub).await;
    quiet(&hub).await;
    let commit = |i: usize| commit(&shared, &hub, i);
    assert_eq!(
        commit(0).await,
        (1, 0),
        "no member subscriber: no membership read"
    );
    let lease = hub.lease_member(spec(A)).unwrap();
    assert_eq!(
        commit(1).await,
        (1, 0),
        "a predicate the store never saw: no read"
    );
    tx(
        &shared,
        BOARD,
        &[("urn:item:x", ASSIGNEE, s("A"), Op::Assert)],
    );
    settled(&shared, &hub).await;
    quiet(&hub).await;
    let second = hub.lease_member(spec(A)).unwrap();
    assert_eq!(
        commit(2).await,
        (1, 1),
        "one condition, two subscribers: one read"
    );
    drop(lease);
    assert_eq!(commit(3).await, (1, 1), "still held by the second lease");
    drop(second);
    assert_eq!(commit(4).await, (1, 0), "released: no read, no leak");
    assert!(hub.member_specs().is_empty());
}

/// One status commit; the hub's change reads and membership reads for it.
async fn commit(shared: &SharedStore, hub: &Hub, i: usize) -> (u64, u64) {
    let reads = || hub.reads.load(Ordering::Relaxed);
    let member_reads = || hub.member_reads.load(Ordering::Relaxed);
    let before = (reads(), member_reads());
    tx(
        shared,
        BOARD,
        &[(&format!("urn:item:{i}"), STATUS, s("open"), Op::Assert)],
    );
    settled(shared, hub).await;
    quiet(hub).await;
    (reads() - before.0, member_reads() - before.1)
}

/// At most 64 distinct conditions; a held one is shared, and a release frees
/// its place. The refusal names the limit.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn member_conditions_are_capped_and_released() {
    let shared = shared();
    let hub = Hub::of(&shared);
    let mut leases: Vec<_> = (0..MAX_MEMBER_CONDITIONS)
        .map(|i| {
            hub.lease_member(spec(&format!("<urn:p:assignee> \"{i}\"")))
                .unwrap()
        })
        .collect();
    let refused = hub
        .lease_member(spec(A))
        .err()
        .expect("the 65th is refused");
    assert!(refused.contains("64"), "{refused}");
    let shared_lease = hub.lease_member(spec("<urn:p:assignee> \"0\""));
    assert!(
        shared_lease.is_ok(),
        "a held condition is shared, not counted again"
    );
    leases.pop();
    assert!(
        hub.lease_member(spec(A)).is_ok(),
        "a released place is reusable"
    );
}

/// `member` is a change-stream filter: one parseable condition, refused on
/// the event stream; the stream follows it from the store and then the hub.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn member_scope_and_a_streams_frames() {
    use super::super::{Cursor, Params, member_lease, scope_ok};
    let p = |value: Json| -> Params { serde_json::from_value(value).unwrap() };
    assert!(scope_ok(&p(serde_json::json!({"member": A})), true));
    assert!(!scope_ok(&p(serde_json::json!({"member": A})), false));
    for bad in ["", "<urn:p> alice", "urn:p \"A\"", "<urn:p> _:b"] {
        assert!(
            !scope_ok(&p(serde_json::json!({"member": bad})), true),
            "{bad:?}"
        );
    }
    let shared = shared();
    let hub = Hub::of(&shared);
    settled(&shared, &hub).await;
    let start = shared.read().latest_tx_id().unwrap();
    tx(
        &shared,
        BOARD,
        &[("urn:item:a", ASSIGNEE, s("A"), Op::Assert)],
    );
    let a_open = tx(
        &shared,
        BOARD,
        &[("urn:item:a", STATUS, s("open"), Op::Assert)],
    );
    tx(
        &shared,
        BOARD,
        &[("urn:item:b", STATUS, s("open"), Op::Assert)],
    );
    let reassign = tx(
        &shared,
        BOARD,
        &[
            ("urn:item:a", ASSIGNEE, s("A"), Op::Retract),
            ("urn:item:a", STATUS, s("moved"), Op::Assert),
        ],
    );
    tx(
        &shared,
        BOARD,
        &[("urn:item:a", STATUS, s("done"), Op::Assert)],
    );
    // The hub read these before the condition existed: the stream must
    // catch up over them from the store and still see each frame once.
    settled(&shared, &hub).await;
    let params = p(serde_json::json!({"predicate": STATUS, "member": A}));
    let lease = member_lease(&shared, &params).unwrap();
    let mut cursor = Cursor::open(shared.clone(), start, params, true, None);
    cursor.member = lease;
    let mut frames = Vec::new();
    while let Ok(Some(frame)) =
        tokio::time::timeout(Duration::from_millis(500), cursor.next_frame()).await
    {
        frames.push(frame);
    }
    let ids: Vec<Option<i64>> = frames.iter().map(|f| f.id).collect();
    assert_eq!(ids, [Some(a_open), Some(reassign)], "{frames:?}");
    let since: Vec<Json> = frames
        .iter()
        .map(|f| serde_json::from_str::<Json>(&f.data).unwrap()["since"].clone())
        .collect();
    assert_eq!(since, [serde_json::json!(start), serde_json::json!(a_open)]);
    drop(cursor);
    assert!(
        hub.member_specs().is_empty(),
        "the cursor's lease is released"
    );
}
