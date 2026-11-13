//! Native compatibility controls, using real root-signed registrations.
use super::*;

const CANONICAL: &str = "https://w3id.org/intigraph/camayoc/core/ns#signature";
const LEGACY: &str = "http://aegis.gastown.local/ontology/signature";
const VERDICT: &str = "http://ex/verdict/dual-witness";
const REG: &str = "http://ex/reg/dual-witness";

fn fixture(governed: bool, authorized: bool) -> (Store, serde_json::Value) {
    let mut store = Store::open_in_memory_test_ns().unwrap();
    let kp = keypair();
    let mut datums = registration(
        &store,
        REG,
        "dual-witness",
        &crate::signing::public_key_hex(&kp),
        T0,
    );
    if !authorized {
        datums.last_mut().unwrap().value = Value::Str("another-predicate".into());
    }
    if governed {
        datums.push(datum(
            &store,
            REG,
            crate::governance::verifier_registry::TRUST_TIER,
            Value::Str("agent".into()),
            T0,
        ));
        amend(&mut store, REG, datums, T0);
    } else {
        crate::governance::trust_root::test_support::ensure_root(&mut store);
        store.transact(&datums, T0, None, None).unwrap();
    }
    let mut input = verdict(&kp, "dual-witness");
    input["verdict"] = json!(VERDICT);
    (store, input)
}

fn signature_fact(store: &Store, input: &serde_json::Value, predicate: &str, at: &str) -> Datum {
    datum(
        store,
        VERDICT,
        predicate,
        Value::Str(input["signature"].as_str().unwrap().into()),
        at,
    )
}

#[test]
fn canonical_and_legacy_use_the_original_recorded_transaction() {
    for predicate in [LEGACY, CANONICAL] {
        let (mut store, mut input) = fixture(true, true);
        let original = input["signature"].clone();
        let fact = signature_fact(&store, &input, predicate, T1);
        let tx = store.transact(&[fact], T1, None, None).unwrap();
        // A supplied instant never overrides a recorded verdict's witness.
        input["signed_at"] = json!(T3);
        input["tx"] = json!(tx + 100);
        let out = tool_verdict_verify(&store, &input).unwrap();
        assert_eq!(out["trusted"], true, "{predicate}: {out:#}");
        assert_eq!(out["trust"]["registration"], REG);
        assert_eq!(out["trust"]["root"]["root_signer"], "test-root");
        assert_eq!(
            out["as_of"],
            json!({"tx": tx, "at": T1, "basis": "recorded"})
        );
        assert_eq!(input["signature"], original);
        assert_eq!(
            crate::governance::verifier_registry::Witness::of_fact(
                &store,
                VERDICT,
                predicate,
                original.as_str().unwrap(),
            )
            .unwrap()
            .unwrap()
            .tx,
            Some(tx)
        );
    }
}

#[test]
fn canonical_and_legacy_keep_revocation_bound_to_the_original_transaction() {
    for predicate in [LEGACY, CANONICAL] {
        let (mut store, input) = fixture(true, true);
        let fact = signature_fact(&store, &input, predicate, T1);
        let tx = store.transact(&[fact], T1, None, None).unwrap();
        amend_retract(
            &mut store,
            REG,
            crate::governance::verifier_registry::TRUST_TIER,
            Value::Str("agent".into()),
            T0,
            T2,
        );
        let then = tool_verdict_verify(&store, &input).unwrap();
        assert_eq!(then["trusted"], true, "{then:#}");
        assert_eq!(then["as_of"]["tx"], tx);
        let mut now = input.clone();
        now.as_object_mut().unwrap().remove("verdict");
        assert_eq!(tool_verdict_verify(&store, &now).unwrap()["trusted"], false);
        let late_iri = "http://ex/verdict/after-revocation";
        let fact = datum(
            &store,
            late_iri,
            predicate,
            Value::Str(input["signature"].as_str().unwrap().into()),
            T3,
        );
        store.transact(&[fact], T3, None, None).unwrap();
        now["verdict"] = json!(late_iri);
        assert_eq!(tool_verdict_verify(&store, &now).unwrap()["trusted"], false);
    }
}

#[test]
fn mixed_and_conflicting_assertions_refuse_even_when_one_signature_matches() {
    for (first, second, conflict) in [
        (LEGACY, CANONICAL, false),
        (LEGACY, CANONICAL, true),
        (CANONICAL, CANONICAL, true),
        (LEGACY, LEGACY, true),
    ] {
        let (mut store, input) = fixture(true, true);
        let first = signature_fact(&store, &input, first, T1);
        let mut second = signature_fact(&store, &input, second, T1);
        if conflict {
            second.value = Value::Str("different-original-signature".into());
        }
        store.transact(&[first, second], T1, None, None).unwrap();
        assert!(tool_verdict_verify(&store, &input).is_err());
    }
}

