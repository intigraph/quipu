use super::*;
use proptest::prelude::*;
use std::collections::{HashSet, VecDeque};

fn placement(name: &str, byte: u8) -> Placement {
    Placement {
        principal: name.into(),
        native: [byte; 32],
    }
}

fn initial() -> Authority {
    Authority {
        store_id: "urn:example:lineage".into(),
        unit: AuthorityUnit::FullStore,
        generation: 0,
        epoch: "epoch-A".into(),
        owner: placement("A", 1),
        policy: "existing-policy-v1".into(),
        predecessor: None,
    }
}

fn certificate(source: Authority, destination: &str) -> Certificate {
    Certificate {
        request: Request {
            operation: format!("op-{destination}-{}", source.generation),
            operator: "existing-operator".into(),
            successor_generation: source.generation + 1,
            source,
            destination: placement(
                destination,
                if destination == "A" {
                    1
                } else if destination == "B" {
                    2
                } else {
                    3
                },
            ),
            profile: "existing-profile-v1".into(),
        },
        cut: Cut {
            transaction: 7,
            facts: [4; 32],
            artifact: [5; 32],
        },
    }
}

fn observed() -> Observations {
    Observations {
        authenticated: true,
        current_grants: true,
        witness_online: true,
    }
}

fn handoff(c: &Certificate, epoch: &str) -> Vec<Action> {
    vec![
        Action::Prepare(c.request.clone()),
        Action::Barrier(c.request.clone()),
        Action::Seal(c.clone()),
        Action::Retire(c.clone()),
        Action::Publish(c.clone()),
        Action::Pending {
            certificate: c.clone(),
            epoch: epoch.into(),
            artifact: c.cut.artifact,
        },
        Action::Install {
            certificate: c.clone(),
            artifact: c.cut.artifact,
        },
        Action::AdvanceWitness(c.clone()),
        Action::Activate {
            certificate: c.clone(),
            epoch: epoch.into(),
            nonce: format!("boot-{epoch}"),
            challenge: format!("boot-{epoch}"),
        },
    ]
}

fn run(model: &mut Model, actions: &[Action]) {
    for action in actions {
        model.apply(action, observed()).unwrap();
    }
}

#[test]
fn retirement_precedes_receipt_and_destination_admission() {
    let a = initial();
    let c = certificate(a.clone(), "B");
    let steps = handoff(&c, "epoch-B");
    let mut model = Model::new(a.clone()).unwrap();
    model.apply(&steps[0], observed()).unwrap();
    assert!(model.can_commit(&a)); // Prepare is not retirement.
    let before = model.clone();
    assert!(
        model
            .apply(&Action::Publish(c.clone()), observed())
            .is_err()
    );
    assert_eq!(model, before);
    model.apply(&steps[1], observed()).unwrap();
    assert!(!model.can_commit(&a)); // The already-open handle is fenced.
    for action in &steps[2..8] {
        model.apply(action, observed()).unwrap();
        assert!(model.writable().is_empty()); // Zero owners is safe.
    }
    let outcome = model.apply(&steps[8], observed()).unwrap();
    let b = c.successor("epoch-B").unwrap();
    assert!(model.can_commit(&b));
    assert!(!model.can_commit(&a));
    assert_eq!(model.writable(), vec![&b]);
    assert_eq!(model.apply(&steps[8], observed()).unwrap(), outcome);
    model
        .apply(&Action::Crash(b.owner.clone()), observed())
        .unwrap();
    // A duplicate receipt readback does NOT recreate a writer after a crash.
    assert_eq!(model.apply(&steps[8], observed()).unwrap(), outcome);
    assert!(!model.can_commit(&b));
}

#[test]
fn every_crash_boundary_keeps_retirement_and_requires_fresh_start() {
    let a = initial();
    let c = certificate(a.clone(), "B");
    let b = c.successor("epoch-B").unwrap();
    let steps = handoff(&c, "epoch-B");
    for prefix in 0..=steps.len() {
        let mut model = Model::new(a.clone()).unwrap();
        run(&mut model, &steps[..prefix]);
        model
            .apply(&Action::Crash(a.owner.clone()), observed())
            .unwrap();
        model
            .apply(&Action::Crash(b.owner.clone()), observed())
            .unwrap();
        assert!(model.writable().is_empty());
        let restart = Action::Restart {
            authority: a.clone(),
            nonce: "fresh-A".into(),
            challenge: "fresh-A".into(),
        };
        assert_eq!(model.apply(&restart, observed()).is_ok(), prefix < 2);
        if prefix >= 2 {
            assert!(!model.can_commit(&a));
        }
        if prefix == steps.len() {
            let restart = Action::Restart {
                authority: b.clone(),
                nonce: "fresh-B".into(),
                challenge: "fresh-B".into(),
            };
            model.apply(&restart, observed()).unwrap();
            assert!(model.can_commit(&b));
        } else if prefix >= 2 {
            // Recovery may inspect/resume the SAME interrupted operation, not unretire.
            run(&mut model, &steps[prefix..]);
            assert!(model.can_commit(&b));
        }
    }
}

