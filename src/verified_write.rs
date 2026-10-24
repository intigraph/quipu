//! Native verification of a signed write, shared by the HTTP receiver and any
//! in-process caller (aegis-2idcev, design of 2026-10-14).
//!
//! [`check_signed_request`] is the one canonical check of a signed request's
//! exact bytes: every check except the nonce spend, plus a read of whether the
//! nonce is already spent. The HTTP middleware calls it, and so does
//! [`verify_signed_write`], which additionally requires a store-audience-bound
//! write-v2 envelope and an audited write route, and returns an opaque
//! [`VerifiedWrite`].
//!
//! A `VerifiedWrite` is an in-process value, not a credential. It has private
//! fields, no public constructor, no `Clone`, no deserializer and no
//! conversion from `Identity` or `PendingAttestation`. It proves only that the
//! canonical verifier accepted these exact bytes for this store at that time.
//! It does not spend the nonce or authorize an effect: a write still needs the
//! writer barrier, which re-checks the binding and spends the nonce under the
//! writer lock (not provided here).

use crate::session_attestation::{
    ATTESTATION_SKEW_SECS, AttestationEnvelope, Refusal, SignedBinding, VerifiedPrincipal,
    WRITE_V2, WriteBinding, body_sha256, check_binding_deferred,
};
use crate::store::Store;

/// Endpoints that accept a signed write, each audited for its barrier:
/// /knot, /update and /episode mutate ONLY through store transactions, so
/// `transaction_auth::record` refuses a refused request. /graph/create writes
/// the graphs registry WITHOUT a transaction; it is an `rw_handler!`, which
/// calls `refuse_if_refused` right after the writer lock is taken. Any other
/// endpoint is added only after the same audit names its barrier.
pub const ATTESTED_PATHS: [&str; 4] = ["/knot", "/update", "/episode", "/graph/create"];

/// The largest body a signed write may carry (the server's own body limit).
pub const MAX_SIGNED_BODY: usize = 64 * 1024 * 1024;

/// The exact request a signature is checked against. Every hash is computed
/// here from these bytes; a caller-supplied hash is never trusted.
#[derive(Debug, Clone, Copy)]
pub struct SignedRequest<'a> {
    /// HTTP method, exactly as sent.
    pub method: &'a str,
    /// Path, with no query string.
    pub path: &'a str,
    /// Content type, exactly as sent.
    pub content_type: &'a str,
    /// The exact body bytes.
    pub body: &'a [u8],
}

fn refusal(verdict: &'static str, message: impl Into<String>) -> Refusal {
    Refusal {
        verdict,
        message: message.into(),
    }
}

/// The canonical check of one signed request against `store`: size bound,
/// body hash from the bytes, the receiving store's own id as the v2 audience
/// (never the envelope's claim), every binding check, and whether the nonce
/// is already spent. Returns the principal and the audience it bound, if any.
///
/// # Errors
/// A [`Refusal`] carrying the client-actionable verdict.
pub fn check_signed_request(
    store: &Store,
    envelope: &AttestationEnvelope,
    request: SignedRequest<'_>,
    now_epoch: u64,
) -> Result<(VerifiedPrincipal, Option<String>), Refusal> {
    if request.body.len() > MAX_SIGNED_BODY {
        return Err(refusal(
            "invalid",
            "the request body exceeds the size limit",
        ));
    }
    if request.path.contains('?') {
        return Err(refusal(
            "invalid",
            "a signed write must not carry a query string",
        ));
    }
    let body_sha256 = body_sha256(request.body);
    let audience = if envelope.version == WRITE_V2 {
        Some(
            store
                .store_id()
                .map_err(|e| refusal("error", format!("this store's id could not be read: {e}")))?,
        )
    } else {
        None
    };
    let write = WriteBinding {
        method: request.method,
        path: request.path,
        content_type: request.content_type,
        body_sha256: &body_sha256,
        audience: audience.as_deref(),
    };
    let principal = check_binding_deferred(
        store,
        envelope,
        &SignedBinding::Write(write),
        now_epoch,
        ATTESTATION_SKEW_SECS,
    )?;
    match store.attestation_nonce_spent(&envelope.session, &envelope.nonce) {
        Ok(false) => Ok((principal, audience)),
        Ok(true) => Err(refusal("replay", "the nonce was already spent")),
        Err(e) => Err(refusal("error", e.to_string())),
    }
}

