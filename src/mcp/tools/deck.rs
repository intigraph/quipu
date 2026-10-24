//! Native decision-deck transport (aegis-blh.2): present, attest, receipt.
//!
//! The deck's authority stays in `store::decision_deck`: these functions only
//! decode a strict JSON request, call the atomic native method, and encode its
//! result. The client signs the challenge THIS response returns, never one it
//! rebuilds; the server re-derives the signed bytes from the stored challenge
//! at attest. A refused or indeterminate attest is never retried here: the
//! caller looks up the receipt by nonce instead.

use serde::Deserialize;
use serde_json::{Value as JsonValue, json};

use crate::error::{Error, Result};
use crate::store::Store;
use crate::verdict_schemes::{b64url_decode, webauthn};

fn input<T: for<'a> Deserialize<'a>>(value: &JsonValue) -> Result<T> {
    serde_json::from_value(value.clone()).map_err(|e| Error::InvalidValue(format!("deck: {e}")))
}

fn bytes(field: &str, value: &str) -> Result<Vec<u8>> {
    b64url_decode(value).map_err(|_| Error::InvalidValue(format!("deck: {field} is not base64url")))
}

fn b64url(data: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Present {
    request: String,
    revision: u32,
    credential: String,
    #[serde(default)]
    choice: String,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    comment: Option<String>,
}

/// Issue one challenge: `{request, revision, credential, choice?, action?,
/// comment?}` -> `{challenge, webauthn_challenge}`, the latter unpadded
/// base64url. A v1 request takes `choice` only; a v2 request takes `action`,
/// with `choice` for `choose` and an optional (for `comment`, required)
/// `comment`.
///
/// # Errors
/// Malformed input, or any refusal of `Store::deck_present_action`.
pub fn tool_deck_present(store: &mut Store, value: &JsonValue) -> Result<JsonValue> {
    let p: Present = input(value)?;
    let challenge = store.deck_present_action(
        &p.request,
        p.revision,
        &p.credential,
        p.action.as_deref(),
        &p.choice,
        p.comment.as_deref(),
    )?;
    Ok(json!({
        "webauthn_challenge": b64url(&challenge.webauthn_challenge()?),
        "challenge": challenge,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Attest {
    nonce: String,
    client_data_json: String,
    authenticator_data: String,
    signature: String,
}

/// Verify and settle one challenge: `{nonce, client_data_json,
/// authenticator_data, signature}` (base64url) -> `{decision, nonce,
/// recorded_at}`.
///
/// # Errors
/// Malformed input, or any refusal of `Store::deck_attest`. After an
/// indeterminate response, call [`tool_deck_receipt`]; never resubmit.
pub fn tool_deck_attest(store: &mut Store, value: &JsonValue) -> Result<JsonValue> {
    let a: Attest = input(value)?;
    let client_data_json = bytes("client_data_json", &a.client_data_json)?;
    let authenticator_data = bytes("authenticator_data", &a.authenticator_data)?;
    let signature = bytes("signature", &a.signature)?;
    let receipt = store.deck_attest(
        &a.nonce,
        &webauthn::Assertion {
            authenticator_data: &authenticator_data,
            client_data_json: &client_data_json,
            signature: &signature,
        },
    )?;
    Ok(json!({
        "decision": receipt.decision,
        "nonce": receipt.challenge.nonce,
        "recorded_at": receipt.recorded_at,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Lookup {
    nonce: String,
}

/// Read-only exact receipt lookup: `{nonce}` -> `{found, receipt, tx}`.
///
/// # Errors
/// Malformed input or store failure. An absent receipt is `found: false`.
pub fn tool_deck_receipt(store: &Store, value: &JsonValue) -> Result<JsonValue> {
    let l: Lookup = input(value)?;
    Ok(match store.deck_receipt(&l.nonce)? {
        Some((receipt, tx)) => json!({"found": true, "receipt": receipt, "tx": tx}),
        None => json!({"found": false, "receipt": null, "tx": null}),
    })
}
