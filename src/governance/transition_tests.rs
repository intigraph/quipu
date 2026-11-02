//! Transition-signature gate tests (quipu-8cc), in the Q-SARC-PLACEMENT
//! pattern: pure `Transition::violation` units, `Store::transact` liveness
//! through the real write path, and a flag-off control that makes every
//! rejection attributable to this gate and nothing else.

use ring::rand::SystemRandom;
use ring::signature::Ed25519KeyPair;

use super::*;
use crate::namespace::RDF_TYPE;
use crate::signing::{public_key_hex, sign_hex};
use crate::store::Store;
use crate::types::Op;

const TS: &str = "2026-08-25T00:00:00Z";
const RUN: &str = "urn:shuttle:run:triage-7";
const STEP_IRI: &str = "urn:shuttle:workflow:triage/step/review";
const AGENT_IRI: &str = "urn:shuttle:agent:alice";

fn keypair() -> Ed25519KeyPair {
    let doc = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap()
}

/// Sign the canonical message the way shuttle's `advance` does, with the
/// field derivations `verify_write` re-applies: step and agent as the last
/// IRI segments.
fn sign(kp: &Ed25519KeyPair, from: &str, to: &str) -> String {
    sign_hex(
        kp,
        &transition_message(RUN, "review", from, to, TS, "alice"),
    )
}

// ── Pure-rule units ──────────────────────────────────────────────────────────

/// A complete event whose signature the closure-supplied registry can check.
fn event(signature: Option<&str>) -> Transition {
    Transition {
        in_run: vec![RUN.to_string()],
        at_step: vec![STEP_IRI.to_string()],
        from_state: vec!["open".to_string()],
        to_state: vec!["reviewed".to_string()],
        ended_at: vec![TS.to_string()],
        performed_by: vec![AGENT_IRI.to_string()],
        signature: signature.map(str::to_string).into_iter().collect(),
        ..Transition::default()
    }
}

fn keys_of(pk: &str) -> impl Fn(&str) -> crate::error::Result<Vec<String>> + '_ {
    move |agent: &str| {
        Ok(if agent == "alice" {
            vec![pk.to_string()]
        } else {
            Vec::new()
        })
    }
}

#[test]
fn the_message_format_is_shuttles_verbatim() {
    // The cross-repo contract. shuttle/signing.py::transition_message joins
    // the same seven slots with '|'; drift here refuses every genuine
    // transition.
    assert_eq!(
        transition_message(RUN, "review", "open", "reviewed", TS, "alice"),
        format!("shuttle-transition-v1|{RUN}|review|open|reviewed|{TS}|alice").into_bytes()
    );
}

#[test]
fn an_unsigned_transition_is_rejected() {
    let kp = keypair();
    let pk = public_key_hex(&kp);
    let why = event(None)
        .violation("urn:ev", &keys_of(&pk))
        .unwrap()
        .unwrap();
    assert!(why.contains("urn:ev"), "names the event: {why}");
    assert!(
        why.contains("aegis:signature"),
        "names the missing field: {why}"
    );
    assert!(
        why.contains("Sign the transition"),
        "names the remedy: {why}"
    );
}

#[test]
fn a_genuine_signature_verifies() {
    let kp = keypair();
    let pk = public_key_hex(&kp);
    let sig = sign(&kp, "open", "reviewed");
    assert!(
        event(Some(&sig))
            .violation("urn:ev", &keys_of(&pk))
            .unwrap()
            .is_none(),
        "a signature by the registered key over the exact fields must verify"
    );
}

#[test]
fn an_unregistered_signer_is_rejected() {
    let kp = keypair();
    let sig = sign(&kp, "open", "reviewed");
    let none = |_: &str| Ok(Vec::new());
    let why = event(Some(&sig))
        .violation("urn:ev", &none)
        .unwrap()
        .unwrap();
    assert!(
        why.contains("alice") && why.contains("VerifierRegistration"),
        "the refusal names the agent and the missing registration: {why}"
    );
    assert!(why.contains("register"), "names the remedy: {why}");
}

