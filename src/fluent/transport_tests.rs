use super::*;
use crate::transaction_auth::{Identity, with_identity};

fn identity() -> Identity {
    Identity {
        principal: "urn:writer:transport-test".into(),
        credential_id: Some("transport-test".into()),
        auth_class: "named_bearer".into(),
        attestation: Default::default(),
    }
}

fn loaded() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    store
        .load_shapes(
            "quipu-fluents-v1",
            include_str!("../../shapes/fluents-v1.ttl"),
            "2026-11-12T00:00:00Z",
        )
        .unwrap();
    store
        .load_shapes(
            "transport-declaration",
            &format!(
                "<urn:count-shape> <http://www.w3.org/ns/shacl#path> <urn:count>; \
        <http://www.w3.org/ns/shacl#targetNode> <urn:subject>; \
        <{}currentValuePolicy> \"sum\" .",
                crate::namespace::QUIPU_FLUENT
            ),
            "2026-11-12T00:00:00Z",
        )
        .unwrap();
    store
}

fn request() -> AppendRequest {
    AppendRequest {
        graph: crate::schema::ROOT_GRAPH_IRI.into(),
        fluent: "urn:fluent:transport".into(),
        event: "urn:event:transport".into(),
        subject: "urn:subject".into(),
        predicate: "urn:count".into(),
        property_shape: "urn:count-shape".into(),
        as_of: "2026-11-12T01:00:00Z".into(),
        value: Scalar::Integer(3),
    }
}

#[test]
fn wire_contract_refuses_forged_identity_and_native_timestamps() {
    let value = serde_json::to_value(request()).unwrap();
    for field in [
        "writer",
        "tx",
        "originTx",
        "recorded_on",
        "recordedOn",
        "actor",
    ] {
        let mut forged = value.clone();
        forged[field] = "urn:forged".into();
        assert!(
            serde_json::from_value::<AppendRequest>(forged).is_err(),
            "{field}"
        );
    }
}

#[test]
fn missing_identity_or_unknown_graph_cannot_write() {
    let mut store = loaded();
    let before = store.latest_tx_id().unwrap();
    assert!(store.append_fluent_observation(&request()).is_err());
    with_identity(Some(identity()), || {
        let mut request = request();
        request.graph = "urn:absent-graph".into();
        assert!(store.append_fluent_observation(&request).is_err());
        assert!(
            store
                .fluent_snapshot(&request.graph, &request.fluent)
                .is_err()
        );
        assert!(store.lookup("urn:absent-graph").unwrap().is_none());
    });
    assert_eq!(store.latest_tx_id().unwrap(), before);
}

#[cfg(feature = "shacl")]
#[test]
fn exact_replay_retains_tx_and_replacement_refuses_atomically() {
    let mut store = loaded();
    with_identity(Some(identity()), || {
        let mut request = request();
        let tx = store.append_fluent_observation(&request).unwrap();
        let before = store.latest_tx_id().unwrap();
        assert_eq!(store.append_fluent_observation(&request).unwrap(), tx);
        assert_eq!(store.latest_tx_id().unwrap(), before);
        let snapshot = store
            .fluent_snapshot(&request.graph, &request.fluent)
            .unwrap();
        assert_eq!(snapshot.policy, Policy::Sum);
        assert_eq!(snapshot.current.values, [Scalar::Integer(3)].into());
        assert_eq!(snapshot.producing_txs, [u64::try_from(tx).unwrap()].into());
        assert_eq!(store.transaction_auth(tx).unwrap(), Some(identity()));
        request.value = Scalar::Integer(99);
        assert!(store.append_fluent_observation(&request).is_err());
        assert_eq!(store.latest_tx_id().unwrap(), before);
        assert_eq!(
            store
                .fluent_snapshot(&request.graph, &request.fluent)
                .unwrap()
                .current,
            snapshot.current
        );
        request.event = "urn:event:second".into();
        request.value = Scalar::Integer(2);
        let second = store.append_fluent_observation(&request).unwrap();
        let snapshot = store
            .fluent_snapshot(&request.graph, &request.fluent)
            .unwrap();
        assert_eq!(snapshot.current.values, [Scalar::Integer(5)].into());
        assert!(
            snapshot
                .producing_txs
                .contains(&u64::try_from(second).unwrap())
        );
    });
}

#[test]
fn undeclared_binding_refuses_without_a_transaction() {
    let mut store = loaded();
    with_identity(Some(identity()), || {
        let before = store.latest_tx_id().unwrap();
        let mut request = request();
        request.property_shape = "urn:undeclared-shape".into();
        assert!(store.append_fluent_observation(&request).is_err());
        assert_eq!(store.latest_tx_id().unwrap(), before);
    });
}

#[cfg(feature = "shacl")]
#[test]
fn two_store_native_observations_have_a_commutative_union() {
    let mut left = loaded();
    let mut right = loaded();
    let first = request();
    with_identity(Some(identity()), || {
        left.append_fluent_observation(&first).unwrap();
    });
    let mut second = request();
    second.event = "urn:event:other-store".into();
    second.value = Scalar::Integer(2);
    let mut other = identity();
    other.principal = "urn:writer:other-store".into();
    with_identity(Some(other), || {
        right.append_fluent_observation(&second).unwrap();
    });
    let a = writer::read_log(&left, 0, &first.fluent, Policy::Sum).unwrap();
    let b = writer::read_log(&right, 0, &first.fluent, Policy::Sum).unwrap();
    let mut ab = a.clone();
    ab.merge(&b).unwrap();
    let mut ba = b;
    ba.merge(&a).unwrap();
    assert_eq!(ab, ba);
    assert_eq!(
        ab.current(&first.fluent, Policy::Sum)
            .unwrap()
            .unwrap()
            .values,
        [Scalar::Integer(5)].into()
    );
}
