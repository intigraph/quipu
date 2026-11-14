use super::*;

fn event(id: &str, tx: u64, writer: &str, value: i64) -> Observation {
    Observation {
        fluent: "urn:count".into(),
        order: Order {
            event: id.into(),
            tx,
            writer: writer.into(),
        },
        as_of: "2026-01-01T00:00:00Z".into(),
        recorded_on: "2026-01-01T00:00:00Z".into(),
        value: Scalar::Integer(value),
    }
}
fn value(log: &Log, policy: Policy) -> BTreeSet<Scalar> {
    log.current("urn:count", policy).unwrap().unwrap().values
}

#[test]
fn union_is_commutative_associative_idempotent_for_all_policies() {
    for policy in [Policy::Latest, Policy::Max, Policy::Sum, Policy::Set] {
        for a in -2..=2 {
            for b in -2..=2 {
                for c in -2..=2 {
                    let mut left = Log::default();
                    left.append(event("urn:a", 1, "urn:first", a)).unwrap();
                    let mut middle = Log::default();
                    middle.append(event("urn:b", 1, "urn:second", b)).unwrap();
                    let mut right = Log::default();
                    right.append(event("urn:c", 2, "urn:first", c)).unwrap();
                    let mut ab = left.clone();
                    ab.merge(&middle).unwrap();
                    let mut ba = middle.clone();
                    ba.merge(&left).unwrap();
                    assert_eq!(ab, ba);
                    let mut abc = ab.clone();
                    abc.merge(&right).unwrap();
                    let mut bc = middle.clone();
                    bc.merge(&right).unwrap();
                    let mut a_bc = left.clone();
                    a_bc.merge(&bc).unwrap();
                    assert_eq!(abc, a_bc);
                    let old = abc.clone();
                    abc.merge(&old).unwrap();
                    assert_eq!(abc, old);
                    assert_eq!(value(&abc, policy), value(&a_bc, policy));
                }
            }
        }
    }
}

#[test]
fn latest_uses_transaction_then_writer_then_event_not_host_clock() {
    let mut log = Log::default();
    let mut future = event("urn:a", 1, "urn:z", 10);
    future.as_of = "2999-01-01T00:00:00Z".into();
    log.append(future).unwrap();
    log.append(event("urn:b", 2, "urn:a", 20)).unwrap();
    log.append(event("urn:c", 2, "urn:z", 30)).unwrap();
    log.append(event("urn:d", 2, "urn:z", 40)).unwrap();
    assert_eq!(
        value(&log, Policy::Latest),
        BTreeSet::from([Scalar::Integer(40)])
    );
    assert_eq!(
        log.current("urn:count", Policy::Latest)
            .unwrap()
            .unwrap()
            .produced_by,
        BTreeSet::from(["urn:d".into()])
    );
    let intervals = log.intervals("urn:count", Policy::Latest).unwrap();
    assert_eq!(intervals.len(), 4);
    assert!(
        intervals
            .windows(2)
            .all(|w| w[0].until.as_ref() == Some(&w[1].from))
    );
    assert_eq!(intervals.last().unwrap().until, None);
}

#[test]
fn duplicate_delivery_counts_once_but_distinct_events_with_equal_values_count_twice() {
    let mut log = Log::default();
    let first = event("urn:a", 1, "urn:w", 7);
    assert!(log.append(first.clone()).unwrap());
    assert!(!log.append(first).unwrap());
    log.append(event("urn:b", 2, "urn:w", 7)).unwrap();
    assert_eq!(
        value(&log, Policy::Sum),
        BTreeSet::from([Scalar::Integer(14)])
    );
    assert_eq!(
        value(&log, Policy::Set),
        BTreeSet::from([Scalar::Integer(7)])
    );
    assert_eq!(
        log.current("urn:count", Policy::Max)
            .unwrap()
            .unwrap()
            .produced_by
            .len(),
        2
    );
}

