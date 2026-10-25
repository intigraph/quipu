//! The ordinary write gates, applied to attested writes: authority refusals
//! are audited and reserved graphs cannot be protected. The success-path
//! verdict flush mirrors the ordinary write and is not tested here: no
//! accepted named-graph write that records a verdict could be constructed.
use super::tests::{OUT, footprint, proposal, ready, refused, sign};
use super::*;
use crate::verdict_schemes::webauthn_tests::Authenticator;

fn refusals(s: &Store) -> Vec<crate::store::events::EventRow> {
    s.events_after(0, 10_000, Some(&["write.refused".to_string()]), None)
        .unwrap()
}

#[test]
fn an_authority_refusal_records_nothing_but_is_audited() {
    let a = Authenticator::es256();
    let mut s = ready(&a);
    let c = s.attest_challenge(&proposal("tonight")).unwrap();
    s.governance_config_mut().enforce_authority = true;
    s.set_principal_chain(vec!["nobody".to_string()]);
    let before = footprint(&s);
    assert!(refusals(&s).is_empty(), "control: none before");
    assert!(
        s.attest_commit(&c.nonce, &sign(&a, &c, 8).as_assertion())
            .is_err()
    );
    assert_eq!(footprint(&s), before);
    let events = refusals(&s);
    assert_eq!(events.len(), 1);
    let payload: serde_json::Value = serde_json::from_str(&events[0].payload).unwrap();
    assert_eq!(payload["gate"], "authority");
    assert_eq!(payload["graph"], OUT);
}

#[test]
fn a_reserved_or_inferred_graph_cannot_be_protected() {
    let mut s = Store::open_in_memory().unwrap();
    refused(s.attest_protect_graph("urn:quipu:anything"), "reserved");
    refused(
        s.attest_protect_graph("https://example.org/graph/answers#inferred"),
        "reserved",
    );
    s.attest_protect_graph("https://example.org/graph/answers")
        .unwrap();
}
