//! The JSON transport over the native attested write (aegis-25k S2): what a
//! client posts to `/attest/*` and what it gets back.
use serde_json::json;

use super::tests::{OUT, SAID, SLOT, footprint, proposal, ready, sign};
use super::*;
use crate::verdict_schemes::webauthn_tests::Authenticator;
use crate::{tool_attest_challenge, tool_attest_commit, tool_attest_receipt};

fn b64(data: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

fn commit_body(
    nonce: &str,
    signed: &crate::verdict_schemes::webauthn_tests::Signed,
) -> serde_json::Value {
    let (authenticator_data, client_data_json, signature) = signed.wire();
    json!({"nonce": nonce, "client_data_json": client_data_json,
           "authenticator_data": authenticator_data, "signature": signature})
}

/// Challenge, sign exactly what came back, commit, then look the receipt up:
/// the client never rebuilds the signed bytes.
#[test]
fn a_client_signs_the_returned_message_and_reads_its_receipt() {
    let a = Authenticator::es256();
    let mut s = ready(&a);
    let issued =
        tool_attest_challenge(&mut s, &serde_json::to_value(proposal("tonight")).unwrap()).unwrap();
    let challenge: Challenge = serde_json::from_value(issued["challenge"].clone()).unwrap();
    assert_eq!(issued["message"], b64(&challenge.message().unwrap()));
    assert_eq!(
        issued["webauthn_challenge"],
        b64(&challenge.webauthn_challenge().unwrap())
    );
    let before = footprint(&s);
    let committed = tool_attest_commit(
        &mut s,
        &commit_body(&challenge.nonce, &sign(&a, &challenge, 8)),
    )
    .unwrap();
    assert_eq!(committed["nonce"], challenge.nonce);
    assert_eq!(committed["receipt_iri"], receipt_iri(&challenge.nonce));
    assert_eq!(
        committed["content_hash"],
        crate::share::sha256(&challenge.message().unwrap())
    );
    // One receipt, one consumed challenge, and the datum plus its
    // server-authored provenance in the attested graph.
    let after = footprint(&s);
    assert_eq!((after.0, after.1), (before.0 + 1, before.1 + 1));
    assert!(after.2 > before.2, "{before:?} -> {after:?}");
    let looked = tool_attest_receipt(&s, &json!({"nonce": challenge.nonce})).unwrap();
    assert_eq!(looked["found"], true);
    assert_eq!(looked["verified"], true);
    assert!(looked["tx"].as_i64().is_some());
    assert_eq!(
        looked["receipt"]["challenge"]["proposal"]["datums"][0]["s"],
        SLOT
    );
    // `verified` is a re-check, not a constant: a retained signature altered
    // after admission still reads as found, and no longer as verified.
    let mut tampered: Receipt = serde_json::from_value(looked["receipt"].clone()).unwrap();
    tampered.signature[8] ^= 0x01;
    s.conn
        .execute(
            "UPDATE attest_receipts SET data=?1 WHERE nonce=?2",
            rusqlite::params![encode(&tampered).unwrap(), challenge.nonce],
        )
        .unwrap();
    let altered = tool_attest_receipt(&s, &json!({"nonce": challenge.nonce})).unwrap();
    assert_eq!(altered["found"], true);
    assert_eq!(altered["verified"], false);
    // An unknown nonce is an absence, not an error.
    let missing = tool_attest_receipt(&s, &json!({"nonce": "0".repeat(64)})).unwrap();
    assert_eq!(missing["found"], false);
    assert_eq!(missing["verified"], false);
}

/// A replayed commit, a commit over content other than the stored challenge,
/// and malformed bodies all record nothing.
#[test]
fn replay_forgery_and_malformed_bodies_record_nothing() {
    let a = Authenticator::es256();
    let mut s = ready(&a);
    let issued =
        tool_attest_challenge(&mut s, &serde_json::to_value(proposal("tonight")).unwrap()).unwrap();
    let c: Challenge = serde_json::from_value(issued["challenge"].clone()).unwrap();
    let before = footprint(&s);
    let forged = Challenge {
        proposal: proposal("weekend"),
        ..c.clone()
    };
    assert!(tool_attest_commit(&mut s, &commit_body(&c.nonce, &sign(&a, &forged, 8))).is_err());
    for body in [
        json!({"nonce": c.nonce}),
        json!({"nonce": c.nonce, "client_data_json": "!", "authenticator_data": "", "signature": ""}),
        json!({"nonce": c.nonce, "client_data_json": "", "authenticator_data": "", "signature": "",
               "datums": []}),
    ] {
        assert!(tool_attest_commit(&mut s, &body).is_err(), "{body}");
    }
    // A proposal with a field the server does not know is refused outright.
    let mut extra = serde_json::to_value(proposal("x")).unwrap();
    extra["note"] = json!("unsigned");
    assert!(tool_attest_challenge(&mut s, &extra).is_err());
    assert_eq!(footprint(&s), before);
    tool_attest_commit(&mut s, &commit_body(&c.nonce, &sign(&a, &c, 8))).unwrap();
    let after = footprint(&s);
    let replay = tool_attest_commit(&mut s, &commit_body(&c.nonce, &sign(&a, &c, 9)));
    assert!(replay.unwrap_err().to_string().contains("replayed"));
    assert_eq!(footprint(&s), after);
}

/// A live precondition that changed after the challenge refuses the commit,
/// and nothing lands in the attested graph.
#[test]
fn a_precondition_that_changed_after_the_challenge_refuses_the_commit() {
    let a = Authenticator::es256();
    let mut s = ready(&a);
    let issued =
        tool_attest_challenge(&mut s, &serde_json::to_value(proposal("tonight")).unwrap()).unwrap();
    let c: Challenge = serde_json::from_value(issued["challenge"].clone()).unwrap();
    // Another writer fills the expected-empty slot first.
    let other =
        tool_attest_challenge(&mut s, &serde_json::to_value(proposal("earlier")).unwrap()).unwrap();
    let o: Challenge = serde_json::from_value(other["challenge"].clone()).unwrap();
    tool_attest_commit(&mut s, &commit_body(&o.nonce, &sign(&a, &o, 8))).unwrap();
    let before = footprint(&s);
    let stale = tool_attest_commit(&mut s, &commit_body(&c.nonce, &sign(&a, &c, 9)));
    assert!(stale.is_err(), "{stale:?}");
    assert_eq!(footprint(&s), before);
    let g = s.registered_graph_id(OUT).unwrap().unwrap();
    let said = s.lookup(SAID).unwrap().unwrap();
    let values: Vec<_> = s
        .current_facts_in_graph(g)
        .unwrap()
        .into_iter()
        .filter(|f| f.attribute == said)
        .map(|f| f.value)
        .collect();
    assert_eq!(values, vec![Value::Str("earlier".into())]);
}

/// The frozen `quipu-attested-write-v1` vector. The expected bytes and both
/// digests were computed outside this crate (Python `json.dumps` with the
/// same field order and compact separators), so a change to the signed
/// encoding fails here instead of silently breaking every client.
#[test]
fn the_v1_signed_message_is_frozen() {
    let datum = |p: &str, o: TypedValue| AttestedDatum {
        s: "https://example.org/answer/1".into(),
        p: p.into(),
        o,
    };
    let challenge = Challenge {
        domain: DOMAIN.into(),
        audience: "golden-store".into(),
        nonce: "0123456789abcdef".repeat(4),
        origin: "https://sign.example.org".into(),
        rp_id: "sign.example.org".into(),
        actor: "golden-human".into(),
        proposal: Proposal {
            signer: "Z29sZGVu".into(),
            graph: "https://example.org/graph/answers".into(),
            datums: vec![
                datum(
                    "https://example.org/said",
                    TypedValue::Str("approve".into()),
                ),
                datum("https://example.org/count", TypedValue::Int(3)),
                datum(
                    "https://example.org/about",
                    TypedValue::Iri("https://example.org/item/1".into()),
                ),
            ],
            expect: vec![
                Expect {
                    graph: "https://example.org/graph/items".into(),
                    s: "https://example.org/item/1".into(),
                    p: "https://example.org/status".into(),
                    o: Some(TypedValue::Str("open".into())),
                },
                Expect {
                    graph: "https://example.org/graph/answers".into(),
                    s: "https://example.org/answer/1".into(),
                    p: "https://example.org/said".into(),
                    o: None,
                },
            ],
        },
        expires_at: 1_792_900_000,
    };
    let message = challenge.message().unwrap();
    assert_eq!(
        String::from_utf8(message.clone()).unwrap(),
        concat!(
            r#"{"domain":"quipu-attested-write-v1","audience":"golden-store","#,
            r#""nonce":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","#,
            r#""origin":"https://sign.example.org","rp_id":"sign.example.org","actor":"golden-human","#,
            r#""proposal":{"signer":"Z29sZGVu","graph":"https://example.org/graph/answers","#,
            r#""datums":[{"s":"https://example.org/answer/1","p":"https://example.org/said","o":{"kind":"str","value":"approve"}},"#,
            r#"{"s":"https://example.org/answer/1","p":"https://example.org/count","o":{"kind":"int","value":3}},"#,
            r#"{"s":"https://example.org/answer/1","p":"https://example.org/about","o":{"kind":"iri","value":"https://example.org/item/1"}}],"#,
            r#""expect":[{"graph":"https://example.org/graph/items","s":"https://example.org/item/1","p":"https://example.org/status","o":{"kind":"str","value":"open"}},"#,
            r#"{"graph":"https://example.org/graph/answers","s":"https://example.org/answer/1","p":"https://example.org/said","o":null}]},"#,
            r#""expires_at":1792900000}"#,
        )
    );
    assert_eq!(message.len(), 939);
    assert_eq!(
        crate::share::sha256(&message),
        "sha256:70c6e46b205c0917aedd8099aed098d885f3a448ce1b762b0a373991fa671677"
    );
    assert_eq!(
        b64(&challenge.webauthn_challenge().unwrap()),
        "RP-DQx0CPO0OjO3W7_mX1O7b658eAAYpicqPr-pKjdc"
    );
}
