//! Deck transport (aegis-blh.2) against a software authenticator: the JSON
//! layer must carry exactly the native refusals and add no authority of its own.

use serde_json::{Value as JsonValue, json};

use super::{tool_deck_attest, tool_deck_present, tool_deck_receipt};
use crate::store::Store;
use crate::store::decision_deck::{Config, Credential, Request};
use crate::verdict_schemes::webauthn_tests::{Authenticator, Ceremony, ORIGIN, RP_ID};

const REQUEST: &str = "urn:test:deck-request";
const CREDENTIAL: &str = "dGVzdA";

fn now() -> i64 {
    i64::try_from(crate::time::epoch_secs()).unwrap()
}

fn store(auth: &Authenticator, enabled: bool) -> Store {
    let mut store = Store::open_in_memory().unwrap();
    store
        .deck_configure(&Config {
            origin: ORIGIN.into(),
            rp_id: RP_ID.into(),
            actor: "test-human".into(),
            author: "test-curator".into(),
        })
        .unwrap();
    store
        .deck_enroll(&Credential {
            id: CREDENTIAL.into(),
            scheme: auth.scheme().tag().into(),
            cose_key: auth.cose_key(),
            expires_at: now() + 3600,
        })
        .unwrap();
    store
        .deck_put_request(&Request {
            id: REQUEST.into(),
            revision: 1,
            policy: "test-only-policy".into(),
            target: "urn:test:target".into(),
            summary: "Approve the harmless fixture?".into(),
            expires_at: now() + 3600,
            choices: vec!["approve".into(), "reject".into()],
            author: "test-curator".into(),
            labels: Vec::new(),
            actions: Vec::new(),
            expect: Vec::new(),
        })
        .unwrap();
    store.deck_set_enabled(enabled).unwrap();
    store
}

fn present(store: &mut Store) -> JsonValue {
    tool_deck_present(
        store,
        &json!({"request": REQUEST, "revision": 1, "credential": CREDENTIAL, "choice": "approve"}),
    )
    .unwrap()
}

/// Sign what the server returned, exactly as a browser would.
fn attest_body(auth: &Authenticator, presented: &JsonValue) -> JsonValue {
    let challenge: crate::store::decision_deck::Challenge =
        serde_json::from_value(presented["challenge"].clone()).unwrap();
    let signed = auth.sign(&Ceremony::for_message(&challenge.message().unwrap()));
    let (authenticator_data, client_data_json, signature) = signed.wire();
    json!({
        "nonce": challenge.nonce,
        "client_data_json": client_data_json,
        "authenticator_data": authenticator_data,
        "signature": signature,
    })
}

fn refused(r: crate::error::Result<JsonValue>, fragment: &str) {
    let e = r.unwrap_err().to_string();
    assert!(e.contains(fragment), "{e} lacks {fragment}");
}

#[test]
fn present_sign_attest_and_receipt_round_trip() {
    for auth in [Authenticator::es256(), Authenticator::ed25519()] {
        let mut store = store(&auth, true);
        let presented = present(&mut store);
        // The returned webauthn challenge is the one derived from the returned bytes.
        let challenge: crate::store::decision_deck::Challenge =
            serde_json::from_value(presented["challenge"].clone()).unwrap();
        let derived = crate::verdict_schemes::b64url_decode(
            presented["webauthn_challenge"].as_str().unwrap(),
        )
        .unwrap();
        assert_eq!(derived, challenge.webauthn_challenge().unwrap().to_vec());

        let body = attest_body(&auth, &presented);
        let settled = tool_deck_attest(&mut store, &body).unwrap();
        assert_eq!(settled["nonce"], body["nonce"]);
        assert!(
            settled["decision"]
                .as_str()
                .unwrap()
                .starts_with("urn:quipu:decision:")
        );

        let found = tool_deck_receipt(&store, &json!({"nonce": body["nonce"]})).unwrap();
        assert_eq!(found["found"], true);
        assert_eq!(found["receipt"]["decision"], settled["decision"]);
        assert!(found["tx"].as_i64().unwrap() > 0);

        // A lost response is reconciled by lookup; resubmitting is refused.
        refused(tool_deck_attest(&mut store, &body), "replayed challenge");
    }
}

