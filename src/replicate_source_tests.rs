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
pub(super) const BOARD: &str = "urn:graph:board";
const OTHER: &str = "urn:graph:other";

pub(super) fn identity() -> Arc<SigningIdentity> {
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
pub(super) fn source() -> Store {
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

pub(super) fn request(src: &Store, since: i64) -> StreamRequest {
    StreamRequest {
        graphs: vec![BOARD.into()],
        context_graphs: vec![],
        since,
        expected_store_id: src.store_id().unwrap(),
    }
}

/// The source's facts in `graph` only, as the applier's comparison sees them.
pub(super) fn in_graph(store: &Store, graph: &str) -> std::collections::BTreeSet<String> {
    crate::applier::live_digest(store)
        .into_iter()
        .filter(|f| f.starts_with(&format!("{graph} ")))
        .collect()
}

pub(super) fn follower(id: &SigningIdentity, scope: &str) -> (Store, Applier) {
    let store = Store::open_in_memory().unwrap();
    let mut applier = Applier::open(&store).unwrap();
    applier.trust_stream(&id.public_key_hex(), scope);
    (store, applier)
}

pub(super) fn feed(
    store: &mut Store,
    applier: &mut Applier,
    frames: &[Frame],
    now: u64,
) -> Vec<Outcome> {
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

/// master -> relay -> leaf, all through the one applier and producer.
struct Tree {
    master: Store,
    master_key: Arc<SigningIdentity>,
    relay: Store,
    relay_applier: Applier,
    upstream: Session,
    leaf: Store,
    leaf_applier: Applier,
    downstream: Session,
}

fn graphs_of(scope: &[&str]) -> Vec<String> {
    scope.iter().map(|g| (*g).to_string()).collect()
}

impl Tree {
    fn new(master: Store, scope: &[&str]) -> Self {
        Self::with_parts(master, scope, None)
    }

    fn with_parts(master: Store, scope: &[&str], part_max_len: Option<usize>) -> Self {
        let master_key = identity();
        let relay_key = identity();
        let mut req = request(&master, 0);
        req.graphs = graphs_of(scope);
        let (mut upstream, open) =
            Session::open(&master, &req, "up".into(), master_key.clone()).unwrap();
        let (mut relay, mut relay_applier) = follower(&master_key, upstream.scope());
        feed(&mut relay, &mut relay_applier, &[open], 1);
        let (frames, _) = upstream.poll(&master, 100, None).unwrap();
        feed(&mut relay, &mut relay_applier, &frames, 1);
        let allowed = graphs_of(scope);
        let (downstream, open) = Session::open_relay(
            &relay,
            &req,
            (&allowed, &[]),
            "down".into(),
            relay_key.clone(),
        )
        .unwrap();
        let downstream = match part_max_len {
            Some(n) => downstream.with_part_max_len(n),
            None => downstream,
        };
        let (mut leaf, mut leaf_applier) = follower(&relay_key, downstream.scope());
        assert!(matches!(
            feed(&mut leaf, &mut leaf_applier, &[open], 1)[0],
            Outcome::SessionOpened { .. }
        ));
        Self {
            master,
            master_key,
            relay,
            relay_applier,
            upstream,
            leaf,
            leaf_applier,
            downstream,
        }
    }

    /// Move everything one hop at a time, in pages of `page` transactions.
    fn sync(&mut self, page: usize) {
        loop {
            let (frames, _) = self.upstream.poll(&self.master, page, None).unwrap();
            feed(&mut self.relay, &mut self.relay_applier, &frames, 2);
            let (down, _) = self.downstream.poll(&self.relay, page, None).unwrap();
            for o in feed(&mut self.leaf, &mut self.leaf_applier, &down, 2) {
                assert!(
                    matches!(
                        o,
                        Outcome::Applied { .. }
                            | Outcome::CoverageAdvanced { .. }
                            | Outcome::Buffered { .. }
                    ),
                    "{o:?}"
                );
            }
            if frames.is_empty() && down.is_empty() {
                return;
            }
        }
    }
}

#[test]
fn a_leaf_behind_a_relay_equals_the_master_in_scope() {
    let mut tree = Tree::new(source(), &[BOARD]);
    tree.sync(100);
    assert_eq!(in_graph(&tree.leaf, BOARD), in_graph(&tree.master, BOARD));
    assert!(in_graph(&tree.leaf, OTHER).is_empty());
    // The leaf follows the master's numbering and coverage, not the relay's.
    assert_eq!(
        tree.leaf_applier.progress().unwrap().source_tx,
        tree.relay_applier.progress().unwrap().source_tx
    );
    assert_eq!(tree.leaf_applier.coverage(), tree.relay_applier.coverage());
    // A new master commit crosses both hops.
    write(&mut tree.master, BOARD, "urn:item:late", "late");
    tree.sync(100);
    assert_eq!(in_graph(&tree.leaf, BOARD), in_graph(&tree.master, BOARD));
}

#[test]
fn a_relay_pages_a_two_graph_scope_without_splitting_a_source_transaction() {
    let mut tree = Tree::new(source(), &[BOARD, OTHER]);
    tree.sync(1);
    for graph in [BOARD, OTHER] {
        assert_eq!(in_graph(&tree.leaf, graph), in_graph(&tree.master, graph));
    }
}

#[test]
fn a_relay_never_covers_past_what_it_verified_upstream() {
    let mut tree = Tree::new(source(), &[BOARD]);
    tree.sync(100);
    let relay_w = tree.relay_applier.coverage().unwrap();
    // The master moves on, but the relay has not polled upstream.
    write(&mut tree.master, OTHER, "urn:x:new", "elsewhere");
    let (down, _) = tree.downstream.poll(&tree.relay, 100, None).unwrap();
    feed(&mut tree.leaf, &mut tree.leaf_applier, &down, 3);
    assert_eq!(tree.leaf_applier.coverage(), Some(relay_w));
    assert!(relay_w < tree.master.latest_tx_id().unwrap());
}

#[test]
fn a_large_transaction_crosses_the_relay_in_parts() {
    let mut master = Store::open_in_memory().unwrap();
    let g = master.graph_create(BOARD).unwrap();
    let datums: Vec<Datum> = (0..40)
        .map(|i| Datum {
            entity: master.intern(&format!("urn:item:{i}")).unwrap(),
            attribute: master.intern("urn:p:title").unwrap(),
            value: Value::Str(format!("a long enough title to need parts {i}")),
            valid_from: T.into(),
            valid_to: None,
            op: Op::Assert,
        })
        .collect();
    master.transact_to_graph(&datums, T, None, None, g).unwrap();
    let mut tree = Tree::with_parts(master, &[BOARD], Some(2_000));
    let (down, _) = tree.downstream.poll(&tree.relay, 100, None).unwrap();
    assert!(down.iter().filter(|f| f.event == event::TX_PART).count() > 1);
    let outcomes = feed(&mut tree.leaf, &mut tree.leaf_applier, &down, 2);
    assert!(
        outcomes
            .iter()
            .any(|o| matches!(o, Outcome::Applied { .. }))
    );
    assert_eq!(in_graph(&tree.leaf, BOARD), in_graph(&tree.master, BOARD));
}

#[test]
fn a_leaf_challenge_is_answered_through_the_relay() {
    let mut tree = Tree::new(source(), &[BOARD]);
    tree.sync(100);
    tree.leaf_applier.challenge("down", [8; 16], 1_000).unwrap();
    // A fresh upstream proof at the relay's coverage, after the challenge (§6).
    let held = tree.relay_applier.coverage().unwrap();
    tree.downstream.upstream_proof(held);
    let (down, answered) = tree
        .downstream
        .poll(&tree.relay, 100, Some([8; 16]))
        .unwrap();
    assert!(answered);
    let outcomes = feed(&mut tree.leaf, &mut tree.leaf_applier, &down, 1_020);
    assert_eq!(
        outcomes.last(),
        Some(&Outcome::ProofAccepted { t_send_ms: 1_000 })
    );
    assert!(tree.leaf_applier.freshness(1_030).within(100));
}

#[test]
fn a_relay_answers_a_leaf_challenge_only_from_a_fresh_upstream_proof() {
    let mut tree = Tree::new(source(), &[BOARD]);
    tree.sync(100);
    // The relay holds coverage already, but no proof was requested upstream
    // after this challenge: its stored coverage is not freshness.
    let (down, answered) = tree
        .downstream
        .poll(&tree.relay, 100, Some([8; 16]))
        .unwrap();
    assert!(!answered);
    assert!(
        down.iter()
            .all(|f| !f.data.contains(&hex::encode([8u8; 16])))
    );
    // A proof the relay does not yet hold answers nothing either.
    let held = tree.relay_applier.coverage().unwrap();
    tree.downstream.upstream_proof(held + 1);
    let (_, answered) = tree
        .downstream
        .poll(&tree.relay, 100, Some([8; 16]))
        .unwrap();
    assert!(!answered);
    // One proof answers one nonce.
    tree.downstream.upstream_proof(held);
    let (_, first) = tree
        .downstream
        .poll(&tree.relay, 100, Some([8; 16]))
        .unwrap();
    let (_, second) = tree
        .downstream
        .poll(&tree.relay, 100, Some([9; 16]))
        .unwrap();
    assert!(first && !second);
}

#[test]
fn a_relay_serves_only_inside_its_declared_scope_even_for_graphs_it_holds() {
    // The relay holds both graphs, but declares only the board as its scope.
    let tree = Tree::new(source(), &[BOARD, OTHER]);
    let board = graphs_of(&[BOARD]);
    let mut both = request(&tree.master, 0);
    both.graphs = graphs_of(&[BOARD, OTHER]);
    assert!(
        Session::open_relay(&tree.relay, &both, (&board, &[]), "w".into(), identity()).is_err()
    );
    let mut only_board = request(&tree.master, 0);
    only_board.graphs = board.clone();
    assert!(
        Session::open_relay(
            &tree.relay,
            &only_board,
            (&board, &[]),
            "b".into(),
            identity()
        )
        .is_ok()
    );
}

#[test]
fn a_relay_refuses_a_wider_scope_a_non_replica_and_an_unheld_cursor() {
    let tree = Tree::new(source(), &[BOARD]);
    let board = graphs_of(&[BOARD]);
    let mut wider = request(&tree.master, 0);
    wider.graphs = graphs_of(&[BOARD, OTHER]);
    assert!(
        Session::open_relay(&tree.relay, &wider, (&board, &[]), "w".into(), identity()).is_err()
    );
    let plain = request(&tree.master, 0);
    assert!(
        Session::open_relay(&tree.master, &plain, (&board, &[]), "m".into(), identity()).is_err(),
        "a source store is not a replica"
    );
    let ahead = request(&tree.master, tree.master.latest_tx_id().unwrap() + 5);
    assert!(
        Session::open_relay(&tree.relay, &ahead, (&board, &[]), "a".into(), identity()).is_err()
    );
}

#[test]
fn an_upstream_epoch_end_reaches_the_leaf_through_the_relay() {
    // ian's #828 review: the relay took the signed epoch_end as Rebootstrap,
    // but its existing child session kept serving the old lineage, so the leaf
    // never learned the master's history changed.
    let mut tree = Tree::new(source(), &[BOARD]);
    tree.sync(100);
    let packs = tempfile::tempdir().unwrap();
    tree.master
        .freeze_graph(BOARD, packs.path().to_str().unwrap(), T, None)
        .unwrap();
    let (up, _) = tree.upstream.poll(&tree.master, 100, None).unwrap();
    assert!(
        up.iter()
            .any(|f| f.event == event::CONTROL && f.data.contains("epoch_end"))
    );
    let at_relay = feed(&mut tree.relay, &mut tree.relay_applier, &up, 3);
    assert!(
        at_relay
            .iter()
            .any(|o| matches!(o, Outcome::Rebootstrap { .. }))
    );
    let (down, _) = tree.downstream.poll(&tree.relay, 100, None).unwrap();
    assert!(tree.downstream.ended());
    let at_leaf = feed(&mut tree.leaf, &mut tree.leaf_applier, &down, 4);
    assert!(
        at_leaf
            .iter()
            .any(|o| matches!(o, Outcome::Rebootstrap { .. })),
        "frames {down:?}, leaf {at_leaf:?}"
    );
    // A new child cannot open on the ended lineage either.
    let board = vec![BOARD.to_string()];
    let again = request(&tree.master, 0);
    assert!(
        Session::open_relay(&tree.relay, &again, (&board, &[]), "n".into(), identity()).is_err()
    );
}

#[test]
fn a_relay_never_signs_coverage_past_what_upstream_verified() {
    // ian's #828 review: transaction 9 reached the relay but its coverage frame
    // did not yet; the relay forwarded 9 with signed coverage 9 over a
    // watermark it had verified only to 8.
    let mut tree = Tree::new(source(), &[BOARD]);
    tree.sync(100);
    let verified = tree.relay_applier.coverage().unwrap();
    write(
        &mut tree.master,
        BOARD,
        "urn:item:pending",
        "pending coverage",
    );
    let (up, _) = tree.upstream.poll(&tree.master, 100, None).unwrap();
    let tx_only: Vec<Frame> = up
        .iter()
        .filter(|f| f.event != event::COVERAGE)
        .cloned()
        .collect();
    feed(&mut tree.relay, &mut tree.relay_applier, &tx_only, 3);
    assert_eq!(tree.relay_applier.coverage(), Some(verified));
    assert!(tree.relay_applier.progress().unwrap().source_tx > verified);
    let (down, _) = tree.downstream.poll(&tree.relay, 100, None).unwrap();
    feed(&mut tree.leaf, &mut tree.leaf_applier, &down, 4);
    assert!(
        tree.leaf_applier.coverage().unwrap() <= verified,
        "frames {down:?}"
    );
    assert!(
        !in_graph(&tree.leaf, BOARD)
            .iter()
            .any(|v| v.contains("pending coverage"))
    );
    // Once the coverage frame arrives, the transaction flows on.
    let cover: Vec<Frame> = up
        .into_iter()
        .filter(|f| f.event == event::COVERAGE)
        .collect();
    feed(&mut tree.relay, &mut tree.relay_applier, &cover, 5);
    tree.sync(100);
    assert_eq!(in_graph(&tree.leaf, BOARD), in_graph(&tree.master, BOARD));
}

#[test]
fn a_child_cannot_reopen_past_what_the_relay_verified() {
    // ian's #828 SAME: upstream verified to 8, transaction 9 applied without its
    // coverage. A child opening at since = 9 was admitted (the bound used the
    // applied tx) and its first poll signed coverage 9.
    let mut tree = Tree::new(source(), &[BOARD]);
    tree.sync(100);
    let verified = tree.relay_applier.coverage().unwrap();
    write(
        &mut tree.master,
        BOARD,
        "urn:item:uncovered",
        "applied, not yet covered",
    );
    let (up, _) = tree.upstream.poll(&tree.master, 100, None).unwrap();
    let tx_only: Vec<Frame> = up
        .iter()
        .filter(|f| f.event != event::COVERAGE)
        .cloned()
        .collect();
    feed(&mut tree.relay, &mut tree.relay_applier, &tx_only, 3);
    let applied = tree.relay_applier.progress().unwrap().source_tx;
    assert!(applied > verified);
    let board = vec![BOARD.to_string()];
    let ahead = request(&tree.master, applied);
    assert!(
        Session::open_relay(&tree.relay, &ahead, (&board, &[]), "r".into(), identity()).is_err(),
        "a cursor past the verified watermark must be refused"
    );
    // At the verified cursor a child opens, and its coverage never passes it.
    let at = request(&tree.master, verified);
    let (mut child, _) =
        Session::open_relay(&tree.relay, &at, (&board, &[]), "v".into(), identity()).unwrap();
    let (frames, _) = child.poll(&tree.relay, 100, None).unwrap();
    for f in frames.iter().filter(|f| f.event == event::COVERAGE) {
        let w: serde_json::Value = serde_json::from_str(&f.data).unwrap();
        assert!(w["watermark"].as_i64().unwrap() <= verified, "{f:?}");
    }
}

#[test]
fn an_upstream_reconnect_on_a_new_epoch_ends_the_children_too() {
    // ian's #828 SAME: the relay's upstream session ended, it reconnected, and
    // the new session's signed frame named a NEW epoch. The relay took that as
    // Rebootstrap, but nothing recorded the end, so its child kept serving.
    let mut tree = Tree::new(source(), &[BOARD]);
    tree.sync(100);
    let packs = tempfile::tempdir().unwrap();
    // Freezing another graph re-mints the store's epoch; BOARD stays served.
    tree.master
        .freeze_graph(OTHER, packs.path().to_str().unwrap(), T, None)
        .unwrap();
    let cursor = tree.relay_applier.progress().unwrap().source_tx;
    let mut req = request(&tree.master, cursor);
    req.graphs = graphs_of(&[BOARD]);
    let (_, reopened) =
        Session::open(&tree.master, &req, "up2".into(), tree.master_key.clone()).unwrap();
    tree.relay_applier.end_session();
    let at_relay = feed(&mut tree.relay, &mut tree.relay_applier, &[reopened], 5);
    assert!(
        at_relay
            .iter()
            .any(|o| matches!(o, Outcome::Rebootstrap { .. })),
        "{at_relay:?}"
    );
    let (down, _) = tree.downstream.poll(&tree.relay, 100, None).unwrap();
    assert!(tree.downstream.ended(), "frames {down:?}");
    let at_leaf = feed(&mut tree.leaf, &mut tree.leaf_applier, &down, 6);
    assert!(
        at_leaf
            .iter()
            .any(|o| matches!(o, Outcome::Rebootstrap { .. })),
        "frames {down:?}, leaf {at_leaf:?}"
    );
}