#[test]
fn a_signature_under_the_wrong_key_is_rejected() {
    let signer = keypair();
    let registered = keypair(); // a DIFFERENT identity holds the registration
    let pk = public_key_hex(&registered);
    let sig = sign(&signer, "open", "reviewed");
    let why = event(Some(&sig))
        .violation("urn:ev", &keys_of(&pk))
        .unwrap()
        .unwrap();
    assert!(
        why.contains("does not verify"),
        "a wrong-key signature is a verification failure, not a registry miss: {why}"
    );
}

#[test]
fn a_tampered_payload_is_rejected() {
    // Signed for open→reviewed; the staged facts claim open→approved. The
    // signature is genuine and the signer registered — the CONTENT lies.
    let kp = keypair();
    let pk = public_key_hex(&kp);
    let sig = sign(&kp, "open", "reviewed");
    let mut tampered = event(Some(&sig));
    tampered.to_state = vec!["approved".to_string()];
    let why = tampered
        .violation("urn:ev", &keys_of(&pk))
        .unwrap()
        .unwrap();
    assert!(
        why.contains("does not verify") && why.contains("approved"),
        "the refusal shows the fields actually checked: {why}"
    );
}

#[test]
fn an_incomplete_event_is_rejected_not_waved_through() {
    // No fromState => the canonical message cannot be re-derived. An
    // uncheckable signature must refuse, never pass.
    let kp = keypair();
    let pk = public_key_hex(&kp);
    let sig = sign(&kp, "open", "reviewed");
    let mut partial = event(Some(&sig));
    partial.from_state = Vec::new();
    let why = partial.violation("urn:ev", &keys_of(&pk)).unwrap().unwrap();
    assert!(
        why.contains("aegis:fromState") && why.contains("cannot be re-derived"),
        "names the missing field and why it is fatal: {why}"
    );
}

#[test]
fn an_ambiguous_field_is_rejected() {
    // Two toState values: one slot in the message, two candidates. Refusing
    // is the only reading that cannot be wrong (the placement discipline).
    let kp = keypair();
    let pk = public_key_hex(&kp);
    let sig = sign(&kp, "open", "reviewed");
    let mut two_valued = event(Some(&sig));
    two_valued.to_state.push("approved".to_string());
    let why = two_valued
        .violation("urn:ev", &keys_of(&pk))
        .unwrap()
        .unwrap();
    assert!(
        why.contains("distinct values") && why.contains("aegis:toState"),
        "names the ambiguity: {why}"
    );
}

#[test]
fn a_pipe_inside_a_field_is_rejected_as_shuttles_signer_would() {
    let kp = keypair();
    let pk = public_key_hex(&kp);
    let sig = sign(&kp, "open", "reviewed");
    let mut evil = event(Some(&sig));
    evil.from_state = vec!["open|reviewed".to_string()];
    let why = evil.violation("urn:ev", &keys_of(&pk)).unwrap().unwrap();
    assert!(why.contains("ambiguous"), "{why}");
}

// ── Liveness through the real write path ─────────────────────────────────────

fn assert_datum(store: &mut Store, entity: i64, attr: &str, value: Value) -> Datum {
    Datum {
        entity,
        attribute: store.intern(attr).unwrap(),
        value,
        valid_from: TS.to_string(),
        valid_to: None,
        op: Op::Assert,
    }
}

/// The datums shuttle's export stages for one `TransitionEvent` — type, run,
/// step, states, time, agent, and (optionally) the signature.
fn transition_datums(store: &mut Store, to: &str, signature: Option<&str>) -> Vec<Datum> {
    let ev = store.intern("urn:shuttle:event:triage-7:0").unwrap();
    let run = store.intern(RUN).unwrap();
    let step = store.intern(STEP_IRI).unwrap();
    let agent = store.intern(AGENT_IRI).unwrap();
    let class = store
        .intern(&format!("{DEFAULT_BASE_NS}TransitionEvent"))
        .unwrap();
    let rdf_type = store.intern(RDF_TYPE).unwrap();
    let mut datums = vec![
        Datum {
            entity: ev,
            attribute: rdf_type,
            value: Value::Ref(class),
            valid_from: TS.to_string(),
            valid_to: None,
            op: Op::Assert,
        },
        assert_datum(
            store,
            ev,
            &format!("{DEFAULT_BASE_NS}inRun"),
            Value::Ref(run),
        ),
        assert_datum(
            store,
            ev,
            &format!("{DEFAULT_BASE_NS}atStep"),
            Value::Ref(step),
        ),
        assert_datum(
            store,
            ev,
            &format!("{DEFAULT_BASE_NS}fromState"),
            Value::Str("open".into()),
        ),
        assert_datum(
            store,
            ev,
            &format!("{DEFAULT_BASE_NS}toState"),
            Value::Str(to.into()),
        ),
        assert_datum(
            store,
            ev,
            &format!("{PROV}endedAtTime"),
            Value::Str(TS.into()),
        ),
        assert_datum(
            store,
            ev,
            &format!("{DEFAULT_BASE_NS}performedBy"),
            Value::Ref(agent),
        ),
    ];
    if let Some(sig) = signature {
        datums.push(assert_datum(
            store,
            ev,
            &format!("{DEFAULT_BASE_NS}signature"),
            Value::Str(sig.into()),
        ));
    }
    datums
}

