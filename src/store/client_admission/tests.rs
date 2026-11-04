//! Real shared-key, current-registry and transactional graph admission controls.
#![cfg(feature = "shacl")]

use super::*;
use crate::client_changeset::{ClientOverlay, ProposalIdentity};
use crate::governance::trust_root;
use crate::types::Op;

const GRAPH: &str = "urn:graph:client-test";
const NAME: &str = "urn:p:name";
const NOTE: &str = "urn:p:note";
const T0: &str = "2026-01-01T00:00:00Z";
const T1: &str = "2026-01-02T00:00:00Z";
const SHAPES: &str = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
<urn:shape:Item> a sh:NodeShape ; sh:targetClass <urn:Item> ;
 sh:property [ sh:path <urn:p:name> ; sh:minCount 1 ; sh:maxCount 1 ;
               sh:datatype <http://www.w3.org/2001/XMLSchema#string> ] .
"#;

struct Fixture {
    store: Store,
    key: ring::signature::Ed25519KeyPair,
    registration: String,
    overlay: ClientOverlay,
}

fn write(store: &mut Store, additions: &str, removals: &str, timestamp: &str) {
    let id = store.registered_graph_id(GRAPH).unwrap().unwrap();
    let mut datums = crate::rdf::parse_rdf(
        store,
        additions.as_bytes(),
        oxrdfio::RdfFormat::NTriples,
        None,
        timestamp,
    )
    .unwrap();
    let mut retracts = crate::rdf::parse_rdf(
        store,
        removals.as_bytes(),
        oxrdfio::RdfFormat::NTriples,
        None,
        timestamp,
    )
    .unwrap();
    for d in &mut retracts {
        d.op = Op::Retract;
    }
    datums.extend(retracts);
    store
        .transact_to_graph(&datums, timestamp, None, Some("synthetic-fixture"), id)
        .unwrap();
}

fn fixture() -> Fixture {
    fixture_with_seed("")
}

fn fixture_with_seed(extra: &str) -> Fixture {
    let mut store = Store::open_in_memory().unwrap();
    store.graph_create(GRAPH).unwrap();
    write(
        &mut store,
        &format!(
            "<urn:item> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <urn:Item> .\n<urn:item> <urn:p:name> \"base\" .\n{extra}"
        ),
        "",
        T0,
    );
    store
        .load_shapes("fixed-client-fixture", SHAPES, T0)
        .unwrap();
    let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[19; 32]).unwrap();
    let public = quipu_signing::public_key_hex(&key);
    let pop = quipu_signing::sign_hex(
        &key,
        &trust_root::bootstrap_message(&store.store_id().unwrap(), "client-fixture", &public),
    );
    let registration = trust_root::bootstrap(
        &mut store,
        "client-fixture",
        &public,
        &[
            write_capability(GRAPH, NAME),
            write_capability(GRAPH, NOTE),
            write_capability(GRAPH, crate::namespace::RDF_TYPE),
        ],
        &pop,
        T0,
    )
    .unwrap()
    .registration;
    let base = store
        .client_base_journal(GRAPH, store.latest_tx_id().unwrap())
        .unwrap();
    let mut overlay = ClientOverlay::open(base, "urn:fork:synthetic-client".into()).unwrap();
    let mut proposed = overlay.read().clone();
    proposed.remove("<urn:item> <urn:p:name> \"base\"");
    proposed.insert("<urn:item> <urn:p:name> \"local\"".into());
    overlay.replace(proposed).unwrap();
    Fixture {
        store,
        key,
        registration,
        overlay,
    }
}

fn proposal(f: &Fixture) -> SignedChangeSet {
    f.overlay
        .sign(
            &f.key,
            ProposalIdentity {
                key_id: f.registration.clone(),
                signer: "client-fixture".into(),
                operation_id: "00000000-0000-4000-8000-000000000001".into(),
                issued_at: T1.into(),
                nonce: "aa".repeat(16),
            },
        )
        .unwrap()
}

