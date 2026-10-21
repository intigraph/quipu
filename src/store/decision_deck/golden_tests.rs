//! Frozen v1 wire vector for client codecs (aegis-blh.2).
//!
//! The Brief signs `Challenge::webauthn_challenge()` and never builds its own
//! envelope, so a client codec must reproduce these bytes exactly. The expected
//! values were computed independently (compact JSON in field order, then
//! SHA-256 over the verifier domain and the message), not by this code.
//! A change here is a wire break: it needs a new domain, never an edit.

use super::{Challenge, Request};

const MESSAGE: &str = concat!(
    r#"{"domain":"quipu-decision-deck-v1","audience":"store-golden-0001","#,
    r#""nonce":"0000000000000000000000000000000000000000000000000000000000000001","#,
    r#""origin":"https://deck.example.org","rp_id":"deck.example.org","#,
    r#""actor":"actor@example.org","credential":"Z29sZGVuLWNyZWRlbnRpYWw","#, // gitleaks:allow (base64 of "golden-credential")
    r#""request":{"id":"https://example.org/decision/request/golden-1","revision":3,"#,
    r#""policy":"https://example.org/policy/brief","target":"https://example.org/item/golden","#,
    r#""summary":"Approve the golden vector fixture","expires_at":1792600000,"#,
    r#""choices":["approve","reject","changes"],"author":"curator@example.org"},"#,
    r#""choice":"approve","expires_at":1792599700}"#,
);
const CHALLENGE_HEX: &str = "f1ec505226691126f20bb3088ada5d655fe5fabc87a260a75def067747cae064";

fn golden() -> Challenge {
    Challenge {
        domain: "quipu-decision-deck-v1".into(),
        audience: "store-golden-0001".into(),
        nonce: format!("{}01", "00".repeat(31)),
        origin: "https://deck.example.org".into(),
        rp_id: "deck.example.org".into(),
        actor: "actor@example.org".into(),
        credential: "Z29sZGVuLWNyZWRlbnRpYWw".into(), // gitleaks:allow
        request: Request {
            id: "https://example.org/decision/request/golden-1".into(),
            revision: 3,
            policy: "https://example.org/policy/brief".into(),
            target: "https://example.org/item/golden".into(),
            summary: "Approve the golden vector fixture".into(),
            expires_at: 1_792_600_000,
            choices: vec!["approve".into(), "reject".into(), "changes".into()],
            author: "curator@example.org".into(),
        },
        choice: "approve".into(),
        expires_at: 1_792_599_700,
    }
}

#[test]
fn v1_message_bytes_are_frozen() {
    assert_eq!(
        String::from_utf8(golden().message().unwrap()).unwrap(),
        MESSAGE
    );
}

#[test]
fn v1_webauthn_challenge_is_frozen() {
    assert_eq!(
        hex::encode(golden().webauthn_challenge().unwrap()),
        CHALLENGE_HEX
    );
}

#[test]
fn the_vector_decodes_back_to_the_same_challenge() {
    let decoded: Challenge = serde_json::from_str(MESSAGE).unwrap();
    assert_eq!(decoded, golden());
}