/// Register `pk` for alice — the fact shape `shuttle register` prints for a
/// human to load.
fn registration_datums(store: &mut Store, pk: &str) -> Vec<Datum> {
    let reg = store.intern("urn:shuttle:registration:alice").unwrap();
    let class = store
        .intern(&format!("{DEFAULT_BASE_NS}VerifierRegistration"))
        .unwrap();
    let rdf_type = store.intern(RDF_TYPE).unwrap();
    vec![
        Datum {
            entity: reg,
            attribute: rdf_type,
            value: Value::Ref(class),
            valid_from: TS.to_string(),
            valid_to: None,
            op: Op::Assert,
        },
        assert_datum(
            store,
            reg,
            &format!("{DEFAULT_BASE_NS}verifier"),
            Value::Str("alice".into()),
        ),
        assert_datum(
            store,
            reg,
            &format!("{DEFAULT_BASE_NS}publicKey"),
            Value::Str(pk.into()),
        ),
    ]
}

fn register(store: &mut Store, pk: &str, graph: i64) {
    let datums = registration_datums(store, pk);
    store
        .transact_to_graph(&datums, TS, None, None, graph)
        .expect("the registration must land");
}

fn govern(store: &mut Store) {
    crate::governance::trust_root::test_support::govern(
        store,
        "urn:shuttle:registration:alice",
        crate::governance::verifier_registry::AGENT_TIER,
        TS,
    );
}

#[test]
fn an_unsigned_transition_is_refused_at_the_write_path() {
    let mut store = Store::open_in_memory().unwrap();
    store.governance_config_mut().verify_transitions = true;
    let datums = transition_datums(&mut store, "reviewed", None);
    let err = store.transact(&datums, TS, None, None);
    let Err(Error::PolicyDenied(why)) = err else {
        panic!("an unsigned TransitionEvent must be refused at write, got {err:?}");
    };
    assert!(why.contains("aegis:signature"), "{why}");
    // The rollback contract: a refused transition leaves nothing behind.
    assert!(
        !matches!(
            crate::sparql::query(&store, "ASK { <urn:shuttle:event:triage-7:0> ?p ?o }").unwrap(),
            crate::sparql::QueryResult::Ask(true)
        ),
        "a refused transition must leave the store byte-identical"
    );
}

#[test]
fn a_signed_registered_transition_lands_with_the_flag_on() {
    // The GREEN case, end to end: real key, real registration, real gate.
    let mut store = Store::open_in_memory().unwrap();
    store.governance_config_mut().verify_transitions = true;
    let kp = keypair();
    register(&mut store, &public_key_hex(&kp), 0);
    govern(&mut store);
    let sig = sign(&kp, "open", "reviewed");
    let datums = transition_datums(&mut store, "reviewed", Some(&sig));
    store
        .transact(&datums, TS, None, None)
        .expect("a genuinely signed transition by a registered agent must land");
}

#[test]
fn udr9_named_identity_registration_requires_root_migration_and_endorsement() {
    let mut store = Store::open_in_memory().unwrap();
    store.governance_config_mut().verify_transitions = true;
    let identity = store.graph_create("urn:app:identity").unwrap();
    let kp = keypair();
    register(&mut store, &public_key_hex(&kp), identity);
    let sig = sign(&kp, "open", "reviewed");
    let datums = transition_datums(&mut store, "reviewed", Some(&sig));
    assert!(matches!(
        store.transact(&datums, TS, None, None),
        Err(Error::PolicyDenied(_))
    ));
    // Copying ordinary facts is not sufficient: endorsement is a separate
    // root-signed amendment, through the production trust-root gate.
    register(&mut store, &public_key_hex(&kp), 0);
    assert!(matches!(
        store.transact(&datums, TS, None, None),
        Err(Error::PolicyDenied(_))
    ));
    govern(&mut store);
    store
        .transact(&datums, TS, None, None)
        .expect("ROOT migration plus a real human-root amendment admits the key");
}

