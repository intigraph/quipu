//! Ownership changes are real epoch boundaries through the signed source/applier.
use super::tests::{feed, follower, identity, request, source};
use super::*;
use crate::applier::Outcome as Applied;

#[test]
fn ownership_copied_store_cannot_open_a_replication_session() {
    let dir = tempfile::tempdir().unwrap();
    let original = source();
    let copy = dir.path().join("copy.db");
    original
        .conn
        .execute("VACUUM INTO ?1", [copy.to_str().unwrap()])
        .unwrap();
    let copied = Store::open(copy.to_str().unwrap()).unwrap();
    assert!(Session::open(&copied, &request(&copied, 0), "copy".into(), identity()).is_err());
    Store::adopt(copy.to_str().unwrap()).unwrap();
    let adopted = Store::open(copy.to_str().unwrap()).unwrap();
    Session::open(
        &adopted,
        &request(&adopted, 0),
        "adopted".into(),
        identity(),
    )
    .unwrap();
}

#[test]
fn ownership_adopt_emits_signed_epoch_end_and_old_follower_never_splices() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.db");
    source()
        .conn
        .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
        .unwrap();
    Store::adopt(path.to_str().unwrap()).unwrap();
    let src = Store::open(path.to_str().unwrap()).unwrap();
    let id = identity();
    let old = src.log_epoch().unwrap();
    let (mut session, open) =
        Session::open(&src, &request(&src, 0), "old".into(), id.clone()).unwrap();
    let (mut replica, mut applier) = follower(&id, session.scope());
    feed(&mut replica, &mut applier, &[open], 1);
    let (page, _) = session.poll(&src, 100, None).unwrap();
    feed(&mut replica, &mut applier, &page, 1);
    let new = Store::adopt(path.to_str().unwrap()).unwrap();
    assert_ne!(new, old);
    let (end, _) = session.poll(&src, 100, None).unwrap();
    assert!(session.ended());
    assert!(end.iter().any(|frame| frame.data.contains("epoch_end")));
    let outcomes = feed(&mut replica, &mut applier, &end, 2);
    assert!(matches!(outcomes[0], Applied::Rebootstrap { .. }));
    let (fresh, open) = Session::open(&src, &request(&src, 0), "new".into(), id).unwrap();
    assert_ne!(fresh.log_epoch, old);
    // The old applier rejects the new lineage. Only an explicit bootstrap can
    // reset its epoch; it cannot treat the new opening as a continuation.
    let outcome = applier.apply_frame(&mut replica, open.event, &open.data, 3);
    assert!(
        matches!(outcome, Ok(Applied::Rebootstrap { .. })) || outcome.is_err(),
        "{outcome:?}"
    );
}

#[test]
fn ownership_relay_copy_refuses_and_adopting_an_owned_relay_ends_its_session() {
    let src = source();
    let id = identity();
    let (mut upstream, open) =
        Session::open(&src, &request(&src, 0), "up".into(), id.clone()).unwrap();
    let (mut replica, mut applier) = follower(&id, upstream.scope());
    feed(&mut replica, &mut applier, &[open], 1);
    let (frames, _) = upstream.poll(&src, 100, None).unwrap();
    feed(&mut replica, &mut applier, &frames, 1);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("relay.db");
    replica
        .conn
        .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
        .unwrap();
    let copied = Store::open(path.to_str().unwrap()).unwrap();
    let allowed = vec![super::tests::BOARD.to_owned()];
    assert!(
        Session::open_relay(
            &copied,
            &request(&src, 0),
            (&allowed, &[]),
            "bad".into(),
            id.clone()
        )
        .is_err()
    );
    Store::adopt(path.to_str().unwrap()).unwrap();
    let relay = Store::open(path.to_str().unwrap()).unwrap();
    let (mut session, _) = Session::open_relay(
        &relay,
        &request(&src, 0),
        (&allowed, &[]),
        "relay".into(),
        id,
    )
    .unwrap();
    Store::adopt(path.to_str().unwrap()).unwrap();
    let (frames, _) = session.poll(&relay, 100, None).unwrap();
    assert!(session.ended());
    assert!(frames.iter().any(|frame| frame.data.contains("epoch_end")));
}