#[test]
fn transport_refuses_what_the_native_layer_refuses() {
    let auth = Authenticator::es256();
    let mut store = store(&auth, true);
    let presented = present(&mut store);
    let body = attest_body(&auth, &presented);

    let mut unknown = body.clone();
    unknown["nonce"] = json!("0".repeat(64));
    refused(tool_deck_attest(&mut store, &unknown), "unknown challenge");

    let mut garbled = body.clone();
    garbled["signature"] = json!("not base64url!");
    refused(
        tool_deck_attest(&mut store, &garbled),
        "signature is not base64url",
    );

    let mut extra = body.clone();
    extra["snapshotTx"] = json!(7);
    refused(tool_deck_attest(&mut store, &extra), "unknown field");
    refused(
        tool_deck_present(
            &mut store,
            &json!({"request": REQUEST, "revision": 1, "credential": CREDENTIAL,
                    "choice": "approve", "snapshot_tx": 7}),
        ),
        "unknown field",
    );

    // A signature over bytes the client built itself does not verify.
    let mut forged: crate::store::decision_deck::Challenge =
        serde_json::from_value(presented["challenge"].clone()).unwrap();
    forged.choice = "reject".into();
    let signed = auth.sign(&Ceremony::for_message(&forged.message().unwrap()));
    let (authenticator_data, client_data_json, signature) = signed.wire();
    let forged_body = json!({"nonce": body["nonce"], "client_data_json": client_data_json,
                             "authenticator_data": authenticator_data, "signature": signature});
    assert!(tool_deck_attest(&mut store, &forged_body).is_err());

    // The refusals above consumed nothing: the genuine assertion still settles.
    tool_deck_attest(&mut store, &body).unwrap();
    let absent = tool_deck_receipt(&store, &json!({"nonce": "1".repeat(64)})).unwrap();
    assert_eq!(absent["found"], false);
}

#[test]
fn disabled_deck_issues_nothing() {
    let auth = Authenticator::es256();
    let mut store = store(&auth, false);
    refused(
        tool_deck_present(
            &mut store,
            &json!({"request": REQUEST, "revision": 1, "credential": CREDENTIAL, "choice": "approve"}),
        ),
        "disabled",
    );
}

/// Wire v2 through the transport: the action and comment reach the signed
/// challenge, and the browser signs exactly what the server returned.
#[test]
fn a_v2_later_with_a_comment_round_trips_through_the_transport() {
    let auth = Authenticator::es256();
    let mut store = store(&auth, true);
    store
        .deck_put_request(&Request {
            id: "urn:test:v2".into(),
            revision: 1,
            policy: "test-only-policy".into(),
            target: "urn:test:item".into(),
            summary: "Which window?".into(),
            expires_at: now() + 3600,
            choices: Vec::new(),
            author: "test-curator".into(),
            labels: vec!["Tonight".into()],
            actions: vec!["choose".into(), "later".into()],
            expect: Vec::new(),
        })
        .unwrap();
    let presented = tool_deck_present(
        &mut store,
        &json!({"request": "urn:test:v2", "revision": 1, "credential": CREDENTIAL,
                "action": "later", "comment": "after the backup"}),
    )
    .unwrap();
    assert_eq!(presented["challenge"]["domain"], "quipu-decision-deck-v2");
    assert_eq!(presented["challenge"]["action"], "later");
    assert_eq!(presented["challenge"]["comment"], "after the backup");
    let settled = tool_deck_attest(&mut store, &attest_body(&auth, &presented)).unwrap();
    assert!(
        settled["decision"]
            .as_str()
            .unwrap()
            .starts_with("urn:quipu:decision:")
    );
    // Unknown fields are still refused.
    refused(
        tool_deck_present(
            &mut store,
            &json!({"request": "urn:test:v2", "revision": 1, "credential": CREDENTIAL,
                    "action": "later", "note": "x"}),
        ),
        "unknown field",
    );
}
