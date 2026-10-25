//! The source's transfer limits (local-sync-mode.md §6): a transaction over
//! them is refused before any of its records is built and the follower's
//! cursor holds, and a page stops at its row and byte budgets.
use super::tests::{BOARD, feed, follower, identity, in_graph, request, source};
use super::*;
use crate::applier::Outcome;
use crate::store::Datum;
use crate::types::{Op, Value};

const T: &str = "2026-10-24T00:00:00Z";

/// One transaction writing `n` small records to `graph`.
fn write_many(store: &mut Store, graph: &str, tag: &str, n: usize) {
    let g = store.graph_create(graph).unwrap();
    let datums: Vec<Datum> = (0..n)
        .map(|i| Datum {
            entity: store.intern(&format!("urn:{tag}:{i}")).unwrap(),
            attribute: store.intern("urn:p:title").unwrap(),
            value: Value::Str(format!("{tag} {i}")),
            valid_from: T.into(),
            valid_to: None,
            op: Op::Assert,
        })
        .collect();
    store.transact_to_graph(&datums, T, None, None, g).unwrap();
}

const SMALL: Limits = Limits {
    tx_max_rows: 5,
    tx_max_bytes: 1 << 20,
    page_max_rows: 6,
    page_max_bytes: 1 << 20,
};

#[test]
fn a_transaction_over_the_row_limit_is_refused_before_it_is_built_and_the_cursor_holds() {
    // ian's #817 P2: many small records are no single oversized record, so
    // the per-record check never fired and the whole transaction was built.
    let mut src = source();
    let id = identity();
    let (session, open) = Session::open(&src, &request(&src, 0), "s".into(), id.clone()).unwrap();
    let mut session = session.with_limits(SMALL);
    let (mut replica, mut applier) = follower(&id, session.scope());
    feed(&mut replica, &mut applier, &[open], 1);
    let (frames, _) = session.poll(&src, 100, None).unwrap();
    feed(&mut replica, &mut applier, &frames, 1);
    let held = applier.progress().map(|p| p.source_tx);
    let before = in_graph(&replica, BOARD);
    write_many(&mut src, BOARD, "big", 6);
    let (frames, answered) = session.poll(&src, 100, Some([7; 16])).unwrap();
    assert!(session.ended());
    assert!(!answered, "a refused page proves nothing about the head");
    assert_eq!(frames.len(), 1, "{frames:?}");
    assert_eq!(frames[0].event, event::CONTROL);
    assert!(frames[0].data.contains("tx_too_large"), "{frames:?}");
    assert!(frames[0].data.contains("6 rows"), "{frames:?}");
    let outcomes = feed(&mut replica, &mut applier, &frames, 2);
    assert!(
        !outcomes
            .iter()
            .any(|o| matches!(o, Outcome::Applied { .. })),
        "{outcomes:?}"
    );
    assert_eq!(applier.progress().map(|p| p.source_tx), held);
    assert_eq!(in_graph(&replica, BOARD), before);
}

#[test]
fn a_page_stops_at_the_row_budget_and_covers_only_what_it_sent() {
    let mut src = Store::open_in_memory().unwrap();
    for i in 0..4 {
        write_many(&mut src, BOARD, &format!("t{i}"), 3);
    }
    let id = identity();
    let (session, open) = Session::open(&src, &request(&src, 0), "s".into(), id.clone()).unwrap();
    let mut session = session.with_limits(SMALL);
    let (mut replica, mut applier) = follower(&id, session.scope());
    feed(&mut replica, &mut applier, &[open], 1);
    // 3 + 3 rows fit the budget of 6; the third transaction waits.
    let (frames, answered) = session.poll(&src, 100, Some([1; 16])).unwrap();
    assert!(!answered, "a cut page did not reach the head");
    let txs = frames.iter().filter(|f| f.event == event::TX).count();
    assert_eq!(txs, 2, "{frames:?}");
    feed(&mut replica, &mut applier, &frames, 1);
    let sent = applier.progress().unwrap().source_tx;
    assert_eq!(applier.coverage(), Some(sent), "coverage stops at the cut");
    // The follower still reaches the source by paging on.
    loop {
        let (frames, _) = session.poll(&src, 100, None).unwrap();
        if frames.is_empty() {
            break;
        }
        feed(&mut replica, &mut applier, &frames, 1);
    }
    assert_eq!(in_graph(&replica, BOARD), in_graph(&src, BOARD));
    assert_eq!(applier.coverage(), Some(src.latest_tx_id().unwrap()));
}

