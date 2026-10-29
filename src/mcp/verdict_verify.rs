//! `quipu_verdict_verify` (`POST /verdict/verify`): verify a signed verdict
//! against the trust root (signing-plane S1, S3b / aegis-c97o). Split from
//! `governance.rs` for the file-size ratchet; re-exported there.

use serde_json::Value as JsonValue;

use crate::error::{Error, Result};
use crate::governance::verifier_registry::{Scope, keyed_registrations};
use crate::store::Store;

use super::verdict_witness::{
    UNCHAINED, is_registered_verifier, trust_json, warn_unchained, witness_from, witness_json,
};

/// MCP tool: `quipu_verdict_verify` -- verify a signed Verdict against the
/// root of trust AS OF the signature (signing-plane S1): the signature must be
/// valid under a key that was registered to the verifier at that instant, AND
/// that same registration must authorize the predicate. `trusted` is exactly
/// that conjunction, the property a consumer should gate on.
///
/// Input: the verdict fields `{ predicate_id, target_ref, outcome,
/// evidence_hash, tier?, verifier, signature }`, plus the instant: `verdict`
/// (the stored verdict's IRI; quipu reads when it was recorded) or
/// `signed_at`/`tx` (explicit), defaulting to now. Output: `{ signature_valid,
/// verifier_registered, verifier_authorized, trusted, trust, untrusted_reason,
/// unchained_registration, as_of }`, where `as_of.basis` is `recorded`,
/// `caller-supplied` or `now`. Only a registration that chains to the human
/// trust root (aegis-c97o) can make a verdict trusted; `trust` names it.
pub fn tool_verdict_verify(store: &Store, input: &JsonValue) -> Result<JsonValue> {
    let field = |k: &str| -> Result<String> {
        input
            .get(k)
            .and_then(JsonValue::as_str)
            .map(std::string::ToString::to_string)
            .ok_or_else(|| Error::InvalidValue(format!("missing '{k}' parameter")))
    };
    let predicate_id = field("predicate_id")?;
    let target_ref = field("target_ref")?;
    let outcome = field("outcome")?;
    let evidence_hash = field("evidence_hash")?;
    let verifier = field("verifier")?;
    let signature = field("signature")?;
    let tier = input
        .get("tier")
        .and_then(JsonValue::as_str)
        .unwrap_or("committed");
    let (witness, basis) = witness_from(store, input, &signature)?;

    let message = crate::signing::verdict_message(
        &predicate_id,
        &target_ref,
        &outcome,
        &evidence_hash,
        tier,
        &verifier,
    );

    let claim = super::governance_hardware::Claim {
        verifier: &verifier,
        predicate_id: &predicate_id,
        message: &message,
        signature: &signature,
    };
    if let Some(out) = super::governance_hardware::dispatch(store, input, &claim, &witness, basis)?
    {
        return Ok(out);
    }

    // aegis-c97o: a key is TRUSTED only through a registration that chains to
    // the human trust root (`Scope::Governed`: an asserted ROOT trust tier,
    // which only a root-signed amendment can write). Any other ROOT
    // registration is still read, so the answer can say it exists and why it
    // does not count, but it never makes a verdict trusted.
    let verifying = |rows: &[crate::governance::verifier_registry::KeyedRegistration]| {
        rows.iter()
            .find(|r| crate::signing::verify_hex(&r.key, &message, &signature))
            .cloned()
    };
    let any_rows = keyed_registrations(store, &verifier, None, &witness, Scope::Root)?;
    let scoped_rows =
        keyed_registrations(store, &verifier, Some(&predicate_id), &witness, Scope::Root)?;
    let governed_rows = keyed_registrations(
        store,
        &verifier,
        Some(&predicate_id),
        &witness,
        Scope::Governed,
    )?;
    let trusted_by = verifying(&governed_rows);
    let seal_ok = trusted_by.is_some();
    let signature_valid = verifying(&any_rows).is_some();
    let unchained = if seal_ok {
        None
    } else {
        verifying(&scoped_rows)
    };
    // An ALLOW-list: a basis added later fails closed rather than trusting by default.
    let trustworthy_basis = matches!(basis, "recorded" | "now");
    let reason = if seal_ok {
        if trustworthy_basis {
            JsonValue::Null
        } else {
            JsonValue::from("caller-supplied-instant")
        }
    } else if any_rows.is_empty() {
        JsonValue::from("verifier-not-registered")
    } else if !signature_valid {
        JsonValue::from("signature-invalid")
    } else if let Some(row) = &unchained {
        warn_unchained(&verifier, &row.registration);
        JsonValue::from(UNCHAINED)
    } else {
        JsonValue::from("not-authorized-for-predicate")
    };
    let trust = match &trusted_by {
        Some(row) => trust_json(store, &row.registration, &witness)?,
        None => JsonValue::Null,
    };
    Ok(serde_json::json!({
        "signature_valid": signature_valid,
        "verifier_registered": !any_rows.is_empty(),
        "verifier_authorized": is_registered_verifier(store, &verifier, &predicate_id, &witness)?,
        // The key that verifies must be one the AUTHORIZING, GOVERNED
        // registration holds, AND the instant must be one quipu chose. A
        // caller-supplied instant is a what-if: naming a tx from before a
        // revocation would otherwise make a revoked key read as trusted.
        "trusted": trustworthy_basis && seal_ok,
        "would_verify_as_of_supplied_instant": if trustworthy_basis { JsonValue::Null } else { JsonValue::Bool(seal_ok) },
        // WHY: which registration, which tier, and how it chains to the trust
        // root; or why not.
        "trust": trust,
        "untrusted_reason": reason,
        "unchained_registration": unchained.map(|r| r.registration),
        "as_of": witness_json(&witness, basis)
    }))
}
