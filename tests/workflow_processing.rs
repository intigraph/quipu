//! Native transaction controls for direct graph workflow authority.
use quipu::{
    governance::transition::{transition_message, transition_message_v2},
    namespace::{DEFAULT_BASE_NS as NS, PROV, RDF_TYPE},
    signing::{public_key_hex, sign_hex},
    store::{Datum, Store},
    types::{Op, Value},
};
use ring::{rand::SystemRandom, signature::Ed25519KeyPair};
const TS: &str = "2026-10-11T00:00:00Z";
const RUN: &str = "urn:shuttle:run:guard";
const STEP: &str = "urn:shuttle:workflow:guard/step/review";
const AGENT: &str = "urn:shuttle:agent:alice";
fn datum(s: &mut Store, e: &str, a: &str, v: Value, op: Op) -> Datum {
    Datum {
        entity: s.intern(e).unwrap(),
        attribute: s.intern(a).unwrap(),
        value: v,
        valid_from: TS.into(),
        valid_to: None,
        op,
    }
}
fn text(s: &mut Store, e: &str, a: &str, v: &str) -> Datum {
    datum(s, e, &format!("{NS}{a}"), Value::Str(v.into()), Op::Assert)
}
fn link(s: &mut Store, e: &str, a: &str, v: &str) -> Datum {
    let id = s.intern(v).unwrap();
    datum(s, e, &format!("{NS}{a}"), Value::Ref(id), Op::Assert)
}
fn kind(s: &mut Store, e: &str, t: &str) -> Datum {
    let id = s.intern(&format!("{NS}{t}")).unwrap();
    datum(s, e, RDF_TYPE, Value::Ref(id), Op::Assert)
}
/// Private fixture: use the same human-root ceremony and signed endorsement
/// as production. Ordinary graph registration alone no longer grants trust.
fn endorse(s: &mut Store) {
    use quipu::governance::trust_root::{bootstrap, bootstrap_message, endorse, endorse_challenge};
    let doc = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let root = Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap();
    let key = public_key_hex(&root);
    let proof = sign_hex(
        &root,
        &bootstrap_message(&s.store_id().unwrap(), "fixture-human", &key),
    );
    bootstrap(s, "fixture-human", &key, &[], &proof, TS).unwrap();
    let challenge = endorse_challenge(s, "urn:reg", "agent").unwrap();
    endorse(
        s,
        "urn:reg",
        "agent",
        "fixture-human",
        &challenge.nonce,
        &sign_hex(&root, &challenge.message),
        "1970-01-01T00:00:00Z",
    )
    .unwrap();
}