#[test]
fn an_unregistered_signer_is_refused_at_the_write_path() {
    let mut store = Store::open_in_memory().unwrap();
    store.governance_config_mut().verify_transitions = true;
    let kp = keypair(); // never registered
    let sig = sign(&kp, "open", "reviewed");
    let datums = transition_datums(&mut store, "reviewed", Some(&sig));
    let err = store.transact(&datums, TS, None, None);
    let Err(Error::PolicyDenied(why)) = err else {
        panic!("an unregistered signer must be refused at write, got {err:?}");
    };
    assert!(
        why.contains("alice") && why.contains("VerifierRegistration"),
        "{why}"
    );
}

#[test]
fn a_wrong_key_signature_is_refused_at_the_write_path() {
    let mut store = Store::open_in_memory().unwrap();
    store.governance_config_mut().verify_transitions = true;
    let registered = keypair();
    register(&mut store, &public_key_hex(&registered), 0);
    govern(&mut store);
    let forger = keypair();
    let sig = sign(&forger, "open", "reviewed");
    let datums = transition_datums(&mut store, "reviewed", Some(&sig));
    let err = store.transact(&datums, TS, None, None);
    assert!(
        matches!(err, Err(Error::PolicyDenied(_))),
        "a signature under an unregistered key must be refused, got {err:?}"
    );
}

#[test]
fn a_tampered_payload_is_refused_at_the_write_path() {
    // Alice signs open→reviewed; the export claims open→approved. Genuine
    // key, registered signer, lying content.
    let mut store = Store::open_in_memory().unwrap();
    store.governance_config_mut().verify_transitions = true;
    let kp = keypair();
    register(&mut store, &public_key_hex(&kp), 0);
    govern(&mut store);
    let sig = sign(&kp, "open", "reviewed");
    let datums = transition_datums(&mut store, "approved", Some(&sig));
    let err = store.transact(&datums, TS, None, None);
    let Err(Error::PolicyDenied(why)) = err else {
        panic!("a tampered transition must be refused at write, got {err:?}");
    };
    assert!(why.contains("does not verify"), "{why}");
}

#[test]
fn the_control_an_unsigned_transition_lands_with_the_flag_off() {
    // Same datums, flag off => accepted. That is what makes every rejection
    // above attributable to this gate — and it is today's default behaviour,
    // unchanged: turning verification on is a deliberate configuration act.
    let mut store = Store::open_in_memory().unwrap();
    assert!(
        !store.governance_config_mut().verify_transitions,
        "the flag must default to off"
    );
    let datums = transition_datums(&mut store, "reviewed", None);
    store
        .transact(&datums, TS, None, None)
        .expect("with verification off the same write lands");
}

#[test]
fn a_non_transition_write_is_not_touched() {
    // The pre-filter: ordinary traffic costs nothing and is never refused.
    let mut store = Store::open_in_memory().unwrap();
    store.governance_config_mut().verify_transitions = true;
    let e = store.intern("urn:test:thing").unwrap();
    let datums = vec![assert_datum(
        &mut store,
        e,
        "urn:test:state",
        Value::Str("open".into()),
    )];
    store
        .transact(&datums, TS, None, None)
        .expect("an ordinary write must pass untouched");
}

#[test]
fn udr9_unsigned_root_or_named_registration_cannot_authorize_a_transition() {
    for named in [false, true] {
        let mut store = Store::open_in_memory().unwrap();
        store.governance_config_mut().verify_transitions = true;
        let graph = if named {
            store.graph_create("urn:app:identity").unwrap()
        } else {
            0
        };
        let kp = keypair();
        register(&mut store, &public_key_hex(&kp), graph);
        let sig = sign(&kp, "open", "reviewed");
        let datums = transition_datums(&mut store, "reviewed", Some(&sig));
        let result = store.transact(&datums, TS, None, None);
        assert!(
            matches!(result, Err(Error::PolicyDenied(_))),
            "an ordinary graph writer must not enroll its own transition key: {result:?}"
        );
        assert!(!matches!(
            crate::sparql::query(&store, "ASK { <urn:shuttle:event:triage-7:0> ?p ?o }").unwrap(),
            crate::sparql::QueryResult::Ask(true)
        ));
    }
}

