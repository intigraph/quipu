//! The source's stream sessions driven straight into the shared applier: a
//! scoped follower fed only by the stream equals the source in its scope, a
//! challenge is answered only by a read that reached the head, and an epoch
//! change ends the session.
use std::sync::Arc;

use ring::signature::Ed25519KeyPair;

use super::*;
use crate::applier::{Applier, Outcome};
use crate::store::Datum;
use crate::types::{Op, Value};

const T: &str = "2026-10-24T00:00:00Z";
const BOARD: &str = "urn:graph:board";
const OTHER: &str = "urn:graph:other";

fn identity() -> Arc<SigningIdentity> {
    let doc = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
    Arc::new(SigningIdentity::new(
        Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap(),
        "quipu",
    ))
}

fn write(store: &mut Store, graph: &str, entity: &str, value: &str) {
    let datum = Datum {
        entity: store.intern(entity).unwrap(),
        attribute: store.intern("urn:p:title").unwrap(),
        value: Value::Str(value.into()),
        valid_from: T.into(),
        valid_to: None,
        op: Op::Assert,
    };
    let g = store.graph_create(graph).unwrap();
    store.transact_to_graph(&[datum], T, None, None, g).unwrap();
}

/// A source with an in-scope graph and an out-of-scope one, interleaved.
fn source() -> Store {
    let mut s = Store::open_in_memory().unwrap();
    for i in 0..4 {
        write(
            &mut s,
            BOARD,
            &format!("urn:item:{i}"),
            &format!("board {i}"),
        );
        write(&mut s, OTHER, &format!("urn:x:{i}"), "elsewhere");
    }
    s
}

fn request(src: &Store, since: i64) -> StreamRequest {
    StreamRequest {
        graphs: vec![BOARD.into()],
        context_graphs: vec![],
        since,
        expected_store_id: src.store_id().unwrap(),
    }
}

/// The source's facts in `graph` only, as the applier's comparison sees them.
fn in_graph(store: &Store, graph: &str) -> std::collections::BTreeSet<String> {
    crate::applier::live_digest(store)
        .into_iter()
        .filter(|f| f.starts_with(&format!("{graph} ")))
        .collect()
}

fn follower(id: &SigningIdentity, scope: &str) -> (Store, Applier) {
    let store = Store::open_in_memory().unwrap();
    let mut applier = Applier::open(&store).unwrap();
    applier.trust_stream(&id.public_key_hex(), scope);
    (store, applier)
}

fn feed(store: &mut Store, applier: &mut Applier, frames: &[Frame], now: u64) -> Vec<Outcome> {
    frames
        .iter()
        .map(|f| applier.apply_frame(store, f.event, &f.data, now).unwrap())
        .collect()
}

#[test]
fn a_scoped_follower_fed_only_by_the_stream_equals_the_source_in_scope() {
    let src = source();
    let id = identity();
    let (mut session, open) =
        Session::open(&src, &request(&src, 0), "s1".into(), id.clone()).unwrap();
    let (mut replica, mut applier) = follower(&id, session.scope());
    feed(&mut replica, &mut applier, &[open], 1);
    // Small pages: the stream must page through and still cover everything.
    loop {
        let (frames, _) = session.poll(&src, 2, None).unwrap();
        if frames.is_empty() {
            break;
        }
        for o in feed(&mut replica, &mut applier, &frames, 1) {
            assert!(
                matches!(
                    o,
                    Outcome::Applied { .. } | Outcome::CoverageAdvanced { .. }
                ),
                "{o:?}"
            );
        }
    }
    assert_eq!(in_graph(&replica, BOARD), in_graph(&src, BOARD));
    assert!(in_graph(&replica, OTHER).is_empty());
    // Coverage reached the source head, past the last in-scope transaction.
    assert_eq!(applier.coverage(), Some(src.latest_tx_id().unwrap()));
}

#[test]
fn the_tail_continues_from_the_cursor_and_a_new_commit_arrives() {
    let mut src = source();
    let id = identity();
    let (mut session, open) =
        Session::open(&src, &request(&src, 0), "s1".into(), id.clone()).unwrap();
    let (mut replica, mut applier) = follower(&id, session.scope());
    feed(&mut replica, &mut applier, &[open], 1);
    let (frames, _) = session.poll(&src, 100, None).unwrap();
    feed(&mut replica, &mut applier, &frames, 1);
    // Nothing new: an idle poll sends nothing.
    assert!(session.poll(&src, 100, None).unwrap().0.is_empty());
    write(&mut src, BOARD, "urn:item:new", "fresh");
    let (frames, _) = session.poll(&src, 100, None).unwrap();
    feed(&mut replica, &mut applier, &frames, 2);
    assert_eq!(in_graph(&replica, BOARD), in_graph(&src, BOARD));
    // An out-of-scope commit advances coverage only.
    write(&mut src, OTHER, "urn:x:new", "elsewhere");
    let (frames, _) = session.poll(&src, 100, None).unwrap();
    assert_eq!(
        feed(&mut replica, &mut applier, &frames, 3),
        vec![Outcome::CoverageAdvanced {
            watermark: src.latest_tx_id().unwrap()
        }]
    );
}

