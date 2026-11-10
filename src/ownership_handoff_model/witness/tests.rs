use super::*;
use crate::ownership_handoff_model::{Action, AuthorityUnit, Cut, Model, Observations, Placement};

fn initial() -> Authority {
    Authority {
        store_id: "urn:example:witness".into(),
        unit: AuthorityUnit::FullStore,
        generation: 0,
        epoch: "epoch-A".into(),
        owner: placement("A", 1),
        policy: "existing-policy".into(),
        predecessor: None,
    }
}
fn placement(name: &str, byte: u8) -> Placement {
    Placement {
        principal: name.into(),
        native: [byte; 32],
    }
}
fn certificate(source: Authority, destination: &str, op: &str) -> Certificate {
    Certificate {
        request: Request {
            operation: op.into(),
            operator: "existing-operator".into(),
            successor_generation: source.generation + 1,
            source,
            destination: placement(destination, 2),
            profile: "existing-profile".into(),
        },
        cut: Cut {
            transaction: 1,
            facts: [3; 32],
            artifact: [4; 32],
        },
    }
}
fn witness() -> InMemoryWitness {
    let Reply::Ok(w) = InMemoryWitness::new(initial()) else {
        panic!("genesis positive control");
    };
    w
}
fn active() -> Journal {
    Journal::Active {
        authority: initial(),
    }
}

#[test]
fn phases_exact_readback_and_exclusive_outcome() {
    let w = witness();
    let c = certificate(initial(), "B", "op1");
    assert_eq!(
        w.head(&initial().store_id, 0),
        Reply::Ok(WitnessHead::Active {
            authority: initial()
        })
    );
    assert_eq!(w.reserve(0, &c.request), Reply::Ok(()));
    assert_eq!(w.reserve(0, &c.request), Reply::Ok(()));
    assert_eq!(
        w.head(&initial().store_id, 0),
        Reply::Ok(WitnessHead::Reserved {
            request: c.request.clone()
        })
    );
    let a = Abandonment {
        request: c.request.clone(),
        reason: "cancel before barrier".into(),
    };
    assert_eq!(w.abandon(0, &a, &active(), true), Reply::Conflict);
    assert_eq!(
        w.abandon(
            0,
            &a,
            &Journal::Retiring {
                request: c.request.clone()
            },
            false
        ),
        Reply::Conflict
    );
    assert_eq!(w.retire(0, &c), Reply::Ok(()));
    assert_eq!(w.abandon(0, &a, &active(), false), Reply::Conflict);
    let b = c.successor("epoch-B").unwrap();
    assert_eq!(
        w.head(&initial().store_id, 0),
        Reply::Ok(WitnessHead::Retired {
            certificate: c.clone()
        })
    );
    assert_eq!(w.activate(&b), Reply::Ok(()));
    assert_eq!(w.activate(&b), Reply::Ok(()));
    assert_eq!(w.retire(0, &c), Reply::Ok(())); // Historical retry does not rewind.
    for anchor in [0, 1] {
        assert_eq!(
            w.head(&b.store_id, anchor),
            Reply::Ok(WitnessHead::Active {
                authority: b.clone()
            })
        );
    }
    assert_eq!(w.head(&b.store_id, 2), Reply::Unknown);
    let mut changed = b;
    changed.epoch = "different-epoch".into();
    assert_eq!(w.activate(&changed), Reply::Conflict);
}

#[test]
fn abandon_keeps_source_and_requires_new_attempt_and_operation() {
    let w = witness();
    let c = certificate(initial(), "B", "op1");
    let a = Abandonment {
        request: c.request.clone(),
        reason: "cancel before barrier".into(),
    };
    assert_eq!(w.reserve(0, &c.request), Reply::Ok(()));
    assert_eq!(w.abandon(0, &a, &active(), false), Reply::Ok(()));
    assert_eq!(w.retire(0, &c), Reply::Conflict);
    assert_eq!(
        w.activate(&c.successor("stale-B").unwrap()),
        Reply::Conflict
    );
    assert_eq!(w.reserve(1, &c.request), Reply::Conflict); // Operation reuse.
    let next = certificate(initial(), "C", "op2");
    assert_eq!(w.reserve(2, &next.request), Reply::Conflict); // Missing attempt.
    assert_eq!(w.reserve(1, &next.request), Reply::Ok(()));
    assert_eq!(w.abandon(0, &a, &active(), true), Reply::Ok(())); // Exact historical readback only.
    assert_eq!(
        w.head(&initial().store_id, 0),
        Reply::Ok(WitnessHead::Reserved {
            request: next.request.clone()
        })
    );
    let mut changed = a;
    changed.reason = "changed reason".into();
    assert_eq!(w.abandon(0, &changed, &active(), false), Reply::Conflict);
}