#[test]
fn offline_owner_continues_but_no_offline_takeover_or_restart() {
    let a = initial();
    let c = certificate(a.clone(), "B");
    let mut model = Model::new(a.clone()).unwrap();
    let unavailable = Observations {
        witness_online: false,
        ..observed()
    };
    assert!(model.can_commit(&a)); // No per-commit witness/clock dependency.
    assert!(
        model
            .apply(&Action::Prepare(c.request.clone()), unavailable)
            .is_err()
    );
    assert!(
        model
            .apply(&Action::Publish(c.clone()), observed())
            .is_err()
    );
    model
        .apply(&Action::Crash(a.owner.clone()), observed())
        .unwrap();
    let start = Action::Restart {
        authority: a.clone(),
        nonce: "fresh".into(),
        challenge: "fresh".into(),
    };
    assert!(model.apply(&start, unavailable).is_err());
    model.apply(&start, observed()).unwrap();
    assert!(model.can_commit(&a));
    model
        .apply(&Action::Crash(a.owner.clone()), observed())
        .unwrap();
    assert!(model.apply(&start, observed()).is_err()); // Nonce cannot admit twice.
}

#[test]
fn conflicting_requests_skipped_generations_and_changed_retries_refuse() {
    let a = initial();
    let c = certificate(a.clone(), "B");
    let mut model = Model::new(a.clone()).unwrap();
    model
        .apply(&Action::Prepare(c.request.clone()), observed())
        .unwrap();
    let before = model.clone();
    let competitor = certificate(a, "C");
    assert!(
        model
            .apply(&Action::Prepare(competitor.request), observed())
            .is_err()
    );
    let mut changed = c.request.clone();
    changed.profile = "different-profile".into();
    assert!(model.apply(&Action::Prepare(changed), observed()).is_err());
    let mut skipped = c.request.clone();
    skipped.successor_generation += 1;
    assert!(model.apply(&Action::Barrier(skipped), observed()).is_err());
    assert_eq!(model, before);
    run(&mut model, &handoff(&c, "epoch-B")[1..]);
    let before = model.clone();
    let changed = Action::Activate {
        certificate: c,
        epoch: "different-epoch".into(),
        nonce: "other".into(),
        challenge: "other".into(),
    };
    assert!(model.apply(&changed, observed()).is_err());
    assert_eq!(model, before);
}

#[test]
fn full_artifact_policy_profile_and_startup_challenge_are_required() {
    let c = certificate(initial(), "B");
    let steps = handoff(&c, "epoch-B");
    let mut model = Model::new(initial()).unwrap();
    for bad in [
        Observations {
            authenticated: false,
            ..observed()
        },
        Observations {
            current_grants: false,
            ..observed()
        },
    ] {
        assert!(model.apply(&steps[0], bad).is_err());
    }
    run(&mut model, &steps[..5]);
    let wrong = Action::Pending {
        certificate: c.clone(),
        epoch: "epoch-B".into(),
        artifact: [6; 32],
    };
    assert!(model.apply(&wrong, observed()).is_err());
    let mut tampered = c.clone();
    tampered.request.destination.native = [9; 32];
    assert!(
        model
            .apply(
                &Action::Pending {
                    certificate: tampered,
                    epoch: "epoch-B".into(),
                    artifact: c.cut.artifact
                },
                observed()
            )
            .is_err()
    );
    run(&mut model, &steps[5..8]);
    let changed_epoch = Action::Pending {
        certificate: c.clone(),
        epoch: "different-pending-epoch".into(),
        artifact: c.cut.artifact,
    };
    assert!(model.apply(&changed_epoch, observed()).is_err());
    let wrong = Action::Activate {
        certificate: c,
        epoch: "epoch-B".into(),
        nonce: "old".into(),
        challenge: "fresh".into(),
    };
    let before = model.clone();
    assert!(model.apply(&wrong, observed()).is_err());
    assert!(model.apply(&steps[8], Observations::default()).is_err());
    assert_eq!(model, before);
    model.apply(&steps[8], observed()).unwrap();
}

