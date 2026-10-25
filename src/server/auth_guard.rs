//! Authentication/read-only middleware shared by the real router and its fixtures.
//! Existing write, signed-request, replica and relay decisions are retained.
use super::{auth, auth_response, relay, replica};
use axum::{http::StatusCode, response::IntoResponse};

pub(crate) struct Config {
    pub auth_policy: quipu::http_auth::BearerPolicy,
    pub session_keys: quipu::http_auth::SessionKeyPolicy,
    pub attest_store: super::SharedStore,
    pub replica_config: Option<quipu::config::ReplicaConfig>,
    pub read_only: bool,
    pub is_relay: bool,
    pub scoped_enabled: bool,
}

pub(crate) fn layer<S: Clone + Send + Sync + 'static>(
    app: axum::Router<S>,
    config: Config,
) -> axum::Router<S> {
    let Config {
        auth_policy,
        session_keys,
        attest_store,
        replica_config,
        read_only,
        is_relay,
        scoped_enabled,
    } = config;
    app
        .layer(axum::middleware::from_fn(
            move |req: axum::extract::Request, next: axum::middleware::Next| {
                let auth_policy = auth_policy.clone();
                let session_keys = session_keys.clone();
                let attest_store = attest_store.clone();
                let replica_config = replica_config.clone();
                async move {
                    let path = req.uri().path().to_string();
                    let is_write = quipu::http_auth::is_write_request(&path, req.method().as_str());
                    if let Some(replica) = &replica_config && is_write {
                        return replica::write_refusal(replica, &path);
                    }
                    // A relay serves its children's streams and challenges,
                    // which are write-class, and refuses every other write.
                    if is_relay && is_write && !path.starts_with("/replicate/") {
                        return relay::write_refusal(&path);
                    }
                    // A signed write authenticates by attestation, never by a
                    // bearer on the same request (aegis-bys8d1).
                    if (!is_write || !read_only) && req.headers().contains_key(auth::write_attest::HEADER) {
                        let response = auth::write_attest::handle(attest_store, session_keys.required, req, next).await;
                        return if scoped_enabled { super::scoped_reads::sanitize_auth(response) } else { response };
                    }
                    if let Some(response) = auth::session_key_refusal(
                        &session_keys, &req, is_write && read_only,
                    ) {
                        return response;
                    }
                    // Scoped reads authenticate named evidence even though the
                    // legacy read classifier deliberately does not need it.
                    // Signed evidence above has precedence and never falls back.
                    if scoped_enabled && !is_write {
                        let header = req.headers().get(axum::http::header::AUTHORIZATION)
                            .and_then(|h| h.to_str().ok());
                        if let Some(principal) = auth_policy.named_principal(header) {
                            let mut response = auth::run_named(req, next, principal).await;
                            response.extensions_mut().insert(quipu::request_usage::AuthOutcome::AuthenticatedNamed);
                            return response;
                        }
                    }
                    let (auth_header, authorization) = auth::authorize_request(
                        &req, &auth_policy, read_only,
                    );
                    match authorization.decision {
                        quipu::http_auth::AccessDecision::Allow => {
                            let mut req = req;
                            if matches!(
                                authorization.generation,
                                Some(
                                    quipu::http_auth::AuthGeneration::Current
                                        | quipu::http_auth::AuthGeneration::Previous
                                )
                            ) {
                                req.extensions_mut().insert(
                                    quipu::http_auth::AuthenticatedPrincipal::LEGACY_SHARED_BEARER,
                                );
                            }
                            let mut response = auth::run_authorized(
                                req, next, &auth_policy, authorization, auth_header.as_deref(),
                            ).await;
                            let outcome = match authorization.generation {
                                Some(quipu::http_auth::AuthGeneration::NotRequired) => {
                                    quipu::request_usage::AuthOutcome::NotRequired
                                }
                                Some(quipu::http_auth::AuthGeneration::OpenWrite) => {
                                    quipu::request_usage::AuthOutcome::OpenWrite
                                }
                                Some(quipu::http_auth::AuthGeneration::Current) => {
                                    quipu::request_usage::AuthOutcome::AuthenticatedCurrent
                                }
                                Some(quipu::http_auth::AuthGeneration::Previous) => {
                                    quipu::request_usage::AuthOutcome::AuthenticatedPrevious
                                }
                                Some(quipu::http_auth::AuthGeneration::Named) => {
                                    quipu::request_usage::AuthOutcome::AuthenticatedNamed
                                }
                                None => quipu::request_usage::AuthOutcome::Pending,
                            };
                            response.extensions_mut().insert(outcome);
                            response
                        }
                        // BOTH refusals carry a JSON body, and that is the whole
                        // point of aegis-zodg0. `StatusCode::into_response()`
                        // yields a bare status with a ZERO-LENGTH body, so
                        // `curl -s` prints NOTHING and exits 0 — a refusal that
                        // is indistinguishable from "the algorithm returned no
                        // results" or "the graph is empty". Measured on /project:
                        // it took two round trips to discover it was auth at all.
                        //
                        // Same failure SHAPE this repo already records for the
                        // bobbin /search 405 (CLAUDE.md): a documented recipe that
                        // silently returns nothing, so the reader concludes the
                        // service is empty rather than that they called it wrong.
                        // A refusal MUST say it refused; an empty body cannot.
                        //
                        // Fixed HERE, in the middleware, rather than per-route:
                        // every bearer-gated endpoint shared the defect. /shapes
                        // returned 0 bytes on 401 too, so there was no correct
                        // per-route body to copy — the bug was never /project's.
                        quipu::http_auth::AccessDecision::Unauthorized => {
                            // The /project note is PATH-SCOPED. Emitting it on
                            // every gated route put an explanation of `louvain`
                            // on a /shapes 401 — a message that is accurate about
                            // some other endpoint is its own small version of the
                            // defect this whole change is about, so it is
                            // conditional rather than convenient.
                            let why = if path == "/project" {
                                " /project looks read-only and is not: `louvain` with persist:true \
                                 WRITES quipu:memberOfCommunity, so the whole route is gated even \
                                 though its other algorithms only read."
                            } else {
                                ""
                            };
                            let mut response = auth_response::unauthorized(&path, why);
                            response.extensions_mut().insert(
                                quipu::request_usage::AuthOutcome::Unauthorized,
                            );
                            response
                        }
                        quipu::http_auth::AccessDecision::ReadOnly => {
                            let mut response = (
                                StatusCode::FORBIDDEN,
                                axum::Json(serde_json::json!({
                                    "error": format!(
                                        "read-only mode: {path} is a WRITE endpoint and this server was \
                                         started read-only, so no credential will authorize it. Restart \
                                         without read-only to permit writes."
                                    ),
                                    "endpoint": path,
                                    "reason": "server_is_read_only",
                                })),
                            )
                                .into_response();
                            response
                                .extensions_mut()
                                .insert(quipu::request_usage::AuthOutcome::ReadOnly);
                            response
                        }
                    }
                }
            },
        ))
}
