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
    Store::adopt(copy.to_str().unwrap()).unwrap();
    std::fs::remove_file(copy.with_extension("db.ownership.json")).unwrap();
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
    Store::adopt(path.to_str().unwrap()).unwrap();
    std::fs::remove_file(path.with_extension("db.ownership.json")).unwrap();
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

#[test]
fn ownership_legacy_stream_requests_remain_explicitly_inactive_without_minting() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy-source.db");
    source()
        .conn
        .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
        .unwrap();
    let src = Store::open(path.to_str().unwrap()).unwrap();
    let epoch = src.log_epoch().unwrap();
    for _ in 0..2 {
        let (mut session, _) =
            Session::open(&src, &request(&src, 0), "legacy".into(), identity()).unwrap();
        assert_eq!(
            session.ownership_state(),
            crate::store::OwnershipState::Inactive
        );
        session.poll(&src, 100, None).unwrap();
        assert_eq!(src.log_epoch().unwrap(), epoch);
        assert!(src.require_ownership().is_err());
    }
}

#[test]
fn ownership_legacy_relay_repeated_requests_remain_inactive() {
    let src = source();
    let id = identity();
    let (mut up, open) = Session::open(&src, &request(&src, 0), "up".into(), id.clone()).unwrap();
    let (mut replica, mut applier) = follower(&id, up.scope());
    feed(&mut replica, &mut applier, &[open], 1);
    let (frames, _) = up.poll(&src, 100, None).unwrap();
    feed(&mut replica, &mut applier, &frames, 1);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy-relay.db");
    replica
        .conn
        .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
        .unwrap();
    let relay = Store::open(path.to_str().unwrap()).unwrap();
    let epoch = relay.log_epoch().unwrap();
    let allowed = vec![super::tests::BOARD.to_owned()];
    for _ in 0..2 {
        let (mut s, _) = Session::open_relay(
            &relay,
            &request(&src, 0),
            (&allowed, &[]),
            "legacy".into(),
            id.clone(),
        )
        .unwrap();
        assert_eq!(s.ownership_state(), crate::store::OwnershipState::Inactive);
        s.poll(&relay, 100, None).unwrap();
        assert_eq!(relay.log_epoch().unwrap(), epoch);
    }
    assert!(relay.require_ownership().is_err());
}