#[test]
fn unknown_slow_and_lost_reply_are_distinct_from_conflict() {
    let w = witness();
    let c = certificate(initial(), "B", "op1");
    for fault in [Fault::Unknown, Fault::Slow] {
        assert_eq!(w.inject(fault), Reply::Ok(()));
        assert_eq!(w.reserve(0, &c.request), Reply::Unknown);
        assert_eq!(
            w.head(&initial().store_id, 0),
            Reply::Ok(WitnessHead::Active {
                authority: initial()
            })
        );
    }
    assert_eq!(w.inject(Fault::Conflict), Reply::Ok(()));
    assert_eq!(w.reserve(0, &c.request), Reply::Conflict);
    assert_eq!(w.inject(Fault::LostReply), Reply::Ok(()));
    assert_eq!(w.reserve(0, &c.request), Reply::Unknown);
    assert_eq!(w.reserve(0, &c.request), Reply::Ok(()));
    let other = certificate(initial(), "C", "op2");
    assert_eq!(w.reserve(0, &other.request), Reply::Conflict);
    let lock = w.backend.0.lock().unwrap();
    assert_eq!(w.head(&initial().store_id, 0), Reply::Unknown);
    drop(lock);
}

#[test]
fn concurrent_different_reservers_have_exactly_one_winner() {
    let w = witness();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let handles: Vec<_> = ["B", "C"]
        .into_iter()
        .map(|destination| {
            let w = w.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let c = certificate(initial(), destination, destination);
                barrier.wait();
                w.reserve(0, &c.request)
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| **r == Reply::Ok(())).count(), 1);
    assert!(
        results
            .iter()
            .all(|r| matches!(r, Reply::Ok(()) | Reply::Conflict | Reply::Unknown))
    );
}

#[test]
fn stale_skipped_and_malformed_inputs_never_create_records() {
    let w = witness();
    let c = certificate(initial(), "B", "op1");
    let mut r = c.request.clone();
    r.successor_generation = 3;
    assert_eq!(w.reserve(0, &r), Reply::Conflict);
    r = c.request.clone();
    r.source.epoch = "stale".into();
    assert_eq!(w.reserve(0, &r), Reply::Conflict);
    r = c.request.clone();
    r.destination = r.source.owner.clone();
    assert_eq!(w.reserve(0, &r), Reply::Conflict);
    assert_eq!(w.retire(0, &c), Reply::Conflict);
    assert_eq!(
        w.activate(&c.successor("epoch-B").unwrap()),
        Reply::Conflict
    );
    assert_eq!(w.head("wrong-store", 0), Reply::Conflict);
    assert_eq!(w.backend.0.lock().unwrap().entries.len(), 1);
}

#[test]
fn tampered_missing_and_unexpected_keys_are_unknown_with_positive_control() {
    for variant in 0..5 {
        let w = witness();
        let c = certificate(initial(), "B", "op1");
        assert_eq!(w.reserve(0, &c.request), Reply::Ok(()));
        assert_eq!(
            w.head(&initial().store_id, 0),
            Reply::Ok(WitnessHead::Reserved {
                request: c.request.clone()
            })
        );
        {
            let mut m = w.backend.0.lock().unwrap();
            match variant {
                0 => {
                    m.entries.insert(
                        Key::Active(2),
                        Entry::Active(c.successor("epoch-B").unwrap())
                            .bytes()
                            .unwrap(),
                    );
                }
                1 => {
                    m.entries.insert(
                        Key::Reserved(0, 1),
                        Entry::Reserved(c.request.clone()).bytes().unwrap(),
                    );
                }
                2 => {
                    m.entries.remove(&Key::Reserved(0, 0));
                    m.entries.insert(
                        Key::Outcome(0, 0),
                        Entry::Retired(c.clone()).bytes().unwrap(),
                    );
                }
                3 => {
                    m.entries
                        .insert(Key::Reserved(0, 0), b"{malformed".to_vec());
                }
                _ => {
                    m.read_budget = 1;
                }
            }
        }
        assert_eq!(
            w.head(&initial().store_id, 0),
            Reply::Unknown,
            "variant {variant}"
        );
    }
}

