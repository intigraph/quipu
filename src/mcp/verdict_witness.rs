//! Which registry instant an MCP verification asks about (signing-plane S1,
//! aegis-kzt0ql.9.1). Split from `governance.rs` for the file-size ratchet.

use serde_json::Value as JsonValue;

use crate::error::{Error, Result};
use crate::governance::verifier_registry::{Scope, Witness};
use crate::store::Store;

/// The registry instant a verification asks about (signing-plane S1).
///
/// * `verdict` (an IRI): the store-witnessed instant at which that verdict's
///   `aegis:signature` was recorded. This is the form a trust decision
///   should use. The signer cannot choose it.
/// * `signed_at` / `tx`: an explicit instant, for "would this have verified
///   then?" questions. The caller chose it, and the output says so.
/// * neither: now.
pub(super) fn witness_from(
    store: &Store,
    input: &JsonValue,
    signature: &str,
) -> Result<(Witness, &'static str)> {
    if let Some(verdict) = input.get("verdict").and_then(JsonValue::as_str) {
        super::governance::guard_iri(verdict)?;
        let witness = Witness::of_fact(
            store,
            verdict,
            "http://aegis.gastown.local/ontology/signature",
            signature,
        )?
        .ok_or_else(|| {
            Error::InvalidValue(format!(
                "verdict '{verdict}' has no recorded aegis:signature with that value; \
                 there is no store-witnessed instant to verify it at"
            ))
        })?;
        return Ok((witness, "recorded"));
    }
    Ok(explicit_witness(input))
}

/// An instant the caller named (`signed_at` / `tx`), or now.
pub(super) fn explicit_witness(input: &JsonValue) -> (Witness, &'static str) {
    let at = input.get("signed_at").and_then(JsonValue::as_str);
    let tx = input.get("tx").and_then(JsonValue::as_i64);
    if at.is_none() && tx.is_none() {
        return (Witness::now(), "now");
    }
    let at = at.map_or_else(crate::time::now_iso, str::to_string);
    (Witness { tx, at }, "caller-supplied")
}

pub(super) fn witness_json(witness: &Witness, basis: &str) -> JsonValue {
    serde_json::json!({ "tx": witness.tx, "at": witness.at, "basis": basis })
}

/// Is `verifier` registered to attest `predicate_id` at `witness` by a
/// registration that chains to the human trust root (aegis-c97o)? True iff a
/// GOVERNED `aegis:VerifierRegistration` (asserted ROOT trust tier, written
/// only by a root-signed amendment) in effect then names both. An unmarked
/// registration is graph-writable by any writer, so it never authorizes.
pub(super) fn is_registered_verifier(
    store: &Store,
    verifier: &str,
    predicate_id: &str,
    witness: &Witness,
) -> Result<bool> {
    crate::governance::verifier_registry::is_authorized(
        store,
        verifier,
        predicate_id,
        witness,
        Scope::Governed,
    )
}

/// The `untrusted_reason` for a key or grant that exists only under a
/// registration with no trust-root chain (aegis-c97o).
pub(super) const UNCHAINED: &str = "registration-not-chained-to-trust-root";

/// Log, never trust, a registration that would have verified before
/// aegis-c97o. This is the migration signal: re-sign it by the trust root
/// (docs/runbooks/trust-root-ceremony.md, "Endorsing agent registrations").
pub(super) fn warn_unchained(verifier: &str, registration: &str) {
    eprintln!(
        "quipu: warning: verifier '{verifier}' matched registration {registration}, which has no \
         trust-root chain (no root-signed aegis:trustTier); NOT trusted (aegis-c97o). Endorse it \
         with a trust-root amendment to restore trust."
    );
}

/// Why a governed registration is trusted: the registration, its tier at
/// `witness`, and how it chains to the trust root.
pub(super) fn trust_json(
    store: &Store,
    registration: &str,
    witness: &Witness,
) -> Result<JsonValue> {
    use crate::governance::trust_root::{Chain, chain_of};
    let tier = crate::governance::verifier_registry::governed_tier(store, registration, witness)?;
    let chain = match chain_of(store, registration)? {
        Chain::Bootstrap {
            record,
            fingerprint,
        } => {
            serde_json::json!({ "kind": "bootstrap", "record": record, "fingerprint": fingerprint })
        }
        Chain::Amendment { amendment, signer } => serde_json::json!({
            "kind": "amendment", "amendment": amendment, "root_signer": signer
        }),
        Chain::GateOnly => serde_json::json!({ "kind": "gate" }),
    };
    Ok(serde_json::json!({
        "registration": registration,
        "tier": tier,
        "root": chain,
    }))
}

/// Is there a NON-governed registration of `verifier` for `predicate_id` at
/// `witness`? Reported so a caller can see a pre-aegis-c97o grant that no
/// longer counts.
fn has_unchained_grant(
    store: &Store,
    verifier: &str,
    predicate_id: &str,
    witness: &Witness,
) -> Result<bool> {
    crate::governance::verifier_registry::is_authorized(
        store,
        verifier,
        predicate_id,
        witness,
        Scope::Root,
    )
}

/// MCP tool: `quipu_verifier_authorized` -- check the verifier registry: may
/// `verifier` attest `predicate`? Input: `{ "verifier": "...", "predicate":
/// "...", "signed_at"?: "...", "tx"?: N }` (default: now). Output:
/// `{ "authorized": bool, "reason": null | "...", "as_of": {...} }`.
/// `authorized` needs a registration that chains to the trust root
/// (aegis-c97o); `reason` says why not.
pub fn tool_verifier_authorized(store: &Store, input: &JsonValue) -> Result<JsonValue> {
    let verifier = input
        .get("verifier")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| Error::InvalidValue("missing 'verifier' parameter".into()))?;
    let predicate = input
        .get("predicate")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| Error::InvalidValue("missing 'predicate' parameter".into()))?;
    let (witness, basis) = explicit_witness(input);
    let authorized = is_registered_verifier(store, verifier, predicate, &witness)?;
    let reason = if authorized {
        JsonValue::Null
    } else if has_unchained_grant(store, verifier, predicate, &witness)? {
        JsonValue::from(UNCHAINED)
    } else {
        JsonValue::from("no-registration-grants-this-predicate")
    };
    Ok(serde_json::json!({
        "authorized": authorized,
        "reason": reason,
        "as_of": witness_json(&witness, basis)
    }))
}
