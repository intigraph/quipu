//! Source-only controls. None of these compile or execution results are assumed.
use super::*;
use crate::fluent::{Scalar, writer::Append};
use crate::governance::{trust_root, verifier_registry};
use crate::store::Datum;
use crate::transaction_auth::{Identity, with_identity};
use crate::{Value, types::Op};
use std::collections::BTreeSet;

const AT: &str = "2026-11-08T00:00:00Z";
const REG: &str = "urn:configuration-registration";
const VERIFIER: &str = "urn:config-verifier";

#[path = "pending_config_transport_tests.rs"]
mod transport_tests;

fn identity() -> Identity {
    Identity {
        principal: "urn:config-importer".into(),
        credential_id: Some("fixture".into()),
        auth_class: "named_bearer".into(),
        attestation: Default::default(),
    }
}

fn signer() -> quipu_signing::SigningIdentity {
    quipu_signing::SigningIdentity::new(
        ring::signature::Ed25519KeyPair::from_seed_unchecked(&[9; 32]).unwrap(),
        "fixture",
    )
}

fn loaded() -> Store {
    loaded_graph(crate::schema::ROOT_GRAPH_IRI).0
}

fn loaded_graph(graph: &str) -> (Store, i64) {
    let mut store = Store::open_in_memory().unwrap();
    store.set_base_ns(crate::namespace::DEFAULT_BASE_NS);
    store
        .load_shapes(
            "quipu-fluents-v1",
            include_str!("../../shapes/fluents-v1.ttl"),
            AT,
        )
        .unwrap();
    store.load_shapes("declarations", "@prefix sh: <http://www.w3.org/ns/shacl#> . @prefix qp: <https://w3id.org/intigraph/quipu/ns#> . <urn:base-shape> sh:path <urn:count>; sh:targetNode <urn:subject>; qp:currentValuePolicy \"sum\" . <urn:output-shape> sh:path <urn:aggregate>; sh:targetNode <urn:subject>; qp:currentValuePolicy \"sum\" .", AT).unwrap();
    let g = if graph == crate::schema::ROOT_GRAPH_IRI {
        0
    } else {
        store.graph_create(graph).unwrap()
    };
    with_identity(Some(identity()), || {
        store.append_fluent_native(
            &Append {
                fluent: "urn:base".into(),
                event: "urn:event".into(),
                subject: "urn:subject".into(),
                predicate: "urn:count".into(),
                property_shape: "urn:base-shape".into(),
                as_of: AT.into(),
                value: Scalar::Integer(3),
            },
            g,
        )
    })
    .unwrap();
    (store, g)
}

fn pending(name: &str, dependencies: &[&str]) -> Pending {
    Pending::from_registration(&Register {
        observable: Observable {
            graph: crate::schema::ROOT_GRAPH_IRI.into(),
            name: name.into(),
            observes: dependencies.iter().map(|s| (*s).into()).collect(),
            computed_by: Policy::Sum,
        },
        subject: "urn:subject".into(),
        predicate: "urn:aggregate".into(),
        property_shape: "urn:output-shape".into(),
    })
    .unwrap()
}

fn approve(store: &Store, request: &Pending) -> Approval {
    Approval::seal(store, request, &identity().principal, VERIFIER, &signer()).unwrap()
}

fn registration(store: &mut Store, scope: &str, graph: i64) {
    let ns = crate::namespace::DEFAULT_BASE_NS;
    let kind = store.intern(&format!("{ns}VerifierRegistration")).unwrap();
    let fields = [
        (crate::namespace::RDF_TYPE.to_string(), Value::Ref(kind)),
        (format!("{ns}verifier"), Value::Str(VERIFIER.into())),
        (
            format!("{ns}publicKey"),
            Value::Str(signer().public_key_hex()),
        ),
        (format!("{ns}attests"), Value::Str(scope.into())),
    ];
    let datums = fields
        .into_iter()
        .map(|(predicate, value)| Datum {
            entity: store.intern(REG).unwrap(),
            attribute: store.intern(&predicate).unwrap(),
            value,
            valid_from: AT.into(),
            valid_to: None,
            op: Op::Assert,
        })
        .collect::<Vec<_>>();
    store
        .transact_to_graph(&datums, AT, None, None, graph)
        .unwrap();
}

fn govern(store: &mut Store, approval: &Approval) {
    registration(store, &approval.scope_id().unwrap(), 0);
    trust_root::test_support::govern(store, REG, verifier_registry::AGENT_TIER, AT);
}

fn fingerprint(store: &Store) -> (i64, i64, usize) {
    (
        store.latest_tx_id().unwrap(),
        store
            .conn
            .query_row("SELECT count(*) FROM terms", [], |r| r.get(0))
            .unwrap(),
        store.current_facts().unwrap().len(),
    )
}

fn accept(store: &mut Store, request: &Pending, approval: &Approval) -> Result<i64> {
    with_identity(Some(identity()), || {
        store.accept_pending_observable_native(request, approval, 0)
    })
}

