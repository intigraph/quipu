//! Native attested-write transport (aegis-25k): challenge, commit, receipt.
//!
//! The authority stays in `store::attested_write`: these functions only
//! decode a strict JSON request, call the native method, and encode its
//! result. The client signs the message THIS challenge response returns,
//! never one it rebuilds: the commit carries only the nonce and the
//! assertion, and the server re-derives the signed bytes from the stored
//! challenge. A refused or indeterminate commit is never retried here; the
//! caller looks up the receipt by nonce instead.

use serde::Deserialize;
use serde_json::{Value as JsonValue, json};

use crate::error::{Error, Result};
use crate::store::Store;
use crate::store::attested_write::{Proposal, receipt_iri};
use crate::verdict_schemes::{b64url_decode, webauthn};

fn input<T: for<'a> Deserialize<'a>>(value: &JsonValue) -> Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|e| Error::InvalidValue(format!("attested write: {e}")))
}

fn bytes(field: &str, value: &str) -> Result<Vec<u8>> {
    b64url_decode(value)
        .map_err(|_| Error::InvalidValue(format!("attested write: {field} is not base64url")))
}

fn b64url(data: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

/// Issue one challenge over a proposal: `{signer, graph, datums, expect?}` ->
/// `{challenge, message, webauthn_challenge}`. `message` is the exact signed
/// bytes and `webauthn_challenge` their digest for
/// `navigator.credentials.get()`, both unpadded base64url.
///
/// # Errors
/// Malformed input, or any refusal of `Store::attest_challenge`.
pub fn tool_attest_challenge(store: &mut Store, value: &JsonValue) -> Result<JsonValue> {
    let proposal: Proposal = input(value)?;
    let challenge = store.attest_challenge(&proposal)?;
    Ok(json!({
        "message": b64url(&challenge.message()?),
        "webauthn_challenge": b64url(&challenge.webauthn_challenge()?),
        "challenge": challenge,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Commit {
    nonce: String,
    client_data_json: String,
    authenticator_data: String,
    signature: String,
}

/// Verify and admit one challenge: `{nonce, client_data_json,
/// authenticator_data, signature}` (base64url) -> `{nonce, receipt_iri,
/// content_hash, recorded_at}`.
///
/// # Errors
/// Malformed input, or any refusal of `Store::attest_commit`. After an
/// indeterminate response, call [`tool_attest_receipt`]; never resubmit.
pub fn tool_attest_commit(store: &mut Store, value: &JsonValue) -> Result<JsonValue> {
    let c: Commit = input(value)?;
    let client_data_json = bytes("client_data_json", &c.client_data_json)?;
    let authenticator_data = bytes("authenticator_data", &c.authenticator_data)?;
    let signature = bytes("signature", &c.signature)?;
    let receipt = store.attest_commit(
        &c.nonce,
        &webauthn::Assertion {
            authenticator_data: &authenticator_data,
            client_data_json: &client_data_json,
            signature: &signature,
        },
    )?;
    Ok(json!({
        "nonce": receipt.challenge.nonce,
        "receipt_iri": receipt_iri(&receipt.challenge.nonce),
        "content_hash": receipt.content_hash,
        "recorded_at": receipt.recorded_at,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Lookup {
    nonce: String,
}

/// Read-only exact receipt lookup: `{nonce}` -> `{found, receipt, tx,
/// verified}`. `verified` re-checks the retained assertion against the
/// enrolled key snapshot; later revocation does not change it.
///
/// # Errors
/// Malformed input or store failure. An absent receipt is `found: false`.
pub fn tool_attest_receipt(store: &Store, value: &JsonValue) -> Result<JsonValue> {
    let l: Lookup = input(value)?;
    Ok(match store.attest_receipt(&l.nonce)? {
        Some((receipt, tx)) => json!({
            "found": true,
            "verified": store.attest_verify_receipt(&l.nonce)?,
            "receipt_iri": receipt_iri(&l.nonce),
            "receipt": receipt,
            "tx": tx,
        }),
        None => json!({"found": false, "verified": false, "receipt": null, "tx": null}),
    })
}
