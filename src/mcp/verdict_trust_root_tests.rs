//! aegis-c97o: `quipu_verdict_verify` trusts a key only through a
//! registration that chains to the human trust root, and says why.
//!
//! Every key is a TEST key; the trust root is the deterministic test root,
//! enrolled through the real bootstrap and signing real amendments.

use serde_json::json;

use super::super::governance::{tool_verdict_verify, tool_verifier_authorized};
use crate::governance::trust_root::test_support::{amend, amend_retract, govern};
use crate::store::{Datum, Store};
use crate::types::{Op, Value};

const NS: &str = "http://aegis.gastown.local/ontology/";
const T0: &str = "2026-01-01T00:00:00Z";
const T1: &str = "2026-01-10T00:00:00Z";
const T2: &str = "2026-01-20T00:00:00Z";
const T3: &str = "2026-01-30T00:00:00Z";

fn keypair() -> ring::signature::Ed25519KeyPair {
    let rng = ring::rand::SystemRandom::new();
    let doc = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
    ring::signature::Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap()
}

fn datum(store: &Store, e: &str, p: &str, value: Value, from: &str) -> Datum {
    Datum {
        entity: store.intern(e).unwrap(),
        attribute: store.intern(p).unwrap(),
        value,
        valid_from: from.to_string(),
        valid_to: None,
        op: Op::Assert,
    }
}

/// A plain (ungoverned) registration's datums.
fn registration(store: &Store, reg: &str, verifier: &str, key: &str, from: &str) -> Vec<Datum> {
    vec![
        datum(
            store,
            reg,
            "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
            Value::Ref(store.intern(&format!("{NS}VerifierRegistration")).unwrap()),
            from,
        ),
        datum(
            store,
            reg,
            &format!("{NS}verifier"),
            Value::Str(verifier.into()),
            from,
        ),
        datum(
            store,
            reg,
            &format!("{NS}publicKey"),
            Value::Str(key.into()),
            from,
        ),
        datum(
            store,
            reg,
            &format!("{NS}attests"),
            Value::Str("has-test".into()),
            from,
        ),
    ]
}

/// A verdict signed by `kp` as `verifier`.
fn verdict(kp: &ring::signature::Ed25519KeyPair, verifier: &str) -> serde_json::Value {
    let msg = crate::signing::verdict_message(
        "has-test",
        "http://ex/sym1",
        "satisfied",
        "fnv1a:00",
        "committed",
        verifier,
    );
    json!({
        "predicate_id": "has-test",
        "target_ref": "http://ex/sym1",
        "outcome": "satisfied",
        "evidence_hash": "fnv1a:00",
        "tier": "committed",
        "verifier": verifier,
        "signature": crate::signing::sign_hex(kp, &msg),
    })
}

/// Record `v`'s signature on stored verdict `iri` at `ts`, and return `v`
/// pointed at it (the recorded basis).
fn record(store: &mut Store, v: &serde_json::Value, iri: &str, ts: &str) -> serde_json::Value {
    let sig = v["signature"].as_str().unwrap().to_string();
    let d = datum(store, iri, &format!("{NS}signature"), Value::Str(sig), ts);
    store.transact(&[d], ts, None, None).unwrap();
    let mut out = v.clone();
    out["verdict"] = json!(iri);
    out
}

#[test]
fn self_enrollment_never_yields_trust() {
    // An agent writes its own registration with one unsigned write. The
    // store has a trust root, so the write lands (it is ungoverned) but the
    // verdict it signs is never trusted, and the answer says why.
    let mut store = Store::open_in_memory().unwrap();
    crate::governance::trust_root::test_support::ensure_root(&mut store);
    let agent = keypair();
    let reg = "http://ex/reg/self-enrolled";
    let datums = registration(
        &store,
        reg,
        "agent-x",
        &crate::signing::public_key_hex(&agent),
        T0,
    );
    store.transact(&datums, T0, Some("agent-x"), None).unwrap();

    let out = tool_verdict_verify(&store, &verdict(&agent, "agent-x")).unwrap();
    assert_eq!(out["signature_valid"], true, "{out:#}");
    assert_eq!(out["verifier_registered"], true, "{out:#}");
    assert_eq!(out["verifier_authorized"], false, "{out:#}");
    assert_eq!(out["trusted"], false, "{out:#}");
    assert_eq!(
        out["untrusted_reason"],
        "registration-not-chained-to-trust-root"
    );
    assert_eq!(out["unchained_registration"], reg);
    assert!(out["trust"].is_null());

    let auth = tool_verifier_authorized(
        &store,
        &json!({"verifier": "agent-x", "predicate": "has-test"}),
    )
    .unwrap();
    assert_eq!(auth["authorized"], false);
    assert_eq!(auth["reason"], "registration-not-chained-to-trust-root");

    // Marking itself governed is a registry change the gate refuses.
    let mark = vec![datum(
        &store,
        reg,
        crate::governance::verifier_registry::TRUST_TIER,
        Value::Str("agent".into()),
        T0,
    )];
    assert!(matches!(
        store.transact(&mark, T0, Some("agent-x"), None),
        Err(crate::error::Error::PolicyDenied(_))
    ));
    assert_eq!(
        tool_verdict_verify(&store, &verdict(&agent, "agent-x")).unwrap()["trusted"],
        false
    );
}