fn resign(f: &Fixture, p: &mut SignedChangeSet) {
    p.envelope.body_sha256 = digest(p.body.as_bytes());
    p.signature = quipu_signing::sign_hex(&f.key, &p.envelope.message().unwrap());
}

fn state(store: &Store) -> (i64, String, i64) {
    let id = store.registered_graph_id(GRAPH).unwrap().unwrap();
    let current = graph(&base::rows(store, id, store.latest_tx_id().unwrap()).unwrap()).unwrap();
    (
        store.latest_tx_id().unwrap(),
        render(&current),
        store
            .conn
            .query_row("SELECT COUNT(*) FROM client_changeset_receipts", [], |r| {
                r.get(0)
            })
            .unwrap(),
    )
}

#[test]
fn explicit_rebase_admits_new_operation_once_and_never_retargets_old_signed_bytes() {
    use crate::client_changeset::RebaseOutcome;
    let mut f = fixture();
    let previous = proposal(&f);
    let original = serde_json::to_vec(&previous).unwrap();
    write(
        &mut f.store,
        "<urn:item> <urn:p:note> \"upstream\" .",
        "",
        T1,
    );
    let current = f
        .store
        .client_base_journal(GRAPH, f.store.latest_tx_id().unwrap())
        .unwrap();
    let identity = ProposalIdentity {
        key_id: f.registration.clone(),
        signer: "client-fixture".into(),
        operation_id: "00000000-0000-4000-8000-000000000002".into(),
        issued_at: T1.into(),
        nonce: "bb".repeat(16),
    };
    let RebaseOutcome::Prepared { rebased } = f
        .overlay
        .rebase(
            &previous,
            &quipu_signing::public_key_hex(&f.key),
            &current,
            SHAPES,
            identity.clone(),
        )
        .unwrap()
    else {
        panic!("unrelated upstream fact must rebase")
    };
    assert_eq!(serde_json::to_vec(&previous).unwrap(), original);
    assert_eq!(rebased.previous, previous);
    let mut next = rebased.overlay.sign(&f.key, identity).unwrap();
    let before = state(&f.store);
    let correct = next.signature.clone();
    next.signature = previous.signature;
    assert!(f.store.admit_client_changeset(&next).is_err());
    assert_eq!(state(&f.store), before);
    next.signature = correct;
    let Admission::Accepted {
        receipt,
        replayed: false,
    } = f.store.admit_client_changeset(&next).unwrap()
    else {
        panic!("new signed operation must pass master admission")
    };
    assert_eq!(receipt.operation_id, rebased.supersession.new_operation_id);
    let after = state(&f.store);
    assert!(after.1.contains("upstream"));
    assert!(after.1.contains("local"));
    assert_eq!(after.0, before.0 + 1);
    assert_eq!(after.2, before.2 + 1);
    let Admission::Accepted {
        receipt: retry,
        replayed: true,
    } = f.store.admit_client_changeset(&next).unwrap()
    else {
        panic!("retry must recover the same receipt")
    };
    assert_eq!(retry, receipt);
    assert_eq!(state(&f.store), after);
    assert!(
        f.store
            .client_changeset_receipt(&rebased.supersession.old_operation_id)
            .unwrap()
            .is_none()
    );
    println!(
        "REBASE {}",
        serde_json::json!({
            "old_base_tx": f.overlay.base().base_tx(),
            "new_base_tx": rebased.overlay.base().base_tx(),
            "old_operation_id": rebased.supersession.old_operation_id,
            "new_operation_id": receipt.operation_id,
            "old_wire_unchanged": serde_json::to_vec(&rebased.previous).unwrap() == original,
            "old_signature_refused": true,
            "upstream_fact_retained": after.1.contains("upstream"),
            "accepted_tx": receipt.accepted_tx,
            "retry_added_transactions": state(&f.store).0 - after.0,
        })
    );
}

