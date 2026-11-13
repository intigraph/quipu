//! Opt-in tenant dispatch: supported computations receive a capability, never
//! the original store. There is no unscoped handler fallback.

use axum::{
    extract::Request,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use quipu::read_authorization::{ReadIdentity, ReadOperation, profile::ScopedReadConfig};
use serde::Deserialize;
use std::collections::BTreeSet;
type PublishedBuffer = (
    String,
    Vec<u8>,
    quipu::read_authorization::publication::PublicationPermit,
    (u64, u64),
);

fn unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        axum::Json(serde_json::json!({
            "error":"scoped read unavailable", "outcome":"UNKNOWN"
        })),
    )
        .into_response()
}
#[cfg(test)]
#[path = "scoped_reads_tests.rs"]
mod tests;

fn denied() -> Response {
    (
        StatusCode::FORBIDDEN,
        axum::Json(serde_json::json!({"error":"scoped read denied"})),
    )
        .into_response()
}

/// Enrolment/replay details are owner diagnostics, not tenant discovery data.
pub(super) fn sanitize_auth(response: Response) -> Response {
    if response.status() == StatusCode::UNAUTHORIZED {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({"error":"scoped read denied"})),
        )
            .into_response();
    }
    if response.status().is_server_error() {
        return unavailable();
    }
    response
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    query: Option<String>,
    graphs: Option<BTreeSet<String>>,
}

/// Fail startup before opening a source or starting background work.
pub(super) fn validate(config: &quipu::QuipuConfig, args: &[String]) {
    if let Some(profile) = &config.server.scoped_reads
        && (profile
            .validate(&config.server, &config.store_path)
            .is_err()
            || !config.attachments.is_empty()
            || args.iter().any(|arg| arg == "--mcp-stdio"))
    {
        eprintln!("invalid scoped read profile; refusing startup");
        std::process::exit(2);
    }
}

/// The first scoped profile publishes only HTTP capabilities. Native MCP is
/// explicitly unsupported, including metadata/tool discovery; do not merge an
/// unqualified transport beside the guarded application.
pub(super) async fn serve_http(app: axum::Router, bind: &str, ready: impl FnOnce()) {
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .unwrap_or_else(|_| {
            eprintln!("cannot bind scoped read listener");
            std::process::exit(1)
        });
    ready();
    axum::serve(listener, app)
        .await
        .expect("scoped read listener failed");
}

/// Shared by the real router and private dispatch fixtures. An unsupported
/// request never reaches the original handler or fallback in scoped mode.
pub(super) fn guard<S: Clone + Send + Sync + 'static>(
    app: axum::Router<S>,
    profile: Option<(ScopedReadConfig, String)>,
    source: super::SharedStore,
) -> axum::Router<S> {
    app.layer(axum::middleware::from_fn(
        move |req, next: axum::middleware::Next| {
            let profile = profile.clone();
            let source = source.clone();
            async move {
                match profile {
                    Some((profile, registry)) => handle(profile, registry, source, req).await,
                    None => next.run(req).await,
                }
            }
        },
    ))
}