fn setup(graph: i64) -> (Store, Ed25519KeyPair) {
    setup_with_loop(graph, false)
}
fn setup_with_loop(graph: i64, loop_plan: bool) -> (Store, Ed25519KeyPair) {
    let mut s = Store::open_in_memory().unwrap();
    s.governance_config_mut().enforce_workflows = true;
    let graph = if graph == 0 {
        0
    } else {
        s.graph_create("urn:test:workflow").unwrap()
    };
    let doc = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let kp = Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap();
    let reg = vec![
        kind(&mut s, "urn:reg", "VerifierRegistration"),
        text(&mut s, "urn:reg", "verifier", "alice"),
        text(&mut s, "urn:reg", "publicKey", &public_key_hex(&kp)),
    ];
    s.transact(&reg, TS, None, None).unwrap();
    endorse(&mut s);
    let mut def = vec![
        kind(&mut s, "urn:w", "Workflow"),
        kind(&mut s, "urn:w", "WorkflowDefinition"),
        text(&mut s, "urn:w", "initialState", "open"),
        text(
            &mut s,
            "urn:w",
            "terminalState",
            if loop_plan { "closed" } else { "reviewed" },
        ),
        link(&mut s, "urn:w", "hasStep", STEP),
        kind(&mut s, STEP, "WorkflowStep"),
        kind(&mut s, STEP, "Step"),
        text(&mut s, STEP, "fromState", "open"),
        text(&mut s, STEP, "toState", "reviewed"),
    ];
    if loop_plan {
        def.extend([
            link(&mut s, "urn:w", "hasStep", "urn:w/step/retry"),
            kind(&mut s, "urn:w/step/retry", "WorkflowStep"),
            kind(&mut s, "urn:w/step/retry", "Step"),
            text(&mut s, "urn:w/step/retry", "fromState", "reviewed"),
            text(&mut s, "urn:w/step/retry", "toState", "open"),
        ]);
    }
    s.transact_to_graph(&def, TS, None, None, graph).unwrap();
    let run = vec![
        kind(&mut s, RUN, "WorkflowRun"),
        link(&mut s, RUN, "runOf", "urn:w"),
        text(&mut s, RUN, "currentState", "open"),
        datum(
            &mut s,
            RUN,
            &format!("{NS}currentRevision"),
            Value::Int(0),
            Op::Assert,
        ),
    ];
    s.transact_to_graph(&run, TS, None, None, graph).unwrap();
    (s, kp)
}
fn event(
    s: &mut Store,
    k: &Ed25519KeyPair,
    id: &str,
    from: &str,
    to: &str,
    step: &str,
    signed: bool,
) -> Vec<Datum> {
    let strict = s.governance_config().enforce_workflows;
    let sig = sign_hex(
        k,
        &if strict {
            transition_message_v2(
                RUN,
                step.rsplit('/').next().unwrap(),
                from,
                to,
                TS,
                "alice",
                0,
            )
        } else {
            transition_message(RUN, step.rsplit('/').next().unwrap(), from, to, TS, "alice")
        },
    );
    let mut ds = vec![
        kind(s, id, "TransitionEvent"),
        link(s, id, "inRun", RUN),
        link(s, id, "atStep", step),
        text(s, id, "fromState", from),
        text(s, id, "toState", to),
        link(s, id, "performedBy", AGENT),
        datum(
            s,
            id,
            &format!("{PROV}endedAtTime"),
            Value::Str(TS.into()),
            Op::Assert,
        ),
    ];
    if strict {
        ds.extend([
            datum(
                s,
                id,
                &format!("{NS}transitionVersion"),
                Value::Int(2),
                Op::Assert,
            ),
            datum(
                s,
                id,
                &format!("{NS}fromRevision"),
                Value::Int(0),
                Op::Assert,
            ),
        ]);
    }
    if signed {
        ds.push(text(s, id, "signature", &sig));
    }
    ds
}
fn state_change(s: &mut Store, from: &str, to: &str) -> Vec<Datum> {
    vec![
        datum(
            s,
            RUN,
            &format!("{NS}currentState"),
            Value::Str(from.into()),
            Op::Retract,
        ),
        text(s, RUN, "currentState", to),
        datum(
            s,
            RUN,
            &format!("{NS}currentRevision"),
            Value::Int(0),
            Op::Retract,
        ),
        datum(
            s,
            RUN,
            &format!("{NS}currentRevision"),
            Value::Int(1),
            Op::Assert,
        ),
    ]
}
fn advance(s: &mut Store, k: &Ed25519KeyPair, id: &str, from: &str, to: &str) -> Vec<Datum> {
    let mut ds = event(s, k, id, from, to, STEP, true);
    ds.extend(state_change(s, from, to));
    ds
}
fn state(s: &Store, g: i64) -> String {
    let q = quipu::sparql::query(
        s,
        &format!(
            "SELECT ?v {} WHERE {{ <{RUN}> <{NS}currentState> ?v }}",
            if g == 0 {
                ""
            } else {
                "FROM <urn:test:workflow>"
            }
        ),
    )
    .unwrap();
    format!("{q:?}")
}
#[test]
fn legal_atomic_advance_and_exact_replay() {
    let (mut s, k) = setup(0);
    let ds = advance(&mut s, &k, "urn:ev:1", "open", "reviewed");
    s.transact(&ds, TS, None, None).unwrap();
    let before = state(&s, 0);
    s.transact(&ds, TS, None, None).unwrap();
    assert_eq!(before, state(&s, 0));
    assert!(before.contains("reviewed"));
}
#[test]
fn state_change_without_new_event_is_refused() {
    let (mut s, _) = setup(0);
    let ds = state_change(&mut s, "open", "reviewed");
    assert!(s.transact(&ds, TS, None, None).is_err());
    assert!(state(&s, 0).contains("open"));
}
#[test]
fn event_without_atomic_state_change_is_refused() {
    let (mut s, k) = setup(0);
    let ds = event(&mut s, &k, "urn:ev:1", "open", "reviewed", STEP, true);
    assert!(s.transact(&ds, TS, None, None).is_err());
    assert!(state(&s, 0).contains("open"));
}
#[test]
fn stale_state_cannot_commit_a_second_event() {
    let (mut s, k) = setup(0);
    let ds = advance(&mut s, &k, "urn:ev:1", "open", "reviewed");
    s.transact(&ds, TS, None, None).unwrap();
    let ds = advance(&mut s, &k, "urn:ev:2", "open", "reviewed");
    assert!(s.transact(&ds, TS, None, None).is_err());
    assert!(state(&s, 0).contains("reviewed"));
}
#[test]
fn undeclared_step_is_refused() {
    let (mut s, k) = setup(0);
    let mut ds = event(
        &mut s,
        &k,
        "urn:ev:1",
        "open",
        "reviewed",
        "urn:w/step/forged",
        true,
    );
    ds.extend(state_change(&mut s, "open", "reviewed"));
    assert!(s.transact(&ds, TS, None, None).is_err());
}
#[test]
fn signatures_required_even_with_legacy_flag_off() {
    let (mut s, k) = setup(0);
    assert!(!s.governance_config().verify_transitions);
    let mut ds = event(&mut s, &k, "urn:ev:1", "open", "reviewed", STEP, false);
    ds.extend(state_change(&mut s, "open", "reviewed"));
    assert!(s.transact(&ds, TS, None, None).is_err());
}
#[test]
fn signature_cannot_substitute_a_run_from_another_graph() {
    let (mut s, k) = setup(1);
    let ds = advance(&mut s, &k, "urn:ev:1", "open", "reviewed");
    assert!(s.transact(&ds, TS, None, None).is_err());
    assert!(state(&s, 1).contains("open"));
    let graph = s.graph_create("urn:test:workflow").unwrap();
    s.transact_to_graph(&ds, TS, None, None, graph).unwrap();
    assert!(state(&s, 1).contains("reviewed"));
}
#[test]
fn committed_plan_is_immutable() {
    let (mut s, _) = setup(0);
    let ds = vec![text(&mut s, STEP, "toState", "forged")];
    assert!(s.transact(&ds, TS, None, None).is_err());
}
#[test]
fn existing_event_cannot_be_rewritten() {
    let (mut s, k) = setup(0);
    let ds = advance(&mut s, &k, "urn:ev:1", "open", "reviewed");
    s.transact(&ds, TS, None, None).unwrap();
    let ds = vec![text(&mut s, "urn:ev:1", "toState", "forged")];
    assert!(s.transact(&ds, TS, None, None).is_err());
    assert!(state(&s, 0).contains("reviewed"));
}
#[test]
fn unrelated_write_remains_allowed() {
    let (mut s, _) = setup(0);
    let ds = vec![text(&mut s, "urn:unrelated", "status", "open")];
    s.transact(&ds, TS, None, None).unwrap();
}