#[test]
fn accepted_once_and_identical_retry_recovers_the_same_receipt_without_a_transaction() {
    let mut f = fixture();
    let p = proposal(&f);
    let before = f.store.latest_tx_id().unwrap();
    let Admission::Accepted {
        receipt,
        replayed: false,
    } = f.store.admit_client_changeset(&p).unwrap()
    else {
        panic!("expected acceptance")
    };
    assert_eq!(receipt.accepted_tx, before + 1);
    assert_eq!(
        f.store
            .client_changeset_receipt(&p.envelope.operation_id)
            .unwrap(),
        Some((*receipt).clone())
    );
    let accepted = state(&f.store);
    let Admission::Accepted {
        receipt: retry,
        replayed: true,
    } = f.store.admit_client_changeset(&p).unwrap()
    else {
        panic!("expected identical replay")
    };
    assert_eq!(receipt, retry);
    assert_eq!(state(&f.store), accepted);
    assert!(accepted.1.contains("\"local\""));
    assert!(!accepted.1.contains("\"base\""));
}

#[test]
fn changed_payload_same_operation_refuses_even_with_a_valid_signature() {
    let mut f = fixture();
    let p = proposal(&f);
    f.store.admit_client_changeset(&p).unwrap();
    let accepted = state(&f.store);
    let mut changed = p.clone();
    changed.envelope.nonce = "bb".repeat(16);
    resign(&f, &mut changed);
    assert!(
        f.store
            .admit_client_changeset(&changed)
            .unwrap_err()
            .to_string()
            .contains("different content")
    );
    assert_eq!(state(&f.store), accepted);
    let mut changed_payload = p.clone();
    let mut delta = p.changeset().unwrap();
    for statement in &mut delta.inserts {
        statement.triple = statement.triple.replace("\"local\"", "\"different\"");
    }
    changed_payload.body = delta.to_sparql_update();
    assert_ne!(changed_payload.body, p.body);
    changed_payload.envelope.delta_sha256 = format!("sha256:{}", delta.digest());
    resign(&f, &mut changed_payload);
    changed_payload
        .verify(&quipu_signing::public_key_hex(&f.key))
        .unwrap();
    assert!(
        f.store
            .admit_client_changeset(&changed_payload)
            .unwrap_err()
            .to_string()
            .contains("different content")
    );
    assert_eq!(state(&f.store), accepted);
}

#[test]
fn concurrent_nonconflicting_master_edit_is_preserved() {
    let mut f = fixture();
    let p = proposal(&f);
    write(
        &mut f.store,
        "<urn:item> <urn:p:note> \"master note\" .",
        "",
        T1,
    );
    assert!(matches!(
        f.store.admit_client_changeset(&p).unwrap(),
        Admission::Accepted { .. }
    ));
    let current = state(&f.store).1;
    assert!(current.contains("\"master note\""));
    assert!(current.contains("\"local\""));
}

#[test]
fn class_targeted_functional_conflict_never_overwrites_and_overlay_is_retained() {
    let mut f = fixture();
    let p = proposal(&f);
    let journal = f.overlay.base().clone();
    let local = f.overlay.read().clone();
    write(
        &mut f.store,
        "<urn:item> <urn:p:name> \"master\" .",
        "<urn:item> <urn:p:name> \"base\" .",
        T1,
    );
    let before = state(&f.store);
    let Admission::Conflicts { records } = f.store.admit_client_changeset(&p).unwrap() else {
        panic!("conflict required")
    };
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].max_count, 1);
    assert_eq!(records[0].ours, vec!["\"master\""]);
    assert_eq!(state(&f.store), before);
    assert_eq!(f.overlay.base(), &journal);
    assert_eq!(f.overlay.read(), &local);
}

#[test]
fn full_post_merge_shacl_refuses_a_required_value_deletion() {
    let mut f = fixture();
    f.overlay
        .replace(["<urn:item> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <urn:Item>".into()])
        .unwrap();
    let p = proposal(&f);
    let before = state(&f.store);
    assert!(f.store.admit_client_changeset(&p).is_err());
    assert_eq!(state(&f.store), before);
    assert!(f.store.conn.is_autocommit());
}

