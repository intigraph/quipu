//! A restricted credential is a separate lane, never an additive operator key.
use axum::{http::StatusCode, response::IntoResponse};
use quipu::http_auth::BearerPolicy;

pub(super) fn scoped_writer_policy(
    config: &quipu::config::ServerConfig,
    policy: BearerPolicy,
) -> Result<BearerPolicy, &'static str> {
    let Some(path) = config.scoped_writer_credentials_file.as_deref() else {
        return Ok(policy);
    };
    // These profiles have other identity/mutation admission contracts. Refuse
    // unsupported combinations instead of silently selecting the weaker lane.
    if !policy.requires_auth()
        || config.scoped_reads.is_some()
        || config.replica.is_some()
        || config.relay.is_some()
        || config.session_keys.required
    {
        return Err("scoped writer profile is incompatible or lacks operator authentication");
    }
    policy.with_scoped_writers(super::load_registry(path)?)
}

pub(super) async fn run_scoped(
    req: axum::extract::Request,
    next: axum::middleware::Next,
    principal: quipu::crew_credentials::CrewPrincipal,
) -> axum::response::Response {
    let path = req.uri().path();
    if quipu::http_auth::is_write_request(path, req.method().as_str())
        && (req.method() != axum::http::Method::POST
            || path == "/set"
            || quipu::writer_scope::WriteOperation::for_route(path).is_none())
    {
        return (StatusCode::FORBIDDEN, "scoped writer route denied").into_response();
    }
    super::run_named_class(req, next, principal, "scoped_named_bearer").await
}
