//! Read identity admitted once by the primary, carried only on the private hop.
use super::{HeaderValue, IntoResponse, KEY, Next, PEER, PID, Response, StatusCode};
use axum::extract::Request;
use base64::Engine as _;
use quipu::transaction_auth::{AttestState, Identity};

pub(super) const HEADER: &str = "x-quipu-read-worker-identity";
const LIMIT: usize = 16 * 1024;

#[derive(Clone)]
struct TrustedReadIdentity(Identity);

/// Only the primary's verified, already-spent request may create handoff evidence.
pub(super) fn for_forward(req: &Request) -> Result<Option<String>, ()> {
    if !req
        .headers()
        .contains_key(super::super::auth::write_attest::HEADER)
    {
        return Ok(None);
    }
    let identity = super::super::auth::request_identity().ok_or(())?;
    if identity.auth_class != "attested_session"
        || !identity
            .attestation
            .0
            .as_ref()
            .is_some_and(|pending| pending.state() == AttestState::Spent)
    {
        return Err(());
    }
    let bytes = serde_json::to_vec(&identity).map_err(|_| ())?;
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    if encoded.len() > LIMIT {
        return Err(());
    }
    Ok(Some(encoded))
}

/// Call ONLY after `worker_guard` has authenticated the private worker key.
/// The opaque extension cannot be constructed from an ordinary public header.
pub(super) fn restore(req: &mut Request) -> Result<(), ()> {
    let signed = req
        .headers()
        .contains_key(super::super::auth::write_attest::HEADER);
    let values: Vec<_> = req.headers().get_all(HEADER).iter().collect();
    if !signed {
        if !values.is_empty() {
            return Err(());
        }
        return Ok(());
    }
    if !super::routed(req.uri().path(), req.method().as_str())
        || quipu::http_auth::is_write_request(req.uri().path(), req.method().as_str())
        || values.len() != 1
    {
        return Err(());
    }
    let raw = values[0].to_str().map_err(|_| ())?;
    if raw.is_empty() || raw.len() > LIMIT {
        return Err(());
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw)
        .map_err(|_| ())?;
    let identity: Identity = serde_json::from_slice(&bytes).map_err(|_| ())?;
    if identity.auth_class != "attested_session"
        || identity.principal.is_empty()
        || identity.credential_id.as_ref().is_none_or(String::is_empty)
        || identity.attestation.0.is_some()
    {
        return Err(());
    }
    req.headers_mut().remove(HEADER);
    req.extensions_mut().insert(TrustedReadIdentity(identity));
    Ok(())
}

pub(crate) fn identity(req: &Request) -> Option<Identity> {
    req.extensions()
        .get::<TrustedReadIdentity>()
        .map(|value| value.0.clone())
}

pub(super) async fn worker_guard(mut req: Request, next: Next) -> Response {
    let expected = std::env::var(KEY).unwrap_or_default();
    if expected.is_empty()
        || req
            .headers()
            .get(super::HEADER)
            .and_then(|v| v.to_str().ok())
            != Some(expected.as_str())
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    if restore(&mut req).is_err() {
        return StatusCode::FORBIDDEN.into_response();
    }
    // Only the authenticated private channel may restore an original peer.
    // MCP calls without a socket peer stay UNKNOWN rather than becoming localhost.
    let peer = req
        .headers()
        .get(PEER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<std::net::SocketAddr>().ok());
    req.extensions_mut()
        .remove::<axum::extract::ConnectInfo<std::net::SocketAddr>>();
    if let Some(peer) = peer {
        req.extensions_mut()
            .insert(axum::extract::ConnectInfo(peer));
    }
    req.headers_mut().remove(PEER);
    req.headers_mut().remove(super::HEADER);
    let mut response = next.run(req).await;
    response.headers_mut().insert(
        PID,
        HeaderValue::from_str(&std::process::id().to_string()).expect("numeric PID"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(path: &str, value: Option<&str>, signed: bool) -> Request {
        let mut req = axum::http::Request::post(path);
        if signed {
            req = req.header(
                super::super::super::auth::write_attest::HEADER,
                "retained-original",
            );
        }
        if let Some(value) = value {
            req = req.header(HEADER, value);
        }
        req.body(axum::body::Body::empty()).unwrap()
    }
    fn encoded() -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&Identity {
                principal: "urn:fixture:principal".into(),
                credential_id: Some("fixture-key".into()),
                auth_class: "attested_session".into(),
                attestation: Default::default(),
            })
            .unwrap(),
        )
    }
    #[test]
    fn private_restore_preserves_principal_without_a_mutable_nonce() {
        let mut req = request("/query", Some(&encoded()), true);
        restore(&mut req).unwrap();
        let identity = identity(&req).unwrap();
        assert_eq!(identity.principal, "urn:fixture:principal");
        assert_eq!(identity.credential_id.as_deref(), Some("fixture-key"));
        assert!(identity.attestation.0.is_none());
        assert!(
            req.headers()
                .contains_key(super::super::super::auth::write_attest::HEADER)
        );
        assert!(!req.headers().contains_key(HEADER));
    }
    #[test]
    fn missing_malformed_unsigned_and_write_handoffs_refuse() {
        let valid = encoded();
        for (path, value, signed) in [
            ("/query", None, true),
            ("/query", Some("malformed"), true),
            ("/query", Some(valid.as_str()), false),
            ("/knot", Some(valid.as_str()), true),
        ] {
            assert!(restore(&mut request(path, value, signed)).is_err());
        }
    }
    #[tokio::test]
    async fn declared_header_and_unspent_identity_cannot_create_a_handoff() {
        let req = request("/query", Some(&encoded()), true);
        assert!(for_forward(&req).is_err());
        let identity = Identity {
            principal: "urn:fixture:principal".into(),
            credential_id: Some("fixture-key".into()),
            auth_class: "attested_session".into(),
            attestation: quipu::transaction_auth::AttestationSlot(Some(
                quipu::transaction_auth::PendingAttestation::new(
                    "fixture-session".into(),
                    "1".repeat(32),
                    "fixture-key".into(),
                    "fixture-introducer".into(),
                ),
            )),
        };
        super::super::super::auth::run_identified(Some(identity), async {
            assert!(for_forward(&req).is_err());
        })
        .await;
    }
}
