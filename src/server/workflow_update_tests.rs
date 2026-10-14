//! Actual SPARQL Update processing, including concurrent compare-and-set.
use super::super::{SharedStore, StoreHandle};
use super::apply_update_as;
use quipu::{
    Store,
    governance::transition::transition_message_v2,
    namespace::DEFAULT_BASE_NS as NS,
    signing::{public_key_hex, sign_hex},
};
use ring::{rand::SystemRandom, signature::Ed25519KeyPair};
use std::sync::{Arc, Barrier};
const RUN: &str = "urn:shuttle:run:direct";
const TS: &str = "2026-10-11T00:00:00Z";
fn setup(full: bool) -> (SharedStore, Ed25519KeyPair) {
    let doc = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let kp = Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap();
    let mut s = Store::open_in_memory().unwrap();
    s.governance_config_mut().enforce_workflows = true;
    let shared = Arc::new(StoreHandle::writer_only(s));
    let seed = format!(
        r#"PREFIX a:<{NS}> INSERT DATA {{
 <urn:reg> a a:VerifierRegistration ; a:verifier "alice" ; a:publicKey "{}" .
 <urn:w> a a:Workflow,a:WorkflowDefinition ; a:initialState "open" ; a:terminalState "reviewed" ; a:hasStep <urn:w/step/review> .
 <urn:w/step/review> a a:WorkflowStep,a:Step ; a:fromState "open" ; a:toState "reviewed" .
 <{RUN}> a a:WorkflowRun ; a:runOf <urn:w> ; a:currentState "open" ; a:currentRevision 0 . }}"#,
        public_key_hex(&kp)
    );
    apply_update_as(&shared, &seed, full).unwrap();
    (shared, kp)
}
fn advance(kp: &Ed25519KeyPair, event: &str, step: &str, revision: u64) -> String {
    let sig = sign_hex(
        kp,
        &transition_message_v2(RUN, step, "open", "reviewed", TS, "alice", revision),
    );
    format!(
        r#"PREFIX a:<{NS}> PREFIX prov:<http://www.w3.org/ns/prov#>
 DELETE {{ <{RUN}> a:currentState ?state ; a:currentRevision ?revision . }}
 INSERT {{ <{RUN}> a:currentState "reviewed" ; a:currentRevision 1 .
 <{event}> a a:TransitionEvent ; a:inRun <{RUN}> ; a:atStep <urn:w/step/{step}> ; a:fromState "open" ; a:toState "reviewed" ; a:transitionVersion 2 ; a:fromRevision {revision} ; prov:endedAtTime "{TS}" ; a:performedBy <urn:shuttle:agent:alice> ; a:signature "{sig}" . }}
 WHERE {{ <{RUN}> a:currentState ?state ; a:currentRevision ?revision . FILTER(?state="open" && ?revision=0) }}"#
    )
}
fn result(s: &SharedStore, q: &str) -> String {
    match quipu::sparql::query(&s.lock(), q).unwrap() {
        quipu::sparql::QueryResult::Select { rows, .. } => {
            let mut canonical: Vec<_> = rows
                .into_iter()
                .map(|row| {
                    let ordered: std::collections::BTreeMap<_, _> = row
                        .into_iter()
                        .map(|(k, v)| (k, format!("{v:?}")))
                        .collect();
                    format!("{ordered:?}")
                })
                .collect();
            canonical.sort();
            format!("{canonical:?}")
        }
        other => format!("{other:?}"),
    }
}

#[test]
fn update_lands_exactly_one_and_lost_response_replay_is_a_noop() {
    for full in [false, true] {
        let (s, k) = setup(full);
        let q = advance(&k, "urn:event:1", "review", 0);
        apply_update_as(&s, &q, full).unwrap();
        let before = result(&s, &format!("SELECT ?p ?v WHERE {{ <{RUN}> ?p ?v }}"));
        apply_update_as(&s, &q, full).unwrap();
        assert_eq!(
            before,
            result(&s, &format!("SELECT ?p ?v WHERE {{ <{RUN}> ?p ?v }}"))
        );
        assert!(before.contains("reviewed"));
    }
}
#[test]
fn update_cannot_forge_a_step_or_signed_revision() {
    for full in [false, true] {
        for (step, rev) in [("forged", 0), ("review", 1)] {
            let (s, k) = setup(full);
            assert!(apply_update_as(&s, &advance(&k, "urn:event:bad", step, rev), full).is_err());
            assert!(
                result(
                    &s,
                    &format!("SELECT ?v WHERE {{ <{RUN}> <{NS}currentState> ?v }}")
                )
                .contains("open")
            );
            assert!(result(&s, "ASK { <urn:event:bad> ?p ?v }").contains("false"));
        }
    }
}
#[test]
fn concurrent_cas_advances_create_only_one_event() {
    for full in [false, true] {
        let (s, k) = setup(full);
        let barrier = Arc::new(Barrier::new(2));
        let mut threads = Vec::new();
        for id in ["urn:event:a", "urn:event:b"] {
            let q = advance(&k, id, "review", 0);
            let shared = s.clone();
            let gate = barrier.clone();
            threads.push(std::thread::spawn(move || {
                gate.wait();
                apply_update_as(&shared, &q, full).unwrap();
            }));
        }
        for t in threads {
            t.join().unwrap();
        }
        let a = result(&s, "ASK { <urn:event:a> ?p ?v }");
        let b = result(&s, "ASK { <urn:event:b> ?p ?v }");
        assert_ne!(a.contains("true"), b.contains("true"));
        assert!(
            result(
                &s,
                &format!("SELECT ?v WHERE {{ <{RUN}> <{NS}currentRevision> ?v }}")
            )
            .contains("Int(1)")
        );
    }
}

#[tokio::test]
async fn capabilities_reports_actual_opt_in() {
    let store = Arc::new(StoreHandle::writer_only(Store::open_in_memory().unwrap()));
    let value = super::super::base::workflow_capabilities(axum::extract::State(store.clone()))
        .await
        .unwrap()
        .0;
    assert_eq!(value["enabled"], false);
    store.lock().governance_config_mut().enforce_workflows = true;
    let value = super::super::base::workflow_capabilities(axum::extract::State(store))
        .await
        .unwrap()
        .0;
    assert_eq!(value["enabled"], true);
    assert_eq!(value["signature_version"], 2);
}
