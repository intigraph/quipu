//! The grant split on the read surface (aegis-lw5 P4): host and session are
//! shown only to an authenticated reader, and read as the literal "redacted"
//! to anyone else. Each read here goes through the reader identity in scope,
//! the same thread-scoped identity the server installs for a request.

use serde_json::{Value, json};

use super::tests::{FORGED, SIGNED_HEADERS, attested, bearer, field, write};
use crate::Store;
use crate::transaction_auth::{AttestationSlot, Identity, with_identity};

fn legacy() -> Identity {
    Identity {
        principal: "urn:quipu:legacy-shared-bearer".into(),
        credential_id: None,
        auth_class: "legacy_shared_bearer".into(),
        attestation: AttestationSlot::default(),
    }
}

fn reader(store: &Store, tx: i64, who: Option<Identity>) -> (Value, Value) {
    with_identity(who, || {
        let provenance = store.transaction_provenance(tx).unwrap();
        let declared = json!(store.transaction_declarations_disclosed(tx).unwrap());
        (provenance, declared)
    })
}

/// A signed write whose client forged a session: the effective agent and
/// session are attested, the forged session is superseded.
fn signed(store: &mut Store) -> i64 {
    let who = attested(store, "binding-lw5");
    write(store, "test-lw5-signed", SIGNED_HEADERS, who)
}

fn assert_redacted(p: &Value, declared: &Value) {
    assert_eq!(p["effective"]["host"], field("redacted", "declared"));
    assert_eq!(p["effective"]["session"], field("redacted", "attested"));
    assert_eq!(
        p["superseded"],
        json!({ "session": field("redacted", "declared") })
    );
    assert_eq!(declared["host"], "redacted");
    assert_eq!(declared["session"], "redacted");
    // Agent, harness and model are under the actor/source grant: unaffected.
    assert_eq!(
        p["effective"]["agent"],
        field("urn:crew:signer", "attested")
    );
    assert_eq!(p["effective"]["harness"], field("claude", "declared"));
    assert_eq!(p["effective"]["model"], field("model-x", "declared"));
    assert_eq!(declared["harness"], "claude");
    assert_eq!(declared["model"], "model-x");
}

#[test]
fn an_anonymous_read_shows_redacted_for_host_and_session_never_an_omission() {
    let mut store = Store::open_in_memory().unwrap();
    let tx = signed(&mut store);
    let (p, declared) = reader(&store, tx, None);
    assert_redacted(&p, &declared);
}

#[test]
fn a_legacy_shared_bearer_read_shows_redacted_for_host_and_session() {
    let mut store = Store::open_in_memory().unwrap();
    let tx = signed(&mut store);
    let (p, declared) = reader(&store, tx, Some(legacy()));
    assert_redacted(&p, &declared);
}

#[test]
fn a_named_bearer_read_shows_host_and_session_values() {
    let mut store = Store::open_in_memory().unwrap();
    let tx = signed(&mut store);
    let (p, declared) = reader(&store, tx, Some(bearer()));
    assert_eq!(p["effective"]["host"], field("host-a", "declared"));
    assert_eq!(p["effective"]["session"], field("binding-lw5", "attested"));
    assert_eq!(
        p["superseded"],
        json!({ "session": field(FORGED, "declared") })
    );
    assert_eq!(declared["host"], "host-a");
    assert_eq!(declared["session"], FORGED);
}

#[test]
fn an_attested_session_read_shows_host_and_session_values() {
    let mut store = Store::open_in_memory().unwrap();
    let tx = signed(&mut store);
    // The verified identity the signed-read middleware installs.
    let who = Identity {
        principal: "urn:crew:reader".into(),
        credential_id: Some("key-reader".into()),
        auth_class: "attested_session".into(),
        attestation: AttestationSlot::default(),
    };
    let (p, declared) = reader(&store, tx, Some(who));
    assert_eq!(p["effective"]["host"], field("host-a", "declared"));
    assert_eq!(declared["session"], FORGED);
}

/// Redaction runs after classification. A codex bearer write requires all
/// five fields; classifying what an anonymous reader sees (host and session
/// withheld) would read `partial`, and this test would fail.
#[test]
fn completeness_and_state_are_the_same_for_every_reader() {
    let mut store = Store::open_in_memory().unwrap();
    let headers = [
        Some("ian"),
        Some("codex"),
        Some("host-b"),
        Some("sess-b"),
        Some("model-b"),
    ];
    let tx = write(&mut store, "test-lw5-classes", headers, bearer());
    let readers = [None, Some(legacy()), Some(bearer())];
    for who in readers {
        let label = who
            .as_ref()
            .map_or("anonymous".into(), |w| w.auth_class.clone());
        let (p, _) = reader(&store, tx, who);
        assert_eq!(p["completeness"], "complete", "{label}");
        assert_eq!(p["state"], "observed", "{label}");
    }
}