#[test]
fn a_signed_undefined_run_is_refused_only_when_processing_is_enrolled() {
    for enabled in [false, true] {
        let mut s = Store::open_in_memory().unwrap();
        s.governance_config_mut().enforce_workflows = enabled;
        s.governance_config_mut().verify_transitions = true;
        let doc = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let kp = Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap();
        let reg = vec![
            kind(&mut s, "urn:reg", "VerifierRegistration"),
            text(&mut s, "urn:reg", "verifier", "alice"),
            text(&mut s, "urn:reg", "publicKey", &public_key_hex(&kp)),
        ];
        s.transact(&reg, TS, None, None).unwrap();
        endorse(&mut s);
        let ds = event(
            &mut s,
            &kp,
            "urn:ev:undefined",
            "open",
            "reviewed",
            STEP,
            true,
        );
        let r = s.transact(&ds, TS, None, None);
        assert_eq!(r.is_ok(), !enabled);
        if enabled {
            assert!(r.unwrap_err().to_string().contains("workflow:"));
        }
    }
}
#[test]
fn a_new_run_cannot_skip_its_initial_state() {
    let (mut s, _) = setup(0);
    let ds = vec![
        kind(&mut s, "urn:run:other", "WorkflowRun"),
        link(&mut s, "urn:run:other", "runOf", "urn:w"),
        text(&mut s, "urn:run:other", "currentState", "reviewed"),
    ];
    assert!(s.transact(&ds, TS, None, None).is_err());
}
#[test]
fn multiple_new_events_for_one_run_are_refused() {
    let (mut s, k) = setup(0);
    let mut ds = advance(&mut s, &k, "urn:ev:1", "open", "reviewed");
    ds.extend(event(
        &mut s, &k, "urn:ev:2", "open", "reviewed", STEP, true,
    ));
    assert!(s.transact(&ds, TS, None, None).is_err());
    assert!(state(&s, 0).contains("open"));
}
#[test]
fn an_event_cannot_be_deleted_by_stripping_its_type() {
    let (mut s, k) = setup(0);
    let ds = advance(&mut s, &k, "urn:ev:1", "open", "reviewed");
    s.transact(&ds, TS, None, None).unwrap();
    let class = s.intern(&format!("{NS}TransitionEvent")).unwrap();
    let ds = vec![datum(
        &mut s,
        "urn:ev:1",
        RDF_TYPE,
        Value::Ref(class),
        Op::Retract,
    )];
    assert!(s.transact(&ds, TS, None, None).is_err());
}