/// A signed write the canonical verifier accepted, for these exact bytes and
/// this receiving store. Read-only; see the module documentation for what it
/// does and does not prove.
#[derive(Debug)]
pub struct VerifiedWrite {
    principal: VerifiedPrincipal,
    envelope: AttestationEnvelope,
    method: String,
    path: String,
    content_type: String,
    body: Vec<u8>,
    body_sha256: String,
    audience: String,
}

impl VerifiedWrite {
    /// The authenticated agent.
    #[must_use]
    pub fn agent(&self) -> &str {
        &self.principal.agent
    }
    /// The authenticated session.
    #[must_use]
    pub fn session(&self) -> &str {
        &self.principal.session
    }
    /// The registered key that signed.
    #[must_use]
    pub fn key_id(&self) -> &str {
        &self.principal.key_id
    }
    /// The introducer that registered the session.
    #[must_use]
    pub fn introducer(&self) -> &str {
        &self.principal.introducer
    }
    /// The signed nonce, still unspent at verification.
    #[must_use]
    pub fn nonce(&self) -> &str {
        &self.envelope.nonce
    }
    /// The exact envelope that was verified.
    #[must_use]
    pub fn envelope(&self) -> &AttestationEnvelope {
        &self.envelope
    }
    /// The signed method.
    #[must_use]
    pub fn method(&self) -> &str {
        &self.method
    }
    /// The signed path, one of [`ATTESTED_PATHS`].
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
    /// The signed content type.
    #[must_use]
    pub fn content_type(&self) -> &str {
        &self.content_type
    }
    /// The exact signed body.
    #[must_use]
    pub fn body(&self) -> &[u8] {
        &self.body
    }
    /// Lowercase hex SHA-256 of the body, computed at verification.
    #[must_use]
    pub fn body_sha256(&self) -> &str {
        &self.body_sha256
    }
    /// The receiving store's id that the signature is bound to.
    #[must_use]
    pub fn audience(&self) -> &str {
        &self.audience
    }
}

/// Verify one signed write for `store`. Requires a write-v2 envelope, so the
/// signature is bound to this store's id; refuses a v1 envelope rather than
/// downgrading. The path must be an audited write route.
///
/// # Errors
/// A [`Refusal`] carrying the client-actionable verdict.
pub fn verify_signed_write(
    store: &Store,
    envelope: AttestationEnvelope,
    request: SignedRequest<'_>,
    now_epoch: u64,
) -> Result<VerifiedWrite, Refusal> {
    if envelope.version != WRITE_V2 {
        return Err(refusal(
            "invalid",
            "a verified write requires a store-audience-bound write-v2 envelope",
        ));
    }
    if !ATTESTED_PATHS.contains(&request.path) {
        return Err(refusal(
            "invalid",
            format!(
                "signed writes are accepted on {} only",
                ATTESTED_PATHS.join(", ")
            ),
        ));
    }
    let (principal, audience) = check_signed_request(store, &envelope, request, now_epoch)?;
    let audience = audience.ok_or_else(|| refusal("error", "write-v2 bound no audience"))?;
    Ok(VerifiedWrite {
        principal,
        body_sha256: body_sha256(request.body),
        method: request.method.to_owned(),
        path: request.path.to_owned(),
        content_type: request.content_type.to_owned(),
        body: request.body.to_vec(),
        envelope,
        audience,
    })
}

#[cfg(test)]
#[path = "verified_write_tests.rs"]
mod tests;