/// Inner to authentication, outer to every route/process/stream handler.
pub(super) async fn handle(
    profile: ScopedReadConfig,
    registry: String,
    source: super::SharedStore,
    req: Request,
) -> Response {
    let path = req.uri().path().to_owned();
    let method = req.method().as_str();
    let evidence = super::auth::request_identity();
    let presented = req
        .headers()
        .get_all(axum::http::header::AUTHORIZATION)
        .iter()
        .count();
    if presented > 1 || (presented == 1 && evidence.is_none()) {
        return denied();
    }
    let class = quipu::read_authorization::routes::classify(&path, method);
    if class == quipu::read_authorization::routes::RouteClass::PublicStatic {
        return axum::Json(serde_json::json!({"service":"quipu","profile":"scoped-current/v1"}))
            .into_response();
    }
    if profile.memory_rpc.is_some() {
        return memory_handle(profile, registry, source, req).await;
    }
    let quipu::read_authorization::routes::RouteClass::Scoped(operation) = class else {
        return denied();
    };
    if req.uri().query().is_some() {
        return denied();
    }
    let Some(evidence) = evidence else {
        return denied();
    };
    let header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let Ok(body) = axum::body::to_bytes(req.into_body(), 64 * 1024).await else {
        return denied();
    };
    let input: Input = if operation == ReadOperation::Metadata && body.is_empty() {
        Input {
            query: None,
            graphs: None,
        }
    } else {
        match serde_json::from_slice(&body) {
            Ok(input) => input,
            Err(_) => return denied(),
        }
    };
    if (operation == ReadOperation::Query) != input.query.is_some() {
        return denied();
    }
    // Owned evidence and capability cross the blocking boundary. No global
    // current principal, graph union or source Store is handed to computations.
    let result = super::base::spawn_tracked(move || -> quipu::Result<PublishedBuffer> {
        let refuse = || quipu::Error::PolicyDenied("scoped read denied".into());
        let now = quipu::time::epoch_secs();
        let reader = source.read();
        let identity = current_identity(&reader, &evidence, &registry, header.as_deref(), now)?;
        let policy = profile
            .open_policy()
            .map_err(|_| quipu::Error::CannotVerify("read policy unavailable".into()))?;
        let snapshot = policy
            .snapshot()
            .map_err(|_| quipu::Error::CannotVerify("read policy unavailable".into()))?;
        let view = snapshot
            .authorize(&identity, operation, input.graphs.as_ref(), now)?
            .open(&reader, now)?;
        drop(reader);
        let output = match operation {
            ReadOperation::Query => {
                let (content_type, bytes) =
                    view.query_response(input.query.as_deref().ok_or_else(refuse)?, true, now)?;
                (content_type.into(), bytes)
            }
            ReadOperation::Export => (
                "application/json".into(),
                serde_json::to_vec(&serde_json::json!({
                    "schema":"quipu-scoped-current/v1", "graphs":view.export(now)?, "complete":true
                }))
                .map_err(|_| refuse())?,
            ),
            ReadOperation::Metadata => (
                "application/json".into(),
                serde_json::to_vec(&view.metadata(now)?).map_err(|_| refuse())?,
            ),
            ReadOperation::Replicate => return Err(refuse()),
        };
        // Acquire before the final recheck and keep the permit in the response
        // body: owner mutation cannot ACK while old buffered output is held.
        let permit = quipu::read_authorization::publication::PublicationPermit::acquire(
            &profile.policy_path,
        )?;
        let current = profile
            .open_policy()
            .and_then(|policy| policy.snapshot())
            .map_err(|_| quipu::Error::CannotVerify("read policy unavailable".into()))?;
        let now = quipu::time::epoch_secs();
        let reader = source.read();
        let identity = current_identity(&reader, &evidence, &registry, header.as_deref(), now)?;
        view.recheck_source(&reader)?;
        view.recheck(&current, &identity, now)?;
        Ok((output.0, output.1, permit, view.publication_window()))
    })
    .await;
    match result {
        Ok(Ok((content_type, bytes, permit, window))) => {
            protected_body(content_type, bytes, permit, window)
        }
        Ok(Err(quipu::Error::PolicyDenied(_) | quipu::Error::InvalidValue(_))) => denied(),
        _ => unavailable(),
    }
}

fn current_identity(
    source: &quipu::Store,
    evidence: &quipu::transaction_auth::Identity,
    registry: &str,
    header: Option<&str>,
    now: u64,
) -> quipu::Result<ReadIdentity> {
    let refuse = || quipu::Error::PolicyDenied("scoped read denied".into());
    if evidence.auth_class == "named_bearer" {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(registry)
            .map_err(|_| refuse())?
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| refuse())?;
        let registry = quipu::crew_credentials::CredentialRegistry::parse(&bytes, "quipu")
            .map_err(|_| refuse())?;
        let principal = registry.authenticate(header).ok_or_else(refuse)?;
        if principal.iri != evidence.principal
            || Some(&principal.credential_id) != evidence.credential_id.as_ref()
        {
            return Err(refuse());
        }
        return ReadIdentity::authenticate_named(&registry, header);
    }
    if evidence.auth_class != "attested_session" {
        return Err(refuse());
    }
    let session = evidence.attestation.0.as_ref().ok_or_else(refuse)?;
    let binding = source
        .attestation_binding(&session.session)?
        .ok_or_else(refuse)?;
    if binding.revoked
        || binding.agent != evidence.principal
        || Some(&binding.key_id) != evidence.credential_id.as_ref()
        || now < binding.issued_at_epoch
        || now >= binding.expires_at_epoch
    {
        return Err(refuse());
    }
    ReadIdentity::from_verified_request(evidence, binding.expires_at_epoch)
}

