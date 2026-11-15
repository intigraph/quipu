use super::*;
use crate::fluent::{Policy, Scalar};
use crate::transaction_auth::{Identity, with_identity};
use std::collections::BTreeSet;

fn identity() -> Identity {
    Identity {
        principal: "urn:writer".into(),
        credential_id: Some("fixture".into()),
        auth_class: "named_bearer".into(),
        attestation: Default::default(),
    }
}

fn append(name: &str, event: &str, value: i64) -> writer::Append {
    writer::Append {
        fluent: name.into(),
        event: event.into(),
        subject: "urn:subject".into(),
        predicate: "urn:count".into(),
        property_shape: "urn:base-shape".into(),
        as_of: "2026-11-08T00:00:00Z".into(),
        value: Scalar::Integer(value),
    }
}

fn loaded(policy: &str) -> Store {
    let mut store = Store::open_in_memory().unwrap();
    store
        .load_shapes(
            "quipu-fluents-v1",
            include_str!("../../shapes/fluents-v1.ttl"),
            "2026-11-08T00:00:00Z",
        )
        .unwrap();
    store.load_shapes("declarations", &format!(
        "@prefix sh: <http://www.w3.org/ns/shacl#> . @prefix qp: <https://w3id.org/intigraph/quipu/ns#> . \
         <urn:base-shape> sh:path <urn:count>; sh:targetNode <urn:subject>; qp:currentValuePolicy \"{policy}\" . \
         <urn:output-shape> sh:path <urn:aggregate>; sh:targetNode <urn:subject>; qp:currentValuePolicy \"sum\" ."
    ), "2026-11-08T00:00:00Z").unwrap();
    with_identity(Some(identity()), || {
        store.append_fluent_native(&append("urn:base", "urn:first", 3), 0)
    })
    .unwrap();
    store
}

fn request(name: &str, dependencies: &[&str]) -> Register {
    Register {
        observable: Observable {
            graph: crate::schema::ROOT_GRAPH_IRI.into(),
            name: name.into(),
            observes: dependencies.iter().map(|s| (*s).into()).collect(),
            computed_by: Policy::Sum,
        },
        subject: "urn:subject".into(),
        predicate: "urn:aggregate".into(),
        property_shape: "urn:output-shape".into(),
    }
}

#[test]
fn native_observable_replay_restart_and_new_inputs_keep_original_provenance() {
    let mut store = loaded("sum");
    let registration = request("urn:derived", &["urn:base"]);
    let tx = with_identity(Some(identity()), || {
        store.register_observable_native(&registration, 0)
    })
    .unwrap();
    assert_eq!(store.transaction_auth(tx).unwrap(), Some(identity()));
    let current = store.fluent_current_native(0, "urn:derived").unwrap();
    assert_eq!(current.values, BTreeSet::from([Scalar::Integer(3)]));
    assert_eq!(current.produced_by, BTreeSet::from(["urn:first".into()]));
    assert_eq!(
        with_identity(Some(identity()), || store
            .register_observable_native(&registration, 0))
        .unwrap(),
        tx
    );
    assert_eq!(store.latest_tx_id().unwrap(), tx);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("observable.db");
    std::fs::write(&path, store.serialize_db().unwrap()).unwrap();
    drop(store);
    let mut store = Store::open(path.to_str().unwrap()).unwrap();
    assert_eq!(
        store.fluent_current_native(0, "urn:derived").unwrap(),
        current
    );
    with_identity(Some(identity()), || {
        store.append_fluent_native(&append("urn:base", "urn:second", 5), 0)
    })
    .unwrap();
    let current = store.fluent_current_native(0, "urn:derived").unwrap();
    assert_eq!(current.values, BTreeSet::from([Scalar::Integer(8)]));
    assert_eq!(
        current.produced_by,
        BTreeSet::from(["urn:first".into(), "urn:second".into()])
    );
}