#[test]
fn signed_revision_refuses_stale_advance_after_a_state_cycle() {
    let (mut s, k) = setup_with_loop(0, true);
    let old = advance(&mut s, &k, "urn:ev:stale", "open", "reviewed");
    let first = advance(&mut s, &k, "urn:ev:first", "open", "reviewed");
    s.transact(&first, TS, None, None).unwrap();
    let sig = sign_hex(
        &k,
        &transition_message_v2(RUN, "retry", "reviewed", "open", TS, "alice", 1),
    );
    let mut retry = event(
        &mut s,
        &k,
        "urn:ev:retry",
        "reviewed",
        "open",
        "urn:w/step/retry",
        true,
    );
    retry.retain(|d| {
        s.resolve(d.attribute).unwrap() != format!("{NS}signature")
            && s.resolve(d.attribute).unwrap() != format!("{NS}fromRevision")
    });
    retry.extend([
        text(&mut s, "urn:ev:retry", "signature", &sig),
        datum(
            &mut s,
            "urn:ev:retry",
            &format!("{NS}fromRevision"),
            Value::Int(1),
            Op::Assert,
        ),
        datum(
            &mut s,
            RUN,
            &format!("{NS}currentState"),
            Value::Str("reviewed".into()),
            Op::Retract,
        ),
        text(&mut s, RUN, "currentState", "open"),
        datum(
            &mut s,
            RUN,
            &format!("{NS}currentRevision"),
            Value::Int(1),
            Op::Retract,
        ),
        datum(
            &mut s,
            RUN,
            &format!("{NS}currentRevision"),
            Value::Int(2),
            Op::Assert,
        ),
    ]);
    s.transact(&retry, TS, None, None).unwrap();
    assert!(state(&s, 0).contains("open"));
    let err = s.transact(&old, TS, None, None).unwrap_err();
    assert!(err.to_string().contains("workflow:"), "{err}");
    assert!(state(&s, 0).contains("open"));
    let q = quipu::sparql::query(
        &s,
        &format!("SELECT ?v WHERE {{ <{RUN}> <{NS}currentRevision> ?v }}"),
    )
    .unwrap();
    assert!(format!("{q:?}").contains("Int(2)"));
    assert!(matches!(
        quipu::sparql::query(&s, "ASK { <urn:ev:stale> ?p ?v }").unwrap(),
        quipu::sparql::QueryResult::Ask(false)
    ));
}
#[test]
fn revision_is_bound_by_the_signature() {
    let (mut s, k) = setup(0);
    let mut ds = advance(&mut s, &k, "urn:ev:1", "open", "reviewed");
    for d in &mut ds {
        if s.resolve(d.attribute).unwrap() == format!("{NS}fromRevision") {
            d.value = Value::Int(1);
        }
    }
    let err = s.transact(&ds, TS, None, None).unwrap_err();
    assert!(err.to_string().contains("does not verify"));
}

#[test]
fn declared_process_requires_both_existing_types_on_one_entity() {
    let (mut s, _) = setup(0);
    s.governance_config_mut().enforce_workflows = false;
    let t = s.intern(&format!("{NS}Workflow")).unwrap();
    let ds = vec![datum(&mut s, "urn:w", RDF_TYPE, Value::Ref(t), Op::Retract)];
    s.transact(&ds, TS, None, None).unwrap();
    s.governance_config_mut().enforce_workflows = true;
    let run = vec![
        kind(&mut s, "urn:run:alias", "WorkflowRun"),
        link(&mut s, "urn:run:alias", "runOf", "urn:w"),
        text(&mut s, "urn:run:alias", "currentState", "open"),
        datum(
            &mut s,
            "urn:run:alias",
            &format!("{NS}currentRevision"),
            Value::Int(0),
            Op::Assert,
        ),
    ];
    assert!(s.transact(&run, TS, None, None).is_err());
}

#[test]
fn an_unendorsed_key_cannot_advance_an_enrolled_workflow_with_legacy_flag_off() {
    let (mut s, _) = setup(0);
    assert!(!s.governance_config().verify_transitions);
    let doc = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let attacker = Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap();
    let reg = vec![
        kind(&mut s, "urn:unendorsed:reg", "VerifierRegistration"),
        text(&mut s, "urn:unendorsed:reg", "verifier", "alice"),
        text(
            &mut s,
            "urn:unendorsed:reg",
            "publicKey",
            &public_key_hex(&attacker),
        ),
    ];
    s.transact(&reg, TS, None, None).unwrap();
    let ds = advance(&mut s, &attacker, "urn:ev:unendorsed", "open", "reviewed");
    assert!(s.transact(&ds, TS, None, None).is_err());
    assert!(state(&s, 0).contains("open"));
    assert!(!matches!(
        quipu::sparql::query(&s, "ASK { <urn:ev:unendorsed> ?p ?o }").unwrap(),
        quipu::sparql::QueryResult::Ask(true)
    ));
}