#[test]
fn stale_local_bytes_missing_journal_and_ambiguous_install_do_not_admit() {
    let a = initial();
    let c = certificate(a.clone(), "B");
    let b = c.successor("epoch-B").unwrap();
    let mut model = Model::new(a.clone()).unwrap();
    run(&mut model, &handoff(&c, "epoch-B"));
    model
        .apply(
            &Action::RestoreData {
                placement: a.owner.clone(),
                data: Some(a.clone()),
            },
            observed(),
        )
        .unwrap();
    assert!(!model.can_commit(&a)); // Held old A does not trust the restored DB.
    model
        .apply(&Action::Crash(a.owner.clone()), observed())
        .unwrap();
    model
        .apply(
            &Action::RestoreLocal {
                placement: a.owner.clone(),
                journal: Some(Journal::Active {
                    authority: a.clone(),
                }),
                data: Some(a.clone()),
            },
            observed(),
        )
        .unwrap();
    assert!(
        model
            .apply(
                &Action::Restart {
                    authority: a,
                    nonce: "fresh-old-A".into(),
                    challenge: "fresh-old-A".into()
                },
                observed()
            )
            .is_err()
    );
    assert!(model.can_commit(&b)); // Old full-host bytes cannot beat the current witness.
    model
        .apply(&Action::Crash(b.owner.clone()), observed())
        .unwrap();
    model
        .apply(
            &Action::RestoreLocal {
                placement: b.owner.clone(),
                journal: None,
                data: Some(b.clone()),
            },
            observed(),
        )
        .unwrap();
    assert!(
        model
            .apply(
                &Action::Restart {
                    authority: b,
                    nonce: "fresh-B".into(),
                    challenge: "fresh-B".into()
                },
                observed()
            )
            .is_err()
    );
    assert!(model.writable().is_empty()); // No legacy fallback for an enrolled unit.
}

#[test]
fn return_to_old_placement_requires_new_handoff_epoch_and_generation() {
    let mut model = Model::new(initial()).unwrap();
    let first = certificate(initial(), "B");
    run(&mut model, &handoff(&first, "epoch-B"));
    let second = certificate(first.successor("epoch-B").unwrap(), "A");
    let steps = handoff(&second, "epoch-A-2");
    run(&mut model, &steps[..5]);
    assert!(
        model
            .apply(
                &Action::Pending {
                    certificate: second.clone(),
                    epoch: initial().epoch,
                    artifact: second.cut.artifact
                },
                observed()
            )
            .is_err()
    );
    run(&mut model, &steps[5..]);
    let a2 = second.successor("epoch-A-2").unwrap();
    assert!(model.can_commit(&a2));
    assert!(!model.can_commit(&initial()));
    assert_eq!(a2.generation, 2);
    assert_eq!(a2.predecessor, Some(second.digest().unwrap()));
    let mut exhausted = initial();
    exhausted.generation = u64::MAX;
    exhausted.predecessor = Some([8; 32]);
    assert!(Model::new(exhausted.clone()).is_err());
    let mut invalid = first.request;
    invalid.source = exhausted;
    invalid.successor_generation = 0;
    assert!(invalid.validate().is_err());
}

fn exploration_actions() -> Vec<Action> {
    let mut actions = Vec::new();
    for destination in ["B", "C"] {
        let c = certificate(initial(), destination);
        actions.extend(handoff(&c, &format!("epoch-{destination}")));
        let successor = c.successor(&format!("epoch-{destination}")).unwrap();
        actions.push(Action::Crash(successor.owner.clone()));
        actions.push(Action::Restart {
            authority: successor,
            nonce: format!("restart-{destination}"),
            challenge: format!("restart-{destination}"),
        });
    }
    actions.push(Action::Crash(initial().owner));
    actions.push(Action::Restart {
        authority: initial(),
        nonce: "restart-A".into(),
        challenge: "restart-A".into(),
    });
    actions
}