#[test]
fn native_observable_refuses_unknown_self_cycle_foreign_graph_and_policy_atomically() {
    let mut store = loaded("sum");
    let before = store.current_facts().unwrap();
    let tx = store.latest_tx_id().unwrap();
    assert!(
        store
            .register_observable_native(&request("urn:no-auth", &["urn:base"]), 0)
            .is_err()
    );
    let mut foreign = request("urn:foreign", &["urn:base"]);
    foreign.observable.graph = "urn:another-graph".into();
    let mut policy = request("urn:wrong-policy", &["urn:base"]);
    policy.observable.computed_by = Policy::Latest;
    for bad in [
        request("urn:self", &["urn:self"]),
        request("urn:unknown", &["urn:missing"]),
        foreign,
        policy,
    ] {
        assert!(
            with_identity(Some(identity()), || store
                .register_observable_native(&bad, 0))
            .is_err()
        );
        assert!(store.lookup(&bad.observable.name).unwrap().is_none());
        assert_eq!(store.latest_tx_id().unwrap(), tx);
        assert_eq!(store.current_facts().unwrap(), before);
    }
}

#[test]
fn native_observable_transitive_rebinding_and_direct_append_cannot_mutate_configuration() {
    let mut store = loaded("sum");
    with_identity(Some(identity()), || {
        store
            .register_observable_native(&request("urn:a", &["urn:base"]), 0)
            .unwrap();
        store
            .register_observable_native(&request("urn:b", &["urn:a"]), 0)
            .unwrap();
    });
    let before = store.current_facts().unwrap();
    let tx = store.latest_tx_id().unwrap();
    assert!(
        with_identity(Some(identity()), || store
            .register_observable_native(&request("urn:a", &["urn:b"]), 0))
        .is_err()
    );
    let mut append = append("urn:a", "urn:illegal-direct-event", 2);
    append.predicate = "urn:aggregate".into();
    append.property_shape = "urn:output-shape".into();
    assert!(with_identity(Some(identity()), || store.append_fluent_native(&append, 0)).is_err());
    let id = store.lookup("urn:a").unwrap().unwrap();
    assert!(
        store
            .conn
            .execute("DELETE FROM facts WHERE e=?1", [id])
            .is_err()
    );
    assert_eq!(store.latest_tx_id().unwrap(), tx);
    assert_eq!(store.current_facts().unwrap(), before);
    assert_eq!(
        store.fluent_current_native(0, "urn:b").unwrap().values,
        BTreeSet::from([Scalar::Integer(3)])
    );
}

#[test]
fn native_observable_numeric_incompatibility_and_overflow_refuse_before_registration() {
    for policy in ["latest", "sum"] {
        let mut store = loaded(policy);
        let mut registration = request("urn:bad-aggregate", &["urn:base"]);
        if policy == "sum" {
            with_identity(Some(identity()), || {
                store.append_fluent_native(&append("urn:large", "urn:large-event", i64::MAX), 0)
            })
            .unwrap();
            registration.observable.observes.insert("urn:large".into());
        }
        let tx = store.latest_tx_id().unwrap();
        let before = store.current_facts().unwrap();
        assert!(
            with_identity(Some(identity()), || store
                .register_observable_native(&registration, 0))
            .is_err()
        );
        assert!(store.lookup("urn:bad-aggregate").unwrap().is_none());
        assert_eq!(store.latest_tx_id().unwrap(), tx);
        assert_eq!(store.current_facts().unwrap(), before);
    }
}