#[test]
fn a_key_added_to_an_endorsed_registration_without_the_root_is_refused() {
    let mut store = Store::open_in_memory().unwrap();
    let legit = keypair();
    let reg = "http://ex/reg/yupana";
    let datums = registration(
        &store,
        reg,
        "yupana",
        &crate::signing::public_key_hex(&legit),
        T0,
    );
    store.transact(&datums, T0, None, None).unwrap();
    govern(&mut store, reg, "agent", T0);
    let mallory = keypair();
    let add = vec![datum(
        &store,
        reg,
        &format!("{NS}publicKey"),
        Value::Str(crate::signing::public_key_hex(&mallory)),
        T1,
    )];
    assert!(store.transact(&add, T1, Some("mallory"), None).is_err());
    let out = tool_verdict_verify(&store, &verdict(&mallory, "yupana")).unwrap();
    assert_eq!(out["trusted"], false, "{out:#}");
    assert_eq!(out["signature_valid"], false, "{out:#}");
}

#[test]
fn a_root_signed_registration_is_trusted_and_says_why() {
    let mut store = Store::open_in_memory().unwrap();
    let kp = keypair();
    let reg = "http://ex/reg/yupana";
    let mut datums = registration(
        &store,
        reg,
        "yupana",
        &crate::signing::public_key_hex(&kp),
        T0,
    );
    datums.push(datum(
        &store,
        reg,
        crate::governance::verifier_registry::TRUST_TIER,
        Value::Str("agent".into()),
        T0,
    ));
    amend(&mut store, reg, datums, T0);

    let out = tool_verdict_verify(&store, &verdict(&kp, "yupana")).unwrap();
    assert_eq!(out["trusted"], true, "{out:#}");
    assert!(out["untrusted_reason"].is_null(), "{out:#}");
    assert_eq!(out["trust"]["registration"], reg);
    assert_eq!(out["trust"]["tier"], "agent");
    assert_eq!(out["trust"]["root"]["kind"], "amendment");
    assert_eq!(out["trust"]["root"]["root_signer"], "test-root");
    assert!(
        out["trust"]["root"]["amendment"]
            .as_str()
            .unwrap()
            .starts_with("http://ex/test-amendment/")
    );
}

#[test]
fn revocation_by_the_root_ends_trust_for_new_verdicts_only() {
    let mut store = Store::open_in_memory().unwrap();
    let kp = keypair();
    let reg = "http://ex/reg/yupana";
    let datums = registration(
        &store,
        reg,
        "yupana",
        &crate::signing::public_key_hex(&kp),
        T0,
    );
    store.transact(&datums, T0, None, None).unwrap();
    govern(&mut store, reg, "agent", T0);
    let v = verdict(&kp, "yupana");
    let recorded = record(&mut store, &v, "http://ex/verdict/before-revocation", T1);
    amend_retract(
        &mut store,
        reg,
        crate::governance::verifier_registry::TRUST_TIER,
        Value::Str("agent".into()),
        T0,
        T2,
    );

    // Read now: the endorsement is gone. The registration still exists, so
    // the answer is "not chained", not "not registered".
    let now = tool_verdict_verify(&store, &v).unwrap();
    assert_eq!(now["trusted"], false, "{now:#}");
    assert_eq!(
        now["untrusted_reason"],
        "registration-not-chained-to-trust-root"
    );
    // A verdict recorded while the endorsement stood still verifies as of
    // its recording (signing-plane S1).
    let then = tool_verdict_verify(&store, &recorded).unwrap();
    assert_eq!(then["trusted"], true, "{then:#}");
    assert_eq!(then["trust"]["tier"], "agent");
    // A verdict recorded AFTER the revocation does not.
    let late = record(&mut store, &v, "http://ex/verdict/after-revocation", T3);
    assert_eq!(
        tool_verdict_verify(&store, &late).unwrap()["trusted"],
        false
    );
}

#[test]
fn a_verdict_sealed_before_endorsement_stays_untrusted_after_migration() {
    // The stated historical rule: endorsement is NOT retroactive. A verdict
    // recorded while its registration had no trust-root chain is evaluated
    // against the registry as it stood then, so re-signing the registration
    // later does not make it trusted. Its signature still reports valid.
    let mut store = Store::open_in_memory().unwrap();
    let kp = keypair();
    let reg = "http://ex/reg/legacy";
    let datums = registration(
        &store,
        reg,
        "legacy",
        &crate::signing::public_key_hex(&kp),
        T0,
    );
    store.transact(&datums, T0, None, None).unwrap();
    let v = verdict(&kp, "legacy");
    let sealed_before = record(&mut store, &v, "http://ex/verdict/legacy", T1);
    govern(&mut store, reg, "agent", T2);

    let old = tool_verdict_verify(&store, &sealed_before).unwrap();
    assert_eq!(old["signature_valid"], true, "{old:#}");
    assert_eq!(old["trusted"], false, "{old:#}");
    assert_eq!(
        old["untrusted_reason"],
        "registration-not-chained-to-trust-root"
    );
    let sealed_after = record(&mut store, &v, "http://ex/verdict/migrated", T3);
    assert_eq!(
        tool_verdict_verify(&store, &sealed_after).unwrap()["trusted"],
        true
    );
}