#[test]
fn equivocation_refuses_atomically_and_numeric_errors_do_not_truncate_history() {
    let mut log = Log::default();
    log.append(event("urn:a", 1, "urn:w", i64::MAX)).unwrap();
    let before = log.clone();
    let mut other = Log::default();
    other.append(event("urn:0-new", 2, "urn:w", 5)).unwrap();
    other.append(event("urn:a", 1, "urn:w", 9)).unwrap();
    assert!(log.merge(&other).is_err());
    assert_eq!(log, before);
    log.append(event("urn:b", 2, "urn:w", 1)).unwrap();
    assert!(log.current("urn:count", Policy::Sum).is_err());
    assert_eq!(log.observations("urn:count").len(), 2);
    let mut nonnumeric = Log::default();
    let mut o = event("urn:text", 1, "urn:w", 1);
    o.value = Scalar::Term("\"hello\"".into());
    nonnumeric.append(o).unwrap();
    assert!(nonnumeric.current("urn:count", Policy::Max).is_err());
    assert!(nonnumeric.current("urn:count", Policy::Latest).is_ok());
}

#[test]
fn policies_are_explicit_shape_annotations_and_ordinary_slots_are_not_promoted() {
    let prefix = "@prefix sh: <http://www.w3.org/ns/shacl#> . @prefix qp: <https://w3id.org/intigraph/quipu/ns#> .";
    let shapes = format!(
        "{prefix} <urn:shape> sh:path <urn:count>; qp:currentValuePolicy \"sum\" . <urn:status> sh:path <urn:status>; sh:maxCount 1 ."
    );
    assert_eq!(
        declarations(&shapes).unwrap(),
        vec![Declaration {
            shape: "urn:shape".into(),
            predicate: "urn:count".into(),
            policy: Policy::Sum
        }]
    );
    for bad in [
        "\"surprise\"",
        "<urn:latest>",
        "\"sum\", \"max\"",
        "\"sum\"@fr",
        "\"sum\"^^<urn:custom>",
    ] {
        assert!(
            declarations(&format!(
                "{prefix} <urn:s> sh:path <urn:p>; qp:currentValuePolicy {bad} ."
            ))
            .is_err()
        );
    }
    assert!(declarations(&format!("{prefix} <urn:s> qp:currentValuePolicy \"sum\" .")).is_err());
}

#[test]
fn observables_derive_with_source_provenance_and_cycle_registration_rolls_back() {
    let mut registry = registry();
    let observable = |name: &str, input: &str| Observable {
        graph: "urn:graph".into(),
        name: name.into(),
        observes: BTreeSet::from([input.into()]),
        computed_by: Policy::Sum,
    };
    registry
        .register(observable("urn:derived", "urn:count"))
        .unwrap();
    registry
        .register(observable("urn:twice-derived", "urn:derived"))
        .unwrap();
    assert!(
        registry
            .register(observable("urn:count", "urn:twice-derived"))
            .is_err()
    );
    assert!(
        registry
            .register(observable("urn:self", "urn:self"))
            .is_err()
    );
    let mut log = Log::default();
    log.append(event("urn:a", 1, "urn:w", 5)).unwrap();
    log.append(event("urn:b", 2, "urn:w", 6)).unwrap();
    let current = registry.current("urn:twice-derived", &log).unwrap();
    assert_eq!(current.values, BTreeSet::from([Scalar::Integer(11)]));
    assert_eq!(
        current.produced_by,
        BTreeSet::from(["urn:a".into(), "urn:b".into()])
    );
    assert!(
        registry
            .current("urn:twice-derived", &Log::default())
            .is_err()
    );
}

#[test]
fn deserialization_validates_identity_and_latest_observable_keeps_source_order() {
    let mut bad = event("urn:a", 0, "urn:writer", 1);
    assert!(
        serde_json::from_str::<Log>(&serde_json::to_string(&vec![bad.clone()]).unwrap()).is_err()
    );
    bad.order.tx = 1;
    let mut other = bad.clone();
    other.value = Scalar::Integer(2);
    assert!(
        serde_json::from_str::<Log>(&serde_json::to_string(&vec![bad.clone(), other]).unwrap())
            .is_err()
    );
    let log = Log::try_from(vec![bad]).unwrap();
    assert_eq!(
        serde_json::from_str::<Log>(&serde_json::to_string(&log).unwrap()).unwrap(),
        log
    );
    let mut registry = Registry::new("urn:graph").unwrap();
    registry
        .declare("urn:count", binding(Policy::Latest))
        .unwrap();
    registry
        .register(Observable {
            graph: "urn:graph".into(),
            name: "urn:derived".into(),
            observes: BTreeSet::from(["urn:count".into()]),
            computed_by: Policy::Latest,
        })
        .unwrap();
    assert_eq!(
        registry.current("urn:derived", &log).unwrap(),
        log.current("urn:count", Policy::Latest).unwrap().unwrap()
    );
}