#[test]
fn duplicate_graphs_and_distinct_historical_transactions_refuse() {
    for predicate in [LEGACY, CANONICAL] {
        let (mut store, input) = fixture(true, true);
        let fact = signature_fact(&store, &input, predicate, T1);
        store.transact(&[fact.clone()], T1, None, None).unwrap();
        let graph = store.overlay_create("http://ex/other-witness", 0).unwrap();
        store
            .transact_to_graph(&[fact], T2, None, None, graph)
            .unwrap();
        assert!(tool_verdict_verify(&store, &input).is_err());

        let (mut store, input) = fixture(true, true);
        let fact = signature_fact(&store, &input, predicate, T1);
        store.transact(&[fact], T1, None, None).unwrap();
        let mut close = signature_fact(&store, &input, predicate, T2);
        close.op = Op::Retract;
        store.transact(&[close], T2, None, None).unwrap();
        let again = signature_fact(&store, &input, predicate, T3);
        store.transact(&[again], T3, None, None).unwrap();
        assert!(tool_verdict_verify(&store, &input).is_err());
    }
}

#[test]
fn missing_mismatched_and_non_string_signatures_never_fall_back_to_supplied_time() {
    for predicate in [LEGACY, CANONICAL] {
        for value in [None, Some(Value::Str("wrong".into())), Some(Value::Int(7))] {
            let (mut store, mut input) = fixture(true, true);
            if let Some(value) = value {
                let fact = datum(&store, VERDICT, predicate, value, T1);
                store.transact(&[fact], T1, None, None).unwrap();
            }
            input["signed_at"] = json!(T1);
            input["tx"] = json!(store.latest_tx_id().unwrap());
            input["trusted"] = json!(true);
            assert!(tool_verdict_verify(&store, &input).is_err());
        }
    }
}

#[test]
fn canonical_witness_does_not_endorse_or_expand_registration_authority() {
    for (governed, authorized, reason) in [
        (false, true, "registration-not-chained-to-trust-root"),
        (true, false, "not-authorized-for-predicate"),
    ] {
        let (mut store, mut input) = fixture(governed, authorized);
        let fact = signature_fact(&store, &input, CANONICAL, T1);
        store.transact(&[fact], T1, None, None).unwrap();
        input["trusted"] = json!(true);
        let out = tool_verdict_verify(&store, &input).unwrap();
        assert_eq!(out["signature_valid"], true);
        assert_eq!(out["trusted"], false, "{out:#}");
        assert_eq!(out["untrusted_reason"], reason);
        assert!(out["trust"].is_null());
    }
}

#[test]
fn canonical_registration_terms_are_not_silently_promoted_to_authority() {
    let mut store = Store::open_in_memory_test_ns().unwrap();
    crate::governance::trust_root::test_support::ensure_root(&mut store);
    let kp = keypair();
    let core = "https://w3id.org/intigraph/camayoc/core/ns#";
    let class = store
        .intern("https://w3id.org/intigraph/quipu/ns#VerifierRegistration")
        .unwrap();
    let mut datums = vec![datum(
        &store,
        REG,
        crate::namespace::RDF_TYPE,
        Value::Ref(class),
        T0,
    )];
    for (name, value) in [
        ("verifier", "dual-witness".to_string()),
        ("publicKey", crate::signing::public_key_hex(&kp)),
        ("attests", "has-test".to_string()),
        ("trustTier", "agent".to_string()),
    ] {
        datums.push(datum(
            &store,
            REG,
            &format!("{core}{name}"),
            Value::Str(value),
            T0,
        ));
    }
    // These are raw data, not a governed enrollment or a root amendment.
    store.transact(&datums, T0, None, None).unwrap();
    let mut input = verdict(&kp, "dual-witness");
    input["verdict"] = json!(VERDICT);
    input["trusted"] = json!(true);
    let fact = signature_fact(&store, &input, CANONICAL, T1);
    store.transact(&[fact], T1, None, None).unwrap();
    let out = tool_verdict_verify(&store, &input).unwrap();
    assert_eq!(out["as_of"]["basis"], "recorded");
    assert_eq!(out["verifier_registered"], false);
    assert_eq!(out["trusted"], false);
    assert_eq!(out["untrusted_reason"], "verifier-not-registered");
}

#[test]
fn a_missing_transaction_record_is_not_witness_evidence() {
    let (mut store, input) = fixture(true, true);
    let fact = signature_fact(&store, &input, CANONICAL, T1);
    store.transact(&[fact], T1, None, None).unwrap();
    // Simulate unreadable/corrupt provenance without altering signature bytes.
    store
        .conn
        .execute(
            "UPDATE facts SET tx = -999 WHERE e = ?1",
            [store.lookup(VERDICT).unwrap().unwrap()],
        )
        .unwrap();
    assert!(tool_verdict_verify(&store, &input).is_err());
}

#[test]
fn caller_supplied_instant_remains_an_untrusted_what_if() {
    let (store, mut input) = fixture(true, true);
    input.as_object_mut().unwrap().remove("verdict");
    input["signed_at"] = json!(T1);
    input["tx"] = json!(store.latest_tx_id().unwrap());
    let out = tool_verdict_verify(&store, &input).unwrap();
    assert_eq!(out["signature_valid"], true);
    assert_eq!(out["would_verify_as_of_supplied_instant"], true);
    assert_eq!(out["trusted"], false);
    assert_eq!(out["untrusted_reason"], "caller-supplied-instant");
    assert_eq!(out["as_of"]["basis"], "caller-supplied");
}