/// The permit is owned by the body stream, including a response abandoned before
/// its first poll. EOF or cancellation drops it; it is never response metadata.
fn protected_body(
    content_type: String,
    bytes: Vec<u8>,
    permit: quipu::read_authorization::publication::PublicationPermit,
    window: (u64, u64),
) -> Response {
    let stream = futures::stream::unfold(Some((bytes, permit)), move |state| async move {
        state.map(|(bytes, permit)| {
            (
                if (window.0..window.1).contains(&quipu::time::epoch_secs()) {
                    Ok(axum::body::Bytes::from(bytes))
                } else {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "read publication expired",
                    ))
                },
                Some((Vec::new(), permit)),
            )
        })
    });
    // One buffered frame, followed by EOF. The retained state owns the permit
    // through the first frame's handoff, rather than dropping it before yield.
    use futures::StreamExt;
    (
        [
            (axum::http::header::CONTENT_TYPE, content_type),
            (
                axum::http::header::CACHE_CONTROL,
                "private, no-store".into(),
            ),
        ],
        axum::body::Body::from_stream(stream.take(1)),
    )
        .into_response()
}

async fn memory_handle(
    profile: ScopedReadConfig,
    registry: String,
    source: super::SharedStore,
    req: Request,
) -> Response {
    let path = req.uri().path();
    if req.method() != axum::http::Method::POST
        || !matches!(path, "/memory/read" | "/memory/export")
        || req.uri().query().is_some()
    {
        return denied();
    }
    let export = path == "/memory/export";
    let Some(evidence) = super::auth::request_identity() else {
        return denied();
    };
    let header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let Ok(bytes) = axum::body::to_bytes(req.into_body(), 64 * 1024).await else {
        return denied();
    };
    let Ok(owner) = serde_json::from_slice::<quipu::memory_files::Owner>(&bytes) else {
        return denied();
    };
    // Root/admin is an exact principal, still requiring the ordinary explicit
    // Export grant. Shared/write-only credentials never become read authority.
    // Attested sessions revoke through the source store, outside this policy
    // inode barrier. They remain unsupported in the private HWH topology.
    if evidence.auth_class != "named_bearer"
        || (evidence.principal != owner.crew && evidence.principal != profile.owner)
    {
        return denied();
    }
    let result = super::base::spawn_tracked(move || -> quipu::Result<_> {
        let config = profile
            .memory_rpc
            .as_ref()
            .ok_or_else(|| quipu::Error::PolicyDenied("memory topology required".into()))?;
        config.check(&owner)?;
        let now = quipu::time::epoch_secs();
        let reader = source.read();
        let identity = current_identity(&reader, &evidence, &registry, header.as_deref(), now)?;
        let policy = profile.open_policy()?;
        let requested = [config.graph.clone()].into_iter().collect();
        let view = policy
            .snapshot()?
            .authorize(&identity, ReadOperation::Export, Some(&requested), now)?
            .open(&reader, now)?;
        drop(reader);
        let bytes = view.memory_response(&owner, &config.graph, export, now)?;
        let permit = quipu::read_authorization::publication::PublicationPermit::acquire(
            &profile.policy_path,
        )?;
        let current = profile.open_policy()?.snapshot()?;
        let now = quipu::time::epoch_secs();
        let reader = source.read();
        let identity = current_identity(&reader, &evidence, &registry, header.as_deref(), now)?;
        view.recheck_source(&reader)?;
        view.recheck(&current, &identity, now)?;
        Ok((bytes, permit, view.publication_window()))
    })
    .await;
    match result {
        Ok(Ok((bytes, permit, window))) => {
            protected_body("application/json".into(), bytes, permit, window)
        }
        Ok(Err(quipu::Error::PolicyDenied(_) | quipu::Error::InvalidValue(_))) => denied(),
        _ => unavailable(),
    }
}