#[test]
fn an_oversized_transaction_later_in_a_page_waits_for_its_own_poll() {
    let mut src = Store::open_in_memory().unwrap();
    write_many(&mut src, BOARD, "small", 2);
    write_many(&mut src, BOARD, "big", 6);
    let id = identity();
    let (session, open) = Session::open(&src, &request(&src, 0), "s".into(), id.clone()).unwrap();
    let mut session = session.with_limits(SMALL);
    let (mut replica, mut applier) = follower(&id, session.scope());
    feed(&mut replica, &mut applier, &[open], 1);
    let (frames, _) = session.poll(&src, 100, None).unwrap();
    assert!(!session.ended());
    feed(&mut replica, &mut applier, &frames, 1);
    assert_eq!(in_graph(&replica, BOARD).len(), 2);
    let (frames, _) = session.poll(&src, 100, None).unwrap();
    assert!(session.ended());
    assert!(frames[0].data.contains("tx_too_large"), "{frames:?}");
}

/// A tx over the byte bound with few rows and small values: the bytes are in
/// the entity IRIs, which a value-only count never saw (ian, #817).
#[test]
fn long_entity_iris_count_against_the_byte_limit() {
    let mut src = Store::open_in_memory().unwrap();
    let g = src.graph_create(BOARD).unwrap();
    let long = "x".repeat(20_000);
    let datums: Vec<Datum> = (0..2)
        .map(|i| Datum {
            entity: src.intern(&format!("urn:{long}:{i}")).unwrap(),
            attribute: src.intern("urn:p:title").unwrap(),
            value: Value::Str("v".into()),
            valid_from: T.into(),
            valid_to: None,
            op: Op::Assert,
        })
        .collect();
    src.transact_to_graph(&datums, T, None, None, g).unwrap();
    let id = identity();
    let (session, _) = Session::open(&src, &request(&src, 0), "s".into(), id).unwrap();
    let mut session = session.with_limits(Limits {
        tx_max_bytes: 64 << 10,
        ..SMALL
    });
    let (frames, _) = session.poll(&src, 100, None).unwrap();
    assert!(session.ended());
    assert!(frames[0].data.contains("tx_too_large"), "{frames:?}");
    assert!(frames[0].data.contains("encoded bytes"), "{frames:?}");
}

#[test]
fn a_page_stops_at_the_byte_budget() {
    let mut src = Store::open_in_memory().unwrap();
    for i in 0..3 {
        write_many(&mut src, BOARD, &format!("t{i}"), 1);
    }
    let g = src.lookup(BOARD).unwrap().unwrap();
    let one = src.tx_encoded_bound(1, &[g], u64::MAX).unwrap();
    let id = identity();
    let (session, open) = Session::open(&src, &request(&src, 0), "s".into(), id.clone()).unwrap();
    let mut session = session.with_limits(Limits {
        page_max_bytes: one * 2,
        ..SMALL
    });
    let (mut replica, mut applier) = follower(&id, session.scope());
    feed(&mut replica, &mut applier, &[open], 1);
    let (frames, _) = session.poll(&src, 100, None).unwrap();
    assert_eq!(frames.iter().filter(|f| f.event == event::TX).count(), 2);
    feed(&mut replica, &mut applier, &frames, 1);
    let (frames, _) = session.poll(&src, 100, None).unwrap();
    feed(&mut replica, &mut applier, &frames, 1);
    assert_eq!(in_graph(&replica, BOARD), in_graph(&src, BOARD));
}
