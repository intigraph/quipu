//! Slice M4 exit proof (aegis-uxja): a mini-Quipu merge carries the mini
//! writer's provenance through the fork merge's carriage code. The merged
//! home transactions keep the APPLIER as principal and the applier's own
//! headers; the mini writer is carried data in the receipt rows and the
//! `carried` object, named by digest in the `receive` source. Forged headers
//! on the merge call change no carried field.
use std::collections::BTreeMap;
use std::sync::Arc;

use super::*;
use crate::transaction_auth::{AttestationSlot, Identity, with_identity};
use crate::write_provenance::{RequestProvenance, scoped};

const WRITER: [Option<&str>; 5] = [
    Some("mini-writer"),
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
fn as_request<T>(headers: [Option<&str>; 5], who: &str, f: impl FnOnce() -> T) -> T {
    let classified = RequestProvenance::classify("", "/merge", headers);
    scoped(Some(Arc::new(classified)), || {
        with_identity(Some(bearer(who)), f)
    })
}

fn forged(session: &'static str) -> [Option<&'static str>; 5] {
    [
        Some("mini-writer"),
        Some("claude"),
        Some("host-forged"),
        Some(session),
        Some("model-forged"),
    ]
}

fn writer_map() -> BTreeMap<String, String> {
    ["agent", "harness", "host", "session", "model"]
        .into_iter()
        .zip(WRITER.map(Option::unwrap))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

/// Home holds x=a at the cut; the mini changes x to c (C1a), written by an
/// authenticated writer with full declarations, plus y=n written with no
/// provenance scope at all. Returns the pair and both mini transactions.
fn fixture(dir: &std::path::Path) -> (Pair, i64, i64) {
    let mut p = pair(dir, &[a("urn:x", ONE, "a", T0)], &[], &[]);
    let d = datums(
        p.mini.store(),
        &[r("urn:x", ONE, "a"), a("urn:x", ONE, "c", TT)],
    );
    let declared = as_request(WRITER, "urn:crew:mini-writer", || {
        p.mini.transact(ROOT_GRAPH_IRI, &d, TT, Some("writer"))
    })
    .unwrap();
    let d = datums(p.mini.store(), &[a("urn:y", MANY, "n", TT)]);
    let bare = p
        .mini
        .transact(ROOT_GRAPH_IRI, &d, TT, Some("bare"))
        .unwrap();
    (p, declared, bare)
}

fn merge_as_applier(p: &mut Pair, headers: [Option<&str>; 5]) -> MergeBack {
    let path = p.path.clone();
    as_request(headers, "urn:crew:applier", || {
        merge_on_legacy(&p.home.catalog, &mut p.home.legacy, &path, &request())
    })
    .unwrap()
}

fn receipt_of(back: MergeBack) -> MergeReceipt {
    match back {
        MergeBack::Merged(r) | MergeBack::AlreadyApplied(r) => r,
        other => panic!("expected a merge, got {other:?}"),
    }
}

#[test]
fn m4_home_principal_is_the_applier_and_the_mini_writer_is_carried() {
    let dir = tempfile::tempdir().unwrap();
    let (mut p, declared, bare) = fixture(dir.path());
    let receipt = receipt_of(merge_as_applier(&mut p, forged("session-forged-1")));
    let home_tx = receipt.receive_tx[0];
    assert_ne!(home_tx, 0, "the clean incoming values are received");

    // Home transaction: the applier's credential and the applier's headers.
    let home = &p.home.legacy;
    let auth = home.transaction_auth(home_tx).unwrap().unwrap();
    assert_eq!(auth.principal, "urn:crew:applier");
    let (own, _) = home.declaration_row(home_tx).unwrap();
    assert_eq!(own.unwrap()["session"], "session-forged-1");
    assert_eq!(
        home.get_transaction(home_tx)
            .unwrap()
            .unwrap()
            .actor
            .as_deref(),
        Some("applier")
    );

    // Carried: the mini transaction's stored declarations, verbatim.
    let carried = receipt.carried.as_ref().expect("an M4 receipt carries");
    assert_eq!(carried.state, "observed");
    let origin = &carried.transactions[&declared.to_string()];
    assert_eq!(origin.state, "observed");
    assert_eq!(origin.declarations.as_ref(), Some(&writer_map()));
    assert_eq!(
        origin.declarations,
        p.mini.store().declaration_row(declared).unwrap().0
    );
    assert_eq!(origin.actor.as_deref(), Some("writer"));
    assert_eq!(origin.source.as_deref(), Some("mini-quipu"));

    // A mini transaction with no declaration row is unknown, never empty.
    let unknown = &carried.transactions[&bare.to_string()];
    assert_eq!(unknown.state, "unknown-no-row");
    assert_eq!(unknown.declarations, None);

    // Decision rows name each slot's origin transactions.
    let rows: BTreeMap<_, _> = receipt
        .rows
        .iter()
        .map(|r| (r.slot.clone(), r.origin_txs.clone()))
        .collect();
    assert_eq!(
        rows[&("<urn:x>".to_owned(), ONE.to_owned())],
        vec![declared]
    );
    assert_eq!(rows[&("<urn:y>".to_owned(), MANY.to_owned())], vec![bare]);
    assert!(receipt.rows.iter().all(|r| r.graph == ROOT_GRAPH_IRI));

    // The receive source names the carried object by digest.
    let digest = crate::share::sha256(serde_json::to_string(carried).unwrap().as_bytes());
    let source = home
        .get_transaction(home_tx)
        .unwrap()
        .unwrap()
        .source
        .unwrap();
    assert!(
        source.contains(&format!(";carried={digest};")),
        "receive source lacks the carried digest: {source}"
    );
}

#[test]
fn m4_forged_merge_headers_change_no_carried_field() {
    let run = |session: &'static str| {
        let dir = tempfile::tempdir().unwrap();
        let (mut p, _, _) = fixture(dir.path());
        let receipt = receipt_of(merge_as_applier(&mut p, forged(session)));
        let own = p
            .home
            .legacy
            .declaration_row(receipt.receive_tx[0])
            .unwrap()
            .0;
        (receipt, own.unwrap()["session"].clone())
    };
    let (one, own_one) = run("session-forged-1");
    let (two, own_two) = run("session-forged-2");
    // The applier's own rows follow the request headers...
    assert_eq!(
        (own_one.as_str(), own_two.as_str()),
        ("session-forged-1", "session-forged-2")
    );
    // ...and nothing carried does.
    assert_eq!(one.carried, two.carried);
    assert_eq!(one.rows, two.rows);
    let text = serde_json::to_string(&one.carried).unwrap();
    assert!(
        !text.contains("forged") && !text.contains("urn:crew:applier"),
        "{text}"
    );
}

#[test]
fn m4_carriage_survives_the_r7_retry_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    let (mut p, _, _) = fixture(dir.path());
    let first = receipt_of(merge_as_applier(&mut p, forged("session-forged-1")));
    match merge_as_applier(&mut p, forged("session-forged-2")) {
        MergeBack::AlreadyApplied(again) => assert_eq!(again, first),
        other => panic!("expected already-applied, got {other:?}"),
    }
}

/// A receipt stored before M4 still loads, and reads as unknown, not empty.
#[test]
fn m4_a_pre_m4_receipt_loads_with_carriage_unknown() {
    let old = r#"{"merge_id":"m","incoming":"s@1#2","receive_tx":[3],"decide_tx":[0],"held":0}"#;
    let receipt: MergeReceipt = serde_json::from_str(old).unwrap();
    assert_eq!(receipt.carried, None);
    assert!(receipt.rows.is_empty());
}