#[test]
fn bounded_enumeration_never_admits_two_writers() {
    let initial = Model::new(initial()).unwrap();
    let mut seen = HashSet::from([initial.clone()]);
    let mut queue = VecDeque::from([(initial, Vec::<usize>::new())]);
    let actions = exploration_actions();
    while let Some((model, trace)) = queue.pop_front() {
        assert!(
            model.writable().len() <= 1,
            "split owner; action indices {trace:?}"
        );
        if trace.len() == 12 {
            continue;
        }
        for (index, action) in actions.iter().enumerate() {
            let mut next = model.clone();
            let before = next.clone();
            match next.apply(action, observed()) {
                Err(_) => assert_eq!(next, before, "refusal changed state"),
                Ok(_) if seen.insert(next.clone()) => {
                    let mut path = trace.clone();
                    path.push(index);
                    queue.push_back((next, path));
                }
                Ok(_) => {}
            }
        }
    }
    eprintln!(
        "MODEL enumerated {} distinct states, 24 actions, depth 12",
        seen.len()
    );
    assert!(
        seen.len() > 50,
        "explorer must exercise handoff and crash states"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn arbitrary_action_orders_preserve_exclusivity_and_refusal_atomicity(indices in prop::collection::vec(0usize..24,0..80)) {
        let mut model=Model::new(initial()).unwrap();let actions=exploration_actions();
        prop_assert_eq!(actions.len(),24);
        for index in indices {
            let before=model.clone();
            if model.apply(&actions[index],observed()).is_err() {prop_assert_eq!(&model,&before);}
            prop_assert!(model.writable().len()<=1);
        }
    }

    #[test]
    fn record_roundtrip_and_digest_ignore_json_key_order(id in "[a-zA-Z0-9_:-]{1,32}", generation in 0u64..u64::MAX, kind in 0usize..8) {
        let mut a=initial();a.store_id=id;a.generation=generation;
        a.predecessor=if generation==0 {None} else {Some([7;32])};
        let c=certificate(a.clone(),"B");let successor=c.successor("epoch-B").unwrap();
        let payload=match kind {
            0=>Payload::Journal{journal:Journal::Active{authority:a}},
            1=>Payload::Journal{journal:Journal::Retiring{request:c.request.clone()}},
            2=>Payload::Journal{journal:Journal::Retired{certificate:c.clone()}},
            3=>Payload::Journal{journal:Journal::Pending{certificate:c.clone(),successor:successor.clone()}},
            4=>Payload::Operation{request:c.request.clone()},
            5=>Payload::Retirement{certificate:c.clone()},
            6=>Payload::Activation{activation:Activation{certificate:c.clone(),authority:successor}},
            _=>Payload::Startup{authority:successor,nonce:"fresh-challenge".into()},
        };
        let record=Record{version:RECORD_VERSION,payload};
        let bytes=record.encode().unwrap();
        prop_assert_eq!(Record::decode(&bytes).unwrap(),record.clone());
        let reordered=serde_json::to_vec(&serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()).unwrap();
        prop_assert_eq!(Record::decode(&reordered).unwrap().encode().unwrap(),bytes);
        let roundtrip:Request=serde_json::from_value(serde_json::to_value(&c.request).unwrap()).unwrap();
        prop_assert_eq!(roundtrip.digest().unwrap(),c.request.digest().unwrap());
    }
}

#[test]
fn strict_schema_rejects_unknown_missing_duplicate_oversized_and_inconsistent_records() {
    let c = certificate(initial(), "B");
    let record = Record {
        version: 1,
        payload: Payload::Retirement { certificate: c },
    };
    let value = serde_json::to_value(&record).unwrap();
    for path in [
        vec!["version"],
        vec!["payload", "certificate", "request", "profile"],
        vec!["payload", "certificate", "cut", "facts"],
    ] {
        let mut missing = value.clone();
        let mut parent = &mut missing;
        for key in &path[..path.len() - 1] {
            parent = &mut parent[*key];
        }
        parent.as_object_mut().unwrap().remove(path[path.len() - 1]);
        assert!(Record::decode(&serde_json::to_vec(&missing).unwrap()).is_err());
    }
    let mut bad = value.clone();
    bad["version"] = 2.into();
    assert!(Record::decode(&serde_json::to_vec(&bad).unwrap()).is_err());
    let mut bad = value.clone();
    bad["extra"] = true.into();
    assert!(Record::decode(&serde_json::to_vec(&bad).unwrap()).is_err());
    let mut bad = value.clone();
    bad["payload"]["certificate"]["request"]["source"]["unit"] = "SCOPED_GRAPH".into();
    assert!(Record::decode(&serde_json::to_vec(&bad).unwrap()).is_err());
    let mut bad = value.clone();
    bad["payload"]["certificate"]["request"]["successor_generation"] = 3.into();
    assert!(Record::decode(&serde_json::to_vec(&bad).unwrap()).is_err());
    let bytes = record.encode().unwrap();
    let duplicate = [b"{\"version\":1,".as_slice(), &bytes[1..]].concat();
    assert!(Record::decode(&duplicate).is_err());
    assert!(Record::decode(&vec![b' '; MAX_RECORD_BYTES + 1]).is_err());
    let mut c = certificate(initial(), "B");
    c.request.profile = "\nunknown".into();
    assert!(c.validate().is_err());
}
