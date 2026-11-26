//! P6 exit proof: on a fork merge the home transactions belong to the applier,
//! the receipt carries the fork writer's declarations verbatim, and forged
//! headers on the merge call change no carried field.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::merge_session::Profile;
use crate::store::{Datum, Store};
use crate::transaction_auth::{AttestationSlot, Identity, with_identity};
use crate::types::{Op, Value};
use crate::write_provenance::{RequestProvenance, scoped};

const TS: &str = "2026-01-02T00:00:00Z";
const TD: &str = "2026-01-10T00:00:00Z";
const LEDGER: &str = "urn:fixture:ledger";
const WRITER: [Option<&str>; 5] = [
    Some("fork-writer"),
    Some("claude"),
    Some("host-w"),
    Some("session-w"),
    Some("model-w"),
];

fn bearer(principal: &str) -> Identity {
    Identity {
        principal: principal.into(),
        credential_id: Some(format!("cred-{principal}")),
        auth_class: "named_bearer".into(),
        attestation: AttestationSlot::default(),
    }
}

/// Run `f` the way the server runs a request: classified headers in scope,
/// under an authenticated identity.
fn as_request<T>(headers: [Option<&str>; 5], who: Identity, f: impl FnOnce() -> T) -> T {
    let classified = RequestProvenance::classify("", "/merge", headers);
    scoped(Some(Arc::new(classified)), || with_identity(Some(who), f))
}

fn datum(store: &mut Store, p: &str, v: &str) -> Datum {
    Datum {
        entity: store.intern("urn:e").unwrap(),
        attribute: store.intern(p).unwrap(),
        value: Value::Str(v.into()),
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    }
}

/// ROOT holds one fact; a fork, written by an authenticated writer with full
/// declarations, adds a value on a slot ROOT never had. Returns the fork tx.
fn fixture() -> (Store, i64) {
    let mut store = Store::open_in_memory().unwrap();
    let root = [datum(&mut store, "urn:p", "base")];
    store.transact(&root, TS, Some("fixture"), None).unwrap();
    let at = store.latest_tx_id().unwrap();
    store.fork_create("f", at, TS, Some("fixture")).unwrap();
    let g = store.fork_graph_for_read("f").unwrap();
    let theirs = [datum(&mut store, "urn:q", "theirs")];
    let tx = as_request(WRITER, bearer("urn:crew:writer"), || {
        store.transact_to_graph(&theirs, TS, Some("writer"), Some("fork-edit"), g)
    })
    .unwrap();
    (store, tx)
}

type Prepared = (crate::merge_session::Prepared, String);

fn prepare(store: &Store) -> Prepared {
    let (request, authored) =
        crate::merge_adapters::fork_request(store, "f", Profile::default()).unwrap();
    let prepared =
        crate::merge_adapters::prepare_store(store, request, &[], &authored, TD).unwrap();
    (prepared, authored)
}

/// Apply a prepared fork merge as the applier, under `headers`.
fn apply(
    store: &mut Store,
    (prepared, authored): Prepared,
    headers: [Option<&str>; 5],
) -> crate::merge_pending::Receipt {
    as_request(headers, bearer("urn:crew:applier"), || {
        crate::merge_adapters::apply_store(store, LEDGER, prepared, &authored, TD, Some("applier"))
    })
    .unwrap()
}

fn merge(store: &mut Store, headers: [Option<&str>; 5]) -> crate::merge_pending::Receipt {
    let prepared = prepare(store);
    apply(store, prepared, headers)
}

fn forged(session: &'static str) -> [Option<&'static str>; 5] {
    [
        Some("fork-writer"),
        Some("claude"),
        Some("host-forged"),
        Some(session),
        Some("model-forged"),
    ]
}

fn expected_writer() -> BTreeMap<String, String> {
    ["agent", "harness", "host", "session", "model"]
        .into_iter()
        .zip(WRITER.map(Option::unwrap))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

#[test]
fn fork_merge_carries_the_fork_writer_and_keeps_the_applier_as_home_principal() {
    let (mut store, fork_tx) = fixture();
    let receipt = merge(&mut store, forged("session-forged-1"));
    let home = receipt
        .receive_tx
        .expect("the clean incoming value is received");

    // Home transaction: the applier's credential and the applier's own headers.
    let auth = store.transaction_auth(home).unwrap().unwrap();
    assert_eq!(auth.principal, "urn:crew:applier");
    let (own, _) = store.declaration_row(home).unwrap();
    assert_eq!(own.unwrap()["session"], "session-forged-1");

    // Carried: the fork transaction's stored declarations, verbatim.
    let carried = receipt.carried.as_ref().unwrap();
    assert_eq!(carried.state, "observed");
    let origin = &carried.transactions[&fork_tx.to_string()];
    assert_eq!(origin.state, "observed");
    assert_eq!(origin.declarations.as_ref(), Some(&expected_writer()));
    assert_eq!(
        origin.declarations,
        store.declaration_row(fork_tx).unwrap().0
    );
    assert_eq!(origin.actor.as_deref(), Some("writer"));
    assert_eq!(origin.source.as_deref(), Some("fork-edit"));
    let row = receipt
        .rows
        .iter()
        .find(|r| r.slot.1 == "urn:q")
        .expect("the received slot has a decision row");
    assert_eq!(row.origin_txs, vec![fork_tx]);
    assert!(
        !carried.transactions.contains_key(&home.to_string()),
        "a home transaction is never its own origin"
    );

    // The stored receipt carries the same object.
    // The stored receipt, read back as its writer, carries the same object.
    let stored = with_identity(Some(bearer("urn:crew:applier")), || {
        crate::merge_pending::load(&store, LEDGER, &receipt.id)
    })
    .unwrap()
    .unwrap();
    assert_eq!(stored.carried, receipt.carried);
}

#[test]
fn forged_headers_on_the_merge_change_no_carried_field() {
    // Repeating the merge with different forged headers returns the recorded
    // receipt: nothing carried moves.
    let (mut store, _) = fixture();
    let prepared = prepare(&store);
    let first = apply(&mut store, prepared.clone(), forged("session-forged-1"));
    let again = apply(&mut store, prepared, forged("session-forged-2"));
    assert_eq!(again.id, first.id);
    assert_eq!(again.carried, first.carried);

    // The same merge applied fresh under other forged headers carries the
    // same fields: headers on the merge call never reach the carriage.
    let (mut other, _) = fixture();
    let fresh = merge(&mut other, forged("session-forged-2"));
    assert_eq!(fresh.carried, first.carried);
    let rows = |r: &crate::merge_pending::Receipt| {
        r.rows
            .iter()
            .map(|row| row.origin_txs.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(rows(&fresh), rows(&first));
    let home = fresh.receive_tx.unwrap();
    assert_eq!(
        other.declaration_row(home).unwrap().0.unwrap()["session"],
        "session-forged-2",
        "the forged header lands only on the applier's own row"
    );
}

#[test]
fn a_receipt_written_before_carriage_reads_as_unknown_not_empty() {
    let legacy: crate::merge_pending::Receipt = {
        let (mut store, _) = fixture();
        let receipt = merge(&mut store, forged("s"));
        let mut wire = serde_json::to_value(&receipt).unwrap();
        wire.as_object_mut().unwrap().remove("carried");
        for row in wire["rows"].as_array_mut().unwrap() {
            row.as_object_mut().unwrap().remove("origin_txs");
        }
        serde_json::from_value(wire).unwrap()
    };
    assert!(legacy.carried.is_none());
    assert!(legacy.rows.iter().all(|r| r.origin_txs.is_empty()));
}