#[test]
fn udr9_a_named_tier_marker_never_endorses_a_registration() {
    let mut store = Store::open_in_memory().unwrap();
    store.governance_config_mut().verify_transitions = true;
    let graph = store.graph_create("urn:app:identity").unwrap();
    let kp = keypair();
    register(&mut store, &public_key_hex(&kp), graph);
    let reg = store.intern("urn:shuttle:registration:alice").unwrap();
    let tier = assert_datum(
        &mut store,
        reg,
        crate::governance::verifier_registry::TRUST_TIER,
        Value::Str("agent".into()),
    );
    store
        .transact_to_graph(&[tier], TS, None, None, graph)
        .unwrap();
    let datums = transition_datums(&mut store, "reviewed", Some(&sign(&kp, "open", "reviewed")));
    assert!(matches!(
        store.transact(&datums, TS, None, None),
        Err(Error::PolicyDenied(_))
    ));
}

#[test]
fn udr9_named_graph_key_shadowing_cannot_replace_an_endorsed_root_key() {
    let mut store = Store::open_in_memory().unwrap();
    store.governance_config_mut().verify_transitions = true;
    let root_key = keypair();
    register(&mut store, &public_key_hex(&root_key), 0);
    govern(&mut store);
    let graph = store.graph_create("urn:app:identity").unwrap();
    let attacker = keypair();
    register(&mut store, &public_key_hex(&attacker), graph);
    let forged = transition_datums(
        &mut store,
        "reviewed",
        Some(&sign(&attacker, "open", "reviewed")),
    );
    assert!(matches!(
        store.transact(&forged, TS, None, None),
        Err(Error::PolicyDenied(_))
    ));
    let genuine = transition_datums(
        &mut store,
        "reviewed",
        Some(&sign(&root_key, "open", "reviewed")),
    );
    store.transact(&genuine, TS, None, None).unwrap();
}

#[test]
fn udr9_revocation_refuses_a_new_backdated_transition_but_preserves_the_old_event() {
    let mut store = Store::open_in_memory().unwrap();
    store.governance_config_mut().verify_transitions = true;
    let kp = keypair();
    let key = public_key_hex(&kp);
    register(&mut store, &key, 0);
    govern(&mut store);
    let datums = transition_datums(&mut store, "reviewed", Some(&sign(&kp, "open", "reviewed")));
    store.transact(&datums, TS, None, None).unwrap();
    crate::governance::trust_root::test_support::amend_retract(
        &mut store,
        "urn:shuttle:registration:alice",
        &format!("{DEFAULT_BASE_NS}publicKey"),
        Value::Str(key),
        TS,
        TS,
    );
    let next = store.intern("urn:shuttle:event:revoked-key").unwrap();
    let mut backdated = datums;
    for datum in &mut backdated {
        datum.entity = next;
    }
    assert!(matches!(
        store.transact(&backdated, TS, None, None),
        Err(Error::PolicyDenied(_))
    ));
    assert!(matches!(
        crate::sparql::query(&store, "ASK { <urn:shuttle:event:triage-7:0> ?p ?o }").unwrap(),
        crate::sparql::QueryResult::Ask(true)
    ));
    assert!(!matches!(
        crate::sparql::query(&store, "ASK { <urn:shuttle:event:revoked-key> ?p ?o }").unwrap(),
        crate::sparql::QueryResult::Ask(true)
    ));
}

#[test]
fn udr9_self_registration_and_transition_in_one_write_are_rolled_back_together() {
    let mut store = Store::open_in_memory().unwrap();
    store.governance_config_mut().verify_transitions = true;
    let kp = keypair();
    let mut datums = registration_datums(&mut store, &public_key_hex(&kp));
    datums.extend(transition_datums(
        &mut store,
        "reviewed",
        Some(&sign(&kp, "open", "reviewed")),
    ));
    assert!(matches!(
        store.transact(&datums, TS, None, None),
        Err(Error::PolicyDenied(_))
    ));
    for iri in [
        "urn:shuttle:event:triage-7:0",
        "urn:shuttle:registration:alice",
    ] {
        assert!(!matches!(
            crate::sparql::query(&store, &format!("ASK {{ <{iri}> ?p ?o }}")).unwrap(),
            crate::sparql::QueryResult::Ask(true)
        ));
    }
}