#[test]
fn a_challenge_is_answered_only_by_a_read_that_reached_the_head() {
    let src = source();
    let id = identity();
    let (mut session, open) =
        Session::open(&src, &request(&src, 0), "s1".into(), id.clone()).unwrap();
    let (mut replica, mut applier) = follower(&id, session.scope());
    feed(&mut replica, &mut applier, &[open], 1);
    applier.challenge("s1", [6; 16], 100).unwrap();
    // A short page leaves the nonce unanswered.
    let (frames, answered) = session.poll(&src, 1, Some([6; 16])).unwrap();
    assert!(!answered);
    feed(&mut replica, &mut applier, &frames, 110);
    assert_eq!(applier.freshness(110).staleness_bound_ms, None);
    // The poll that reaches the head answers it.
    let (frames, answered) = session.poll(&src, 100, Some([6; 16])).unwrap();
    assert!(answered);
    let outcomes = feed(&mut replica, &mut applier, &frames, 130);
    assert_eq!(
        outcomes.last(),
        Some(&Outcome::ProofAccepted { t_send_ms: 100 })
    );
    assert!(applier.freshness(150).within(100));
    // A quiet source still answers, with no new frames before the coverage.
    applier.challenge("s1", [7; 16], 5_000).unwrap();
    let (frames, answered) = session.poll(&src, 100, Some([7; 16])).unwrap();
    assert!(answered && frames.len() == 1);
    feed(&mut replica, &mut applier, &frames, 5_010);
    assert!(applier.freshness(5_020).within(100));
}

#[test]
fn a_heartbeat_keeps_coverage_but_never_bounds_freshness() {
    let src = source();
    let id = identity();
    let (mut session, open) =
        Session::open(&src, &request(&src, 0), "s1".into(), id.clone()).unwrap();
    let (mut replica, mut applier) = follower(&id, session.scope());
    feed(&mut replica, &mut applier, &[open], 1);
    let (frames, _) = session.poll(&src, 100, None).unwrap();
    feed(&mut replica, &mut applier, &frames, 1);
    let w = applier.coverage();
    let beat = session.heartbeat();
    assert!(matches!(
        applier
            .apply_frame(&mut replica, beat.event, &beat.data, 60_000)
            .unwrap(),
        Outcome::CoverageAdvanced { .. }
    ));
    assert_eq!(applier.coverage(), w);
    assert_eq!(applier.freshness(60_000).staleness_bound_ms, None);
}

#[test]
fn a_transaction_split_into_parts_reaches_the_follower_whole() {
    let mut src = Store::open_in_memory().unwrap();
    let g = src.graph_create(BOARD).unwrap();
    let datums: Vec<Datum> = (0..40)
        .map(|i| Datum {
            entity: src.intern(&format!("urn:item:{i}")).unwrap(),
            attribute: src.intern("urn:p:title").unwrap(),
            value: Value::Str(format!("a long enough title to need parts {i}")),
            valid_from: T.into(),
            valid_to: None,
            op: Op::Assert,
        })
        .collect();
    src.transact_to_graph(&datums, T, None, None, g).unwrap();
    let id = identity();
    let (session, open) = Session::open(&src, &request(&src, 0), "s1".into(), id.clone()).unwrap();
    let mut session = session.with_part_max_len(2_000);
    let (mut replica, mut applier) = follower(&id, session.scope());
    feed(&mut replica, &mut applier, &[open], 1);
    let (frames, _) = session.poll(&src, 100, None).unwrap();
    assert!(frames.iter().filter(|f| f.event == event::TX_PART).count() > 1);
    let outcomes = feed(&mut replica, &mut applier, &frames, 1);
    let (body, tail) = outcomes.split_at(outcomes.len() - 2);
    assert!(
        body.iter().all(|o| matches!(o, Outcome::Buffered { .. })),
        "{body:?}"
    );
    assert!(matches!(tail[0], Outcome::Applied { .. }), "{tail:?}");
    assert_eq!(in_graph(&replica, BOARD), in_graph(&src, BOARD));
}

#[test]
fn a_new_log_epoch_ends_the_session_with_a_signed_epoch_end() {
    let src = source();
    let id = identity();
    let (mut session, open) =
        Session::open(&src, &request(&src, 0), "s1".into(), id.clone()).unwrap();
    let (mut replica, mut applier) = follower(&id, session.scope());
    feed(&mut replica, &mut applier, &[open], 1);
    let (frames, _) = session.poll(&src, 100, None).unwrap();
    feed(&mut replica, &mut applier, &frames, 1);
    // A history-removing operation re-mints the epoch.
    src.mint_log_epoch("test: history removed").unwrap();
    let (frames, _) = session.poll(&src, 100, None).unwrap();
    assert!(session.ended());
    let outcomes = feed(&mut replica, &mut applier, &frames, 2);
    assert!(
        matches!(outcomes[0], Outcome::Rebootstrap { .. }),
        "{outcomes:?}"
    );
}