#[test]
fn bad_signature_signer_scope_base_epoch_and_shape_bindings_refuse_without_mutation() {
    let mut f = fixture();
    let original = proposal(&f);
    let before = state(&f.store);
    let mut cases = Vec::new();
    let mut p = original.clone();
    p.signature = "00".repeat(64);
    cases.push(p);
    let mut p = original.clone();
    p.envelope.signer = "unregistered".into();
    resign(&f, &mut p);
    cases.push(p);
    let mut p = original.clone();
    p.envelope.scope_digest = digest(b"other scope");
    resign(&f, &mut p);
    cases.push(p);
    let mut p = original.clone();
    p.envelope.base_slice_hash = digest(b"other base");
    resign(&f, &mut p);
    cases.push(p);
    let mut p = original.clone();
    p.envelope.log_epoch = "urn:epoch:foreign".into();
    resign(&f, &mut p);
    cases.push(p);
    let mut p = original.clone();
    p.envelope.audience = "urn:store:foreign".into();
    resign(&f, &mut p);
    cases.push(p);
    let mut p = original.clone();
    p.body = p
        .body
        .replace(&p.envelope.shapes_hash, &digest(b"other shapes"));
    p.envelope.shapes_hash = digest(b"other shapes");
    resign(&f, &mut p);
    cases.push(p);
    for p in cases {
        assert!(f.store.admit_client_changeset(&p).is_err());
        assert_eq!(state(&f.store), before);
        assert!(f.store.conn.is_autocommit());
    }
}

#[test]
fn current_registration_does_not_authorize_an_ungranted_predicate() {
    let mut f = fixture();
    let mut values = f.overlay.read().clone();
    values.insert("<urn:item> <urn:p:blocked> \"no\"".into());
    f.overlay.replace(values).unwrap();
    let p = proposal(&f);
    let before = state(&f.store);
    assert!(
        f.store
            .admit_client_changeset(&p)
            .unwrap_err()
            .to_string()
            .contains("graph/predicate authority")
    );
    assert_eq!(state(&f.store), before);
}

#[test]
fn failed_receipt_storage_rolls_back_the_data_terms_and_transaction() {
    let mut f = fixture();
    let mut values = f.overlay.read().clone();
    values.insert("<urn:new:client-entity> <urn:p:note> \"new\"".into());
    f.overlay.replace(values).unwrap();
    let p = proposal(&f);
    let before = state(&f.store);
    assert!(f.store.lookup("urn:new:client-entity").unwrap().is_none());
    f.store.conn.execute_batch("CREATE TEMP TRIGGER refuse_test_receipt BEFORE INSERT ON client_changeset_receipts BEGIN SELECT RAISE(ABORT,'synthetic receipt failure'); END;").unwrap();
    assert!(
        f.store
            .admit_client_changeset(&p)
            .unwrap_err()
            .to_string()
            .contains("synthetic receipt failure")
    );
    assert_eq!(state(&f.store), before);
    assert!(f.store.lookup("urn:new:client-entity").unwrap().is_none());
}

#[test]
fn stale_shapes_and_unavailable_base_leave_the_local_proposal_intact() {
    let mut f = fixture();
    let mut p = proposal(&f);
    p.body = p.body.replace(
        &format!("# base-tx: {}", p.envelope.base_tx),
        "# base-tx: 999999",
    );
    p.envelope.base_tx = 999999;
    resign(&f, &mut p);
    let before = state(&f.store);
    assert!(
        f.store
            .admit_client_changeset(&p)
            .unwrap_err()
            .to_string()
            .contains("base-unavailable")
    );
    assert_eq!(state(&f.store), before);
    let original = proposal(&f);
    f.store.load_shapes("new-shape", "@prefix sh: <http://www.w3.org/ns/shacl#> . <urn:new-shape> a sh:NodeShape ; sh:targetClass <urn:Unused> .", T1).unwrap();
    let before = state(&f.store);
    assert!(
        f.store
            .admit_client_changeset(&original)
            .unwrap_err()
            .to_string()
            .contains("stale-shapes")
    );
    assert_eq!(state(&f.store), before);
}

#[path = "loop_tests.rs"]
mod loop_tests;

#[path = "security_tests.rs"]
mod security_tests;

#[path = "respace_tests.rs"]
mod respace_tests;
