//! The read-side provenance object (aegis-7tt P3): precedence, the unknown
//! states, and one test that drives the metric and the read from one request.

use std::sync::Arc;

use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};

use crate::Store;
use crate::session_attestation::SessionBinding;
use crate::transaction_auth::{AttestationSlot, Identity, PendingAttestation, with_identity};
use crate::write_provenance::{RequestProvenance, scoped};
use crate::write_provenance_read::Disclosure;

const TS: &str = "2026-01-01T00:00:00Z";
const NONCE: &str = "fedcba9876543210fedcba9876543210";
/// A forged session header on a signed write: never the effective session.
pub(super) const FORGED: &str = "forged-session";
/// Headers in `FIELDS` order (agent, harness, host, session, model): a signed
/// client omits the agent, and this one also forges a session.
pub(super) const SIGNED_HEADERS: [Option<&str>; 5] = [
    None,
    Some("claude"),
    Some("host-a"),
    Some(FORGED),
    Some("model-x"),
];

pub(super) fn attested(store: &Store, session: &str) -> Identity {
    let key = Ed25519KeyPair::from_seed_unchecked(&[42u8; 32]).unwrap();
    let now = crate::time::epoch_secs();
    let mut binding = SessionBinding::new(
        "urn:crew:signer",
        session,
        hex::encode(key.public_key().as_ref()),
        "urn:crew:lead",
        now - 60,
        now + 3600,
    )
    .unwrap();
    binding.allow_write = true;
    store.attestation_register(&binding).unwrap();
    let pending = PendingAttestation::new(
        binding.session.clone(),
        NONCE.to_owned(),
        binding.key_id.clone(),
        binding.introducer.clone(),
    );
    Identity {
        principal: "urn:crew:signer".into(),
        credential_id: None,
        auth_class: "attested_session".into(),
        attestation: AttestationSlot(Some(pending)),
    }
}

pub(super) fn bearer() -> Identity {
    Identity {
        principal: "urn:crew:bearer".into(),
        credential_id: Some("cred-1".into()),
        auth_class: "named_bearer".into(),
        attestation: AttestationSlot::default(),
    }
}

/// Commit one transaction the way the server does: the middleware's
/// classification, credited only for a verified attestation, under `identity`.
pub(super) fn write(
    store: &mut Store,
    client: &str,
    headers: [Option<&str>; 5],
    who: Identity,
) -> i64 {
    let classified = RequestProvenance::classify(client, "/knot", headers);
    let credited = if who.auth_class == "attested_session" {
        classified.with_verified_identity()
    } else {
        classified
    };
    scoped(Some(Arc::new(credited)), || {
        with_identity(Some(who), || store.transact(&[], TS, None, None))
    })
    .unwrap()
}

fn series(client: &str, class: &str) -> u64 {
    let text = crate::metrics::metrics().render(0, 0, 0, None);
    let key = format!(
        "quipu_write_provenance_total{{client=\"{client}\",endpoint=\"/knot\",provenance=\"{class}\"}} "
    );
    text.lines()
        .find_map(|l| l.strip_prefix(&key))
        .map_or(0, |n| n.parse().unwrap())
}

pub(super) fn field(value: &str, basis: &str) -> Value {
    json!({ "value": value, "basis": basis })
}

#[test]
fn a_signed_write_with_a_forged_session_reads_the_binding_session_as_effective() {
    let mut store = Store::open_in_memory().unwrap();
    let who = attested(&store, "binding-s1");
    let tx = write(&mut store, "test-d4yw-forged", SIGNED_HEADERS, who);
    let p = store
        .transaction_provenance_for(tx, Disclosure::Full)
        .unwrap();
    assert_eq!(p["state"], "observed");
    assert_eq!(
        p["effective"]["agent"],
        field("urn:crew:signer", "attested")
    );
    assert_eq!(p["effective"]["session"], field("binding-s1", "attested"));
    assert_eq!(
        p["superseded"],
        json!({ "session": field(FORGED, "declared") })
    );
    for (name, value) in [
        ("harness", "claude"),
        ("host", "host-a"),
        ("model", "model-x"),
    ] {
        assert_eq!(p["effective"][name], field(value, "declared"), "{name}");
    }
    // The verified binding supplies the agent the signed client omitted.
    assert_eq!(p["completeness"], "complete");
    // The existing keys are unchanged: `declared` still holds the raw header.
    assert_eq!(
        store.transaction_declarations(tx).unwrap().unwrap()["session"],
        FORGED
    );
}

#[test]
fn a_bearer_write_reads_every_field_as_declared() {
    let mut store = Store::open_in_memory().unwrap();
    let headers = [
        Some("ian"),
        Some("codex"),
        Some("host-b"),
        Some("sess-b"),
        Some("model-b"),
    ];
    let tx = write(&mut store, "test-d4yw-bearer", headers, bearer());
    let p = store
        .transaction_provenance_for(tx, Disclosure::Full)
        .unwrap();
    assert_eq!(p["superseded"], json!({}));
    let effective = p["effective"].as_object().unwrap();
    assert_eq!(effective.len(), 5);
    assert!(effective.values().all(|f| f["basis"] == "declared"));
    assert_eq!(p["effective"]["agent"], field("ian", "declared"));
    assert_eq!(p["effective"]["session"], field("sess-b", "declared"));
    assert_eq!(p["completeness"], "complete");
}

#[test]
fn a_transaction_with_no_declaration_row_reads_unknown_no_row() {
    let mut store = Store::open_in_memory().unwrap();
    // No request scope: an engine write, which records no declaration row.
    let tx = store.transact(&[], TS, None, None).unwrap();
    let p = store
        .transaction_provenance_for(tx, Disclosure::Full)
        .unwrap();
    assert_eq!(p["state"], "unknown-no-row");
    assert_eq!(p["completeness"], "unknown");
    assert_eq!(p["effective"], json!({}));
}

#[test]
fn a_read_only_open_of_a_store_without_the_table_reads_unknown_no_table() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    let path = path.to_str().unwrap();
    let tx = {
        let mut store = Store::open(path).unwrap();
        let tx = store.transact(&[], TS, None, None).unwrap();
        // A store from before P1 never had the table.
        store
            .conn
            .execute_batch("DROP TABLE transaction_declarations;")
            .unwrap();
        tx
    };
    let store = Store::open_read_only(path).unwrap();
    let p = store
        .transaction_provenance_for(tx, Disclosure::Full)
        .unwrap();
    assert_eq!(p["state"], "unknown-no-table");
    assert_eq!(p["completeness"], "unknown");
}

/// One request drives both the metric and the read: the class the metric
/// counted is the class `/transactions` reports. Reading without
/// `with_verified_identity` would say `partial` (no agent header) and fail.
#[test]
fn the_read_completeness_equals_the_metric_class_for_the_same_request() {
    const CLIENT: &str = "test-d4yw-equal";
    let mut store = Store::open_in_memory().unwrap();
    let who = attested(&store, "binding-s2");
    let tx = write(&mut store, CLIENT, SIGNED_HEADERS, who);
    let read = store
        .transaction_provenance_for(tx, Disclosure::Full)
        .unwrap();
    let class = read["completeness"].as_str().unwrap();
    assert_eq!(series(CLIENT, class), 1, "metric counted {class}");
    for other in ["complete", "partial", "absent"] {
        if other != class {
            assert_eq!(series(CLIENT, other), 0, "metric also counted {other}");
        }
    }
    assert_eq!(class, "complete");
}