#[test]
fn native_current_projection_is_read_only_and_reifies_aggregate_with_source_events() {
    let mut store = loaded("sum");
    with_identity(Some(identity()), || {
        store.append_fluent_native(&append("urn:base", "urn:second", 4), 0)
    })
    .unwrap();
    let before = store.current_facts().unwrap();
    let tx = store.latest_tx_id().unwrap();
    let terms: i64 = store
        .conn
        .query_row("SELECT count(*) FROM terms", [], |r| r.get(0))
        .unwrap();
    let projection = store.fluent_projection_native(0, "urn:base").unwrap();
    assert_eq!(projection.graph, crate::schema::ROOT_GRAPH_IRI);
    assert_eq!(projection.statements.len(), 3);
    let oxrdf::Term::Triple(inner) = &projection.statements[0].object else {
        panic!("projection must reify a first-class triple term")
    };
    assert_eq!(inner.subject.to_string(), "<urn:subject>");
    assert_eq!(inner.predicate.as_str(), "urn:count");
    assert_eq!(
        inner.object,
        oxrdf::Term::Literal(oxrdf::Literal::new_typed_literal(
            "7",
            oxrdf::vocab::xsd::INTEGER
        ))
    );
    let sources: BTreeSet<_> = projection.statements[1..]
        .iter()
        .map(|t| t.object.to_string())
        .collect();
    assert_eq!(
        sources,
        BTreeSet::from(["<urn:first>".into(), "<urn:second>".into()])
    );
    assert_eq!(
        store.fluent_projection_native(0, "urn:base").unwrap(),
        projection
    );
    assert!(store.fluent_projection_native(0, "urn:missing").is_err());
    assert_eq!(store.current_facts().unwrap(), before);
    assert_eq!(store.latest_tx_id().unwrap(), tx);
    let after: i64 = store
        .conn
        .query_row("SELECT count(*) FROM terms", [], |r| r.get(0))
        .unwrap();
    assert_eq!(after, terms);
}

#[test]
fn authenticated_source_label_and_untyped_fields_do_not_admit_a_fluent_event() {
    let mut store = loaded("sum");
    let before = store.fluent_current_native(0, "urn:base").unwrap();
    let next = store.latest_tx_id().unwrap() + 1;
    let qp = crate::namespace::QUIPU_FLUENT;
    let at = "2026-11-08T01:00:00Z";
    let fields = [
        ("forFluent", writer::iri("urn:base").unwrap()),
        (
            "originTx",
            oxrdf::Literal::new_typed_literal(next.to_string(), oxrdf::vocab::xsd::INTEGER).into(),
        ),
        ("writer", writer::iri("urn:writer").unwrap()),
        (
            "asOf",
            oxrdf::Literal::new_typed_literal(at, oxrdf::vocab::xsd::DATE_TIME).into(),
        ),
        (
            "recordedOn",
            oxrdf::Literal::new_typed_literal(at, oxrdf::vocab::xsd::DATE_TIME).into(),
        ),
        (
            "observationValue",
            oxrdf::Literal::new_typed_literal("999", oxrdf::vocab::xsd::INTEGER).into(),
        ),
    ];
    let datums: Vec<_> = fields
        .into_iter()
        .map(|(p, v)| {
            writer::datum(&store, "urn:untyped-forgery", &format!("{qp}{p}"), &v, at).unwrap()
        })
        .collect();
    // Ordinary untyped metadata remains ordinary, even with real authentication
    // and a caller-selected native-looking source label and transaction number.
    let tx = with_identity(Some(identity()), || {
        store.transact(&datums, at, None, Some("quipu:fluent"))
    })
    .unwrap();
    assert_eq!(tx, next);
    assert_eq!(store.fluent_current_native(0, "urn:base").unwrap(), before);
    let promote = writer::datum(
        &store,
        "urn:untyped-forgery",
        crate::namespace::RDF_TYPE,
        &writer::iri(&format!("{qp}FluentObservation")).unwrap(),
        at,
    )
    .unwrap();
    assert!(
        with_identity(Some(identity()), || store.transact(
            &[promote],
            at,
            None,
            Some("quipu:fluent")
        ))
        .is_err()
    );
    assert!(
        with_identity(Some(identity()), || store.append_fluent_native(
            &append("urn:base", "urn:untyped-forgery", 999),
            0
        ))
        .is_err()
    );
    assert_eq!(store.latest_tx_id().unwrap(), tx);
}