#[test]
fn pending_config_decode_preserves_exact_bytes_and_has_no_store_effect() {
    let store = loaded();
    let before = fingerprint(&store);
    let request = pending("urn:derived", &["urn:base"]);
    let mut raw = request.bytes().to_vec();
    raw.push(b' ');
    let decoded = Pending::decode(&raw).unwrap();
    assert_eq!(decoded.bytes(), raw);
    assert_ne!(
        decoded.id(),
        request.id(),
        "identity binds exact received bytes"
    );
    assert_eq!(fingerprint(&store), before);
    assert!(store.lookup("urn:derived").unwrap().is_none());
    for update in [
        "expression",
        "actor",
        "duplicate-dependency",
        "unsorted",
        "bad-iri",
    ] {
        let mut value: serde_json::Value = serde_json::from_slice(request.bytes()).unwrap();
        match update {
            "expression" => value["expression"] = serde_json::json!("arbitrary code"),
            "actor" => value["actor"] = serde_json::json!("root"),
            "duplicate-dependency" => {
                value["observes"] = serde_json::json!(["urn:base", "urn:base"]);
            }
            "unsorted" => value["observes"] = serde_json::json!(["urn:z", "urn:a"]),
            "bad-iri" => value["graph"] = serde_json::json!("not an IRI"),
            _ => unreachable!(),
        }
        assert!(Pending::decode(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    assert!(Pending::decode(&vec![b' '; 65_537]).is_err());
}

#[test]
fn pending_config_raw_registration_and_event_scope_never_authorize_configuration() {
    for mode in ["absent", "raw", "event-scope", "named-graph"] {
        let mut store = loaded();
        let request = pending("urn:derived", &["urn:base"]);
        let approval = approve(&store, &request);
        match mode {
            "raw" => registration(&mut store, &approval.scope_id().unwrap(), 0),
            "event-scope" => {
                let origin = crate::fluent::source_witness::capture(
                    &store,
                    0,
                    "urn:base",
                    "urn:event",
                    &store.store_id().unwrap(),
                )
                .unwrap();
                let proof =
                    crate::fluent::source_witness::seal(origin, VERIFIER, &signer()).unwrap();
                registration(&mut store, &proof.scope_id().unwrap(), 0);
                trust_root::test_support::govern(
                    &mut store,
                    REG,
                    verifier_registry::AGENT_TIER,
                    AT,
                );
                assert!(
                    proof.verify_governed_now(&store).is_ok(),
                    "event grant is actually valid"
                );
            }
            "named-graph" => {
                let g = store.graph_create("urn:untrusted-grants").unwrap();
                registration(&mut store, &approval.scope_id().unwrap(), g);
            }
            _ => {}
        }
        let before = fingerprint(&store);
        assert!(accept(&mut store, &request, &approval).is_err(), "{mode}");
        assert_eq!(fingerprint(&store), before);
        assert!(store.lookup("urn:derived").unwrap().is_none());
    }
}

#[test]
fn pending_config_governed_acceptance_replay_and_restart_use_native_registration() {
    let mut store = loaded();
    let request = pending("urn:derived", &["urn:base"]);
    let approval = approve(&store, &request);
    govern(&mut store, &approval);
    let before = fingerprint(&store);
    assert!(
        store
            .accept_pending_observable_native(&request, &approval, 0)
            .is_err()
    );
    assert_eq!(fingerprint(&store), before);
    let tx = accept(&mut store, &request, &approval).unwrap();
    assert_eq!(store.transaction_auth(tx).unwrap(), Some(identity()));
    let current = store.fluent_current_native(0, "urn:derived").unwrap();
    assert_eq!(current.values, BTreeSet::from([Scalar::Integer(3)]));
    assert_eq!(current.produced_by, BTreeSet::from(["urn:event".into()]));
    assert_eq!(accept(&mut store, &request, &approval).unwrap(), tx);
    assert_eq!(store.latest_tx_id().unwrap(), tx);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.db");
    std::fs::write(&path, store.serialize_db().unwrap()).unwrap();
    drop(store);
    let mut store = Store::open(path.to_str().unwrap()).unwrap();
    store.set_base_ns(crate::namespace::DEFAULT_BASE_NS);
    let wire = serde_json::to_vec(&approval).unwrap();
    let approval = Approval::decode(&wire).unwrap();
    assert_eq!(accept(&mut store, &request, &approval).unwrap(), tx);
    assert_eq!(
        store.fluent_current_native(0, "urn:derived").unwrap(),
        current
    );
}

#[test]
fn pending_config_approval_binds_receiver_request_profile_importer_and_logical_graph() {
    let mut store = loaded();
    let request = pending("urn:derived", &["urn:base"]);
    let approval = approve(&store, &request);
    govern(&mut store, &approval);
    let mut foreign = loaded();
    govern(&mut foreign, &approval); // same key+scope cannot grant another StoreID
    let before = fingerprint(&foreign);
    assert!(accept(&mut foreign, &request, &approval).is_err());
    assert_eq!(fingerprint(&foreign), before);
    let before = fingerprint(&store);
    let altered = pending("urn:changed", &["urn:base"]);
    assert!(accept(&mut store, &altered, &approval).is_err());
    assert_eq!(fingerprint(&store), before);
    let mut other = identity();
    other.principal = "urn:other-importer".into();
    assert!(
        with_identity(Some(other), || store
            .accept_pending_observable_native(&request, &approval, 0))
        .is_err()
    );
    assert_eq!(fingerprint(&store), before);
    let graph = store.graph_create("urn:different-logical-graph").unwrap();
    assert!(
        with_identity(Some(identity()), || store
            .accept_pending_observable_native(&request, &approval, graph))
        .is_err()
    );
    // graph_create legitimately changes terms; make a fresh baseline for profile refusal.
    assert!(fingerprint(&store).0 >= before.0);
    store.load_shapes("additional-profile", "@prefix sh: <http://www.w3.org/ns/shacl#> . <urn:extra> sh:path <urn:extra-property> .", AT).unwrap();
    let before = fingerprint(&store);
    assert!(accept(&mut store, &request, &approval).is_err());
    assert_eq!(fingerprint(&store), before);
    assert!(store.lookup("urn:derived").unwrap().is_none());
}

#[test]
fn pending_config_even_governed_approval_cannot_bypass_cycle_missing_or_policy_gates() {
    for mode in ["self-cycle", "missing", "policy"] {
        let mut store = loaded();
        let request = match mode {
            "self-cycle" => pending("urn:derived", &["urn:derived"]),
            "missing" => pending("urn:derived", &["urn:missing"]),
            _ => {
                let mut value: serde_json::Value =
                    serde_json::from_slice(pending("urn:derived", &["urn:base"]).bytes()).unwrap();
                value["policy"] = serde_json::json!("latest");
                Pending::decode(&serde_json::to_vec(&value).unwrap()).unwrap()
            }
        };
        let approval = approve(&store, &request);
        govern(&mut store, &approval);
        let before = fingerprint(&store);
        assert!(accept(&mut store, &request, &approval).is_err());
        assert_eq!(fingerprint(&store), before);
        assert!(store.lookup("urn:derived").unwrap().is_none());
    }
}

#[test]
fn pending_config_current_revocation_and_signature_tamper_refuse_atomically() {
    let mut store = loaded();
    let request = pending("urn:derived", &["urn:base"]);
    let approval = approve(&store, &request);
    govern(&mut store, &approval);
    let mut value = serde_json::to_value(&approval).unwrap();
    value["signature"] = serde_json::json!("00".repeat(64));
    let forged = Approval::decode(&serde_json::to_vec(&value).unwrap()).unwrap();
    let before = fingerprint(&store);
    assert!(accept(&mut store, &request, &forged).is_err());
    assert_eq!(fingerprint(&store), before);
    assert!(Approval::decode(&vec![b' '; 16_385]).is_err());
    let tx = accept(&mut store, &request, &approval).unwrap();
    trust_root::test_support::amend_retract(
        &mut store,
        REG,
        &format!("{}attests", crate::namespace::DEFAULT_BASE_NS),
        Value::Str(approval.scope_id().unwrap()),
        AT,
        "2026-11-08T00:01:00Z",
    );
    let before = fingerprint(&store);
    assert!(
        accept(&mut store, &request, &approval).is_err(),
        "replay cannot reuse revoked config authority"
    );
    assert_eq!(fingerprint(&store), before);
    assert!(store.latest_tx_id().unwrap() >= tx);
    assert_eq!(
        store
            .fluent_current_native(0, "urn:derived")
            .unwrap()
            .values,
        BTreeSet::from([Scalar::Integer(3)])
    );
}

#[test]
fn pending_config_exact_logical_graph_positive_and_remap_refusal() {
    for graph in [crate::schema::ROOT_GRAPH_IRI, "urn:graph-a", "urn:graph-b"] {
        let (mut store, g) = loaded_graph(graph);
        let mut registration = pending("urn:derived", &["urn:base"]).registration();
        registration.observable.graph = graph.into();
        let request = Pending::from_registration(&registration).unwrap();
        let approval = approve(&store, &request);
        govern(&mut store, &approval);
        let tx = with_identity(Some(identity()), || {
            store.accept_pending_observable_native(&request, &approval, g)
        })
        .unwrap();
        let other = store.graph_create("urn:remapped-target").unwrap();
        let before = fingerprint(&store);
        assert!(
            with_identity(Some(identity()), || store
                .accept_pending_observable_native(&request, &approval, other))
            .is_err()
        );
        assert_eq!(fingerprint(&store), before);
        assert_eq!(
            with_identity(Some(identity()), || store
                .accept_pending_observable_native(&request, &approval, g))
            .unwrap(),
            tx
        );
        assert_eq!(
            store
                .fluent_current_native(g, "urn:derived")
                .unwrap()
                .values,
            BTreeSet::from([Scalar::Integer(3)])
        );
    }
}