/// Use the actual as-of producer, not a hand-built bootstrap manifest.
fn ownership_bootstrap_frame(store: &Store, dir: &std::path::Path) -> String {
    let cut = crate::asof_share::cut(
        store,
        &crate::asof_share::Request {
            graphs: vec![super::tests::BOARD.into()],
            context_graphs: vec![],
            tx: None,
            expected_store_id: store.store_id().unwrap(),
        },
        &dir.join("spool"),
        &crate::asof_share::Limits {
            disk_floor_bytes: 0,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(cut.parts.parts, 1, "small fixture has one real cut part");
    let part: serde_json::Value =
        serde_json::from_str(std::fs::read_to_string(cut.spool).unwrap().trim()).unwrap();
    serde_json::json!({"manifest": cut.manifest, "records": part["records"]}).to_string()
}

#[test]
fn ownership_stage_b_adopt_and_restore_rebootstrap_then_tail_without_splicing() {
    use crate::applier::{Applier, live_digest};
    use crate::store::Datum;
    use crate::types::{Op, Value};

    // Restore leaves the original session valid; in-place adoption ends it.
    // Both require a new follower lineage at the destination epoch.
    for restore in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.db");
        source()
            .conn
            .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
            .unwrap();
        Store::adopt(path.to_str().unwrap()).unwrap();
        let src = Store::open(path.to_str().unwrap()).unwrap();
        let id = identity();
        let old_epoch = src.log_epoch().unwrap();
        let (mut old_session, open) =
            Session::open(&src, &request(&src, 0), "old".into(), id.clone()).unwrap();
        let (mut old_replica, mut old_applier) = follower(&id, old_session.scope());
        feed(&mut old_replica, &mut old_applier, &[open], 1);
        let (page, _) = old_session.poll(&src, 100, None).unwrap();
        feed(&mut old_replica, &mut old_applier, &page, 1);
        let held_facts = live_digest(&old_replica);
        let held_progress = old_applier.progress().cloned();

        let destination = if restore {
            let target = dir.path().join("restored.db");
            Store::restore(path.to_str().unwrap(), target.to_str().unwrap()).unwrap();
            assert_eq!(src.log_epoch().unwrap(), old_epoch);
            assert!(old_session.poll(&src, 100, None).unwrap().0.is_empty());
            assert!(!old_session.ended());
            target
        } else {
            Store::adopt(path.to_str().unwrap()).unwrap();
            let (end, _) = old_session.poll(&src, 100, None).unwrap();
            assert!(old_session.ended());
            assert!(end.iter().any(|frame| frame.data.contains("epoch_end")));
            assert!(
                feed(&mut old_replica, &mut old_applier, &end, 2)
                    .iter()
                    .any(|outcome| matches!(outcome, Applied::Rebootstrap { .. }))
            );
            path.clone()
        };
        let mut owned = Store::open(destination.to_str().unwrap()).unwrap();
        owned.require_ownership().unwrap();
        assert_ne!(owned.log_epoch().unwrap(), old_epoch);
        assert_eq!(owned.store_id().unwrap(), src.store_id().unwrap());
        let (_, new_open) =
            Session::open(&owned, &request(&owned, 0), "new".into(), id.clone()).unwrap();
        let rejected = old_applier.apply_frame(&mut old_replica, new_open.event, &new_open.data, 3);
        assert!(matches!(rejected, Ok(Applied::Rebootstrap { .. })) || rejected.is_err());
        assert_eq!(live_digest(&old_replica), held_facts);
        assert_eq!(old_applier.progress(), held_progress.as_ref());

        let bootstrap = ownership_bootstrap_frame(&owned, dir.path());
        let refused = old_applier
            .apply_frame(&mut old_replica, "quipu.asof", &bootstrap, 4)
            .unwrap();
        assert!(!matches!(refused, Applied::Bootstrapped { .. }));
        assert_eq!(live_digest(&old_replica), held_facts);
        let mut fresh = Store::open_in_memory().unwrap();
        let mut applier = Applier::open(&fresh).unwrap();
        assert!(matches!(
            applier
                .apply_frame(&mut fresh, "quipu.asof", &bootstrap, 4)
                .unwrap(),
            Applied::Bootstrapped { .. }
        ));
        let progress = applier.progress().unwrap();
        assert_eq!(progress.source.log_epoch, owned.log_epoch().unwrap());
        let (mut tail, open) = Session::open(
            &owned,
            &request(&owned, progress.source_tx),
            "fresh".into(),
            id.clone(),
        )
        .unwrap();
        applier.trust_stream(&id.public_key_hex(), tail.scope());
        feed(&mut fresh, &mut applier, &[open], 5);
        let datum = Datum {
            entity: owned.intern("urn:item:after-ownership").unwrap(),
            attribute: owned.intern("urn:p:title").unwrap(),
            value: Value::Str("new epoch only".into()),
            valid_from: "2026-10-29T00:00:00Z".into(),
            valid_to: None,
            op: Op::Assert,
        };
        let graph = owned.graph_create(super::tests::BOARD).unwrap();
        owned
            .transact_to_graph(&[datum], "2026-10-29T00:00:00Z", None, None, graph)
            .unwrap();
        let (frames, _) = tail.poll(&owned, 100, None).unwrap();
        feed(&mut fresh, &mut applier, &frames, 6);
        assert_eq!(
            super::tests::in_graph(&fresh, super::tests::BOARD),
            super::tests::in_graph(&owned, super::tests::BOARD)
        );
        assert_eq!(applier.coverage(), Some(owned.latest_tx_id().unwrap()));
        assert_eq!(
            live_digest(&old_replica),
            held_facts,
            "old replica never receives the new tail"
        );
    }
}