#[test]
fn udr9_a_finite_unsigned_root_tier_cannot_authorize_a_persisted_transition() {
    let mut store = Store::open_in_memory().unwrap();
    store.governance_config_mut().verify_transitions = true;
    crate::governance::trust_root::test_support::ensure_root(&mut store);
    let kp = keypair();
    register(&mut store, &public_key_hex(&kp), 0);
    let reg = store.intern("urn:shuttle:registration:alice").unwrap();
    let mut tier = assert_datum(
        &mut store,
        reg,
        crate::governance::verifier_registry::TRUST_TIER,
        Value::Str("agent".into()),
    );
    tier.valid_to = Some("2099-01-01T00:00:00Z".into());
    let enrollment = store.transact(&[tier], TS, None, None);
    let datums = transition_datums(&mut store, "reviewed", Some(&sign(&kp, "open", "reviewed")));
    let transition = store.transact(&datums, TS, None, None);
    eprintln!("finite unsigned ROOT tier: enrollment={enrollment:?}; transition={transition:?}");
    assert!(
        matches!(transition, Err(Error::PolicyDenied(_))),
        "an unsigned finite tier must not authorize a persisted transition: {transition:?}"
    );
    assert!(!matches!(
        crate::sparql::query(&store, "ASK { <urn:shuttle:event:triage-7:0> ?p ?o }").unwrap(),
        crate::sparql::QueryResult::Ask(true)
    ));
}

#[test]
fn udr9_a_bounded_key_cannot_shadow_a_human_endorsed_registration() {
    let mut store = Store::open_in_memory().unwrap();
    store.governance_config_mut().verify_transitions = true;
    let genuine = keypair();
    register(&mut store, &public_key_hex(&genuine), 0);
    govern(&mut store);
    let attacker = keypair();
    let reg = store.intern("urn:shuttle:registration:alice").unwrap();
    let mut key = assert_datum(
        &mut store,
        reg,
        &format!("{DEFAULT_BASE_NS}publicKey"),
        Value::Str(public_key_hex(&attacker)),
    );
    key.valid_to = Some("2099-01-01T00:00:00Z".into());
    // The bounded row remains ordinary data; it must never become authority.
    store.transact(&[key], TS, None, None).unwrap();
    let forged = transition_datums(
        &mut store,
        "reviewed",
        Some(&sign(&attacker, "open", "reviewed")),
    );
    assert!(matches!(
        store.transact(&forged, TS, None, None),
        Err(Error::PolicyDenied(_))
    ));
    let valid = transition_datums(
        &mut store,
        "reviewed",
        Some(&sign(&genuine, "open", "reviewed")),
    );
    store.transact(&valid, TS, None, None).unwrap();
}

#[test]
fn udr9_bounded_human_tier_is_raw_data_not_current_human_authority() {
    use crate::governance::verifier_registry::{Scope, Witness, governed_tier, registered_keys};
    let mut store = Store::open_in_memory().unwrap();
    crate::governance::trust_root::test_support::ensure_root(&mut store);
    let kp = keypair();
    let key = public_key_hex(&kp);
    register(&mut store, &key, 0);
    let reg = store.intern("urn:shuttle:registration:alice").unwrap();
    let mut tier = assert_datum(
        &mut store,
        reg,
        crate::governance::verifier_registry::TRUST_TIER,
        Value::Str("human".into()),
    );
    tier.valid_to = Some("2099-01-01T00:00:00Z".into());
    store.transact(&[tier], TS, None, None).unwrap();
    let now = Witness::now();
    assert_eq!(
        registered_keys(&store, "alice", None, &now, Scope::Root).unwrap(),
        vec![key]
    );
    for scope in [Scope::HumanTier, Scope::Governed] {
        assert!(
            registered_keys(&store, "alice", None, &now, scope)
                .unwrap()
                .is_empty()
        );
    }
    assert!(
        governed_tier(&store, "urn:shuttle:registration:alice", &now)
            .unwrap()
            .is_none()
    );
}