// Compose ACTUAL model transitions and ACTUAL witness operations. Only commit
// the local candidate after an unambiguous witness result. No mock validators.
fn apply(model: &mut Model, w: &InMemoryWitness, action: &Action) -> Result<(), ()> {
    let mut candidate = model.clone();
    candidate
        .apply(
            action,
            Observations {
                authenticated: true,
                current_grants: true,
                witness_online: true,
            },
        )
        .map_err(|_| ())?;
    let reply = match action {
        Action::Prepare(r) => w.reserve(0, r),
        Action::FenceWitness(c) => w.retire(0, c),
        Action::AdvanceWitness(c) => {
            let WitnessHead::Active { authority } = candidate.witness_head() else {
                return Err(());
            };
            assert_eq!(authority.owner, c.request.destination);
            w.activate(authority)
        }
        _ => Reply::Ok(()),
    };
    if reply != Reply::Ok(()) {
        return Err(());
    }
    assert_eq!(
        w.head(&initial().store_id, 0),
        Reply::Ok(candidate.witness_head().clone())
    );
    *model = candidate;
    Ok(())
}

#[test]
fn composed_enumeration_rejects_split_owners_stale_generation_and_nonce_reuse() {
    let c = certificate(initial(), "B", "op1");
    let b = c.successor("epoch-B").unwrap();
    let actions = vec![
        Action::Prepare(c.request.clone()),
        Action::Barrier(c.request.clone()),
        Action::Seal(c.clone()),
        Action::Retire(c.clone()),
        Action::FenceWitness(c.clone()),
        Action::Publish(c.clone()),
        Action::Pending {
            certificate: c.clone(),
            epoch: b.epoch.clone(),
            artifact: c.cut.artifact,
        },
        Action::Install {
            certificate: c.clone(),
            artifact: c.cut.artifact,
        },
        Action::AdvanceWitness(c.clone()),
        Action::Activate {
            certificate: c.clone(),
            epoch: b.epoch.clone(),
            nonce: "fresh-B".into(),
            challenge: "fresh-B".into(),
        },
    ];
    let mut model = Model::new(initial()).unwrap();
    let w = witness();
    for action in &actions {
        apply(&mut model, &w, action).unwrap();
        assert!(model.writable().len() <= 1);
    }
    assert_eq!(model.writable(), vec![&b]);
    let reused = Action::Restart {
        authority: b,
        nonce: "fresh-B".into(),
        challenge: "fresh-B".into(),
    };
    assert!(apply(&mut model, &w, &reused).is_err());
    // Enumerate every subset of the 10 exact transition boundaries in order.
    // A fresh model/witness pair for each path, including all skipped steps.
    for mask in 0..(1_u16 << actions.len()) {
        let mut model = Model::new(initial()).unwrap();
        let w = witness();
        for (i, action) in actions.iter().enumerate() {
            if mask & (1 << i) != 0 {
                let _ = apply(&mut model, &w, action);
            }
            assert!(model.writable().len() <= 1, "mask={mask}");
        }
    }
}

#[test]
fn lost_reply_does_not_advance_local_model_until_exact_readback() {
    let w = witness();
    let mut model = Model::new(initial()).unwrap();
    let c = certificate(initial(), "B", "op1");
    let action = Action::Prepare(c.request.clone());
    let before = model.clone();
    assert_eq!(w.inject(Fault::LostReply), Reply::Ok(()));
    assert!(apply(&mut model, &w, &action).is_err());
    assert_eq!(model, before);
    apply(&mut model, &w, &action).unwrap();
    assert_eq!(
        model.witness_head(),
        &WitnessHead::Reserved { request: c.request }
    );
}