#[test]
fn a_session_refuses_another_lineage_an_unserved_graph_or_a_future_cursor() {
    let src = source();
    let id = identity();
    let mut wrong = request(&src, 0);
    wrong.expected_store_id = "another-store".into();
    assert!(Session::open(&src, &wrong, "s".into(), id.clone()).is_err());
    let mut unregistered = request(&src, 0);
    unregistered.graphs = vec!["urn:graph:missing".into()];
    assert!(Session::open(&src, &unregistered, "s".into(), id.clone()).is_err());
    let ahead = request(&src, src.latest_tx_id().unwrap() + 1);
    assert!(Session::open(&src, &ahead, "s".into(), id).is_err());
}

#[test]
fn the_stream_serves_the_scope_digest_an_as_of_share_names() {
    let src = source();
    let (session, _) = Session::open(&src, &request(&src, 0), "s".into(), identity()).unwrap();
    assert_eq!(session.scope(), scope_digest(&[BOARD.to_string()], &[]));
}

#[test]
fn a_session_refuses_the_metadata_graph_and_a_frozen_graph() {
    // Shared with the as-of cut: the graph-metadata graph is source-local and
    // never streamed (s24.1.1 ruling). A frozen graph's rows live in its pack,
    // which the cut reads but the stream does not serve yet.
    let mut src = source();
    let id = identity();
    assert!(Session::open(&src, &request(&src, 0), "s".into(), id.clone()).is_ok());
    let mut meta = request(&src, 0);
    meta.graphs = vec![crate::namespace::META_GRAPH_IRI.into()];
    let err = Session::open(&src, &meta, "s".into(), id.clone())
        .err()
        .expect("meta refused");
    assert!(
        format!("{err:?}").contains("graph-metadata graph"),
        "{err:?}"
    );
    let packs = tempfile::tempdir().unwrap();
    src.freeze_graph(
        OTHER,
        packs.path().to_str().unwrap(),
        "2026-10-25T00:00:00Z",
        None,
    )
    .unwrap();
    let mut frozen = request(&src, 0);
    frozen.graphs = vec![OTHER.into()];
    let err = Session::open(&src, &frozen, "s".into(), id.clone())
        .err()
        .expect("frozen refused");
    assert!(format!("{err:?}").contains("frozen into a pack"), "{err:?}");
    // The unfrozen graph beside it is still served.
    assert!(Session::open(&src, &request(&src, 0), "s".into(), id).is_ok());
}

#[test]
fn a_freeze_committed_during_the_open_ends_the_session_instead_of_covering_nothing() {
    // ian's #817 race: the scope check passed, a freeze of the scope graph
    // committed, then the epoch was read, so the session opened on the NEW
    // epoch and signed coverage over zero records with the fact still in
    // scope. The open's reads are one snapshot, epoch first: the session
    // opens on the old epoch and its first poll ends it.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.db");
    let path = path.to_str().unwrap();
    let mut src = Store::open(path).unwrap();
    write(&mut src, BOARD, "urn:item:0", "board 0");
    let mut writer = Store::open(path).unwrap();
    let packs = tempfile::tempdir().unwrap();
    let before = src.log_epoch().unwrap();
    let id = identity();
    let (mut session, open) =
        Session::open_between(&src, &request(&src, 0), "s".into(), id.clone(), || {
            writer
                .freeze_graph(
                    BOARD,
                    packs.path().to_str().unwrap(),
                    "2026-10-25T00:00:00Z",
                    None,
                )
                .unwrap();
        })
        .unwrap();
    assert_ne!(
        src.log_epoch().unwrap(),
        before,
        "the freeze re-minted the epoch"
    );
    assert!(
        open.data.contains(&before),
        "opened on the old epoch: {}",
        open.data
    );
    let (frames, _) = session.poll(&src, 100, None).unwrap();
    assert!(session.ended());
    assert_eq!(frames.len(), 1, "{frames:?}");
    assert_eq!(frames[0].event, event::CONTROL, "{frames:?}");
    assert!(frames[0].data.contains("epoch_end"), "{frames:?}");
    let (mut replica, mut applier) = follower(&id, session.scope());
    let outcomes = feed(&mut replica, &mut applier, &[open, frames[0].clone()], 1);
    assert!(
        matches!(outcomes.last(), Some(Outcome::Rebootstrap { .. })),
        "{outcomes:?}"
    );
}

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