#[test]
fn portable_observation_contract_refuses_atomically_and_preserves_rdf_terms() {
    for bad in ["_:blank", "relative", "bad iri", ""] {
        for field in 0..3 {
            let mut observation = event("urn:valid", 1, "urn:writer", 1);
            match field {
                0 => observation.fluent = bad.into(),
                1 => observation.order.event = bad.into(),
                _ => observation.order.writer = bad.into(),
            }
            let mut log = Log::default();
            assert!(log.append(observation).is_err(), "identity {field}: {bad}");
            assert_eq!(log, Log::default());
        }
    }
    for bad in [
        "not RDF",
        "_:blank",
        "<< <urn:s> <urn:p> <urn:o> >>",
        "<urn:o> <urn:extra>",
    ] {
        let mut observation = event("urn:valid", 1, "urn:writer", 1);
        observation.value = Scalar::Term(bad.into());
        assert!(Log::default().append(observation).is_err(), "value {bad}");
    }
    for bad in ["2026-01-01T00:00:00", "2026-02-30T00:00:00Z", "not time"] {
        for recorded in [false, true] {
            let mut observation = event("urn:valid", 1, "urn:writer", 1);
            if recorded {
                observation.recorded_on = bad.into();
            } else {
                observation.as_of = bad.into();
            }
            assert!(Log::default().append(observation).is_err(), "time {bad}");
        }
    }
    for term in [
        "<urn:value>",
        "\"7\"",
        "\"007\"^^<http://www.w3.org/2001/XMLSchema#integer>",
        "\"bonjour\"@fr",
    ] {
        let mut observation = event("urn:valid", 1, "urn:writer", 1);
        observation.as_of = "2026-01-01T01:00:00+01:00".into();
        observation.value = Scalar::Term(term.into());
        let log = Log::try_from(vec![observation.clone()]).unwrap();
        assert_eq!(log.observations("urn:count"), vec![&observation]);
        let restored: Log = serde_json::from_str(&serde_json::to_string(&log).unwrap()).unwrap();
        assert_eq!(restored, log);
    }
}

fn binding(policy: Policy) -> Binding {
    Binding {
        graph: "urn:graph".into(),
        subject: "urn:subject".into(),
        declaration: Declaration {
            shape: "urn:shape".into(),
            predicate: "urn:count".into(),
            policy,
        },
    }
}
fn registry() -> Registry {
    let mut registry = Registry::new("urn:graph").unwrap();
    registry.declare("urn:count", binding(Policy::Sum)).unwrap();
    registry
}
#[test]
fn observable_registration_requires_known_same_graph_inputs_and_immutable_identity() {
    let mut registry = registry();
    let observable = Observable {
        graph: "urn:graph".into(),
        name: "urn:derived".into(),
        observes: BTreeSet::from(["urn:count".into()]),
        computed_by: Policy::Sum,
    };
    for dependency in ["urn:unknown", "urn:foreign"] {
        let mut bad = observable.clone();
        bad.observes = BTreeSet::from([dependency.into()]);
        assert!(registry.register(bad).is_err());
    }
    let mut foreign = observable.clone();
    foreign.graph = "urn:foreign".into();
    assert!(registry.register(foreign).is_err());
    registry.register(observable.clone()).unwrap();
    registry.register(observable.clone()).unwrap();
    let mut changed = observable.clone();
    changed.computed_by = Policy::Max;
    assert!(registry.register(changed).is_err());
    let mut changed = binding(Policy::Max);
    assert!(registry.declare("urn:count", changed.clone()).is_err());
    changed.graph = "urn:foreign".into();
    assert!(registry.declare("urn:foreign", changed).is_err());
    registry.register(observable).unwrap();
    registry.declare("urn:count", binding(Policy::Sum)).unwrap();
}
