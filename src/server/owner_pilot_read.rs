//! Buffered, authorized HTTP reads over one already admitted dataset.
use super::{PreparedDataset, refuse};
use crate::base::AppError;
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{OriginalUri, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use quipu::read_authorization::{
    ComposedQueryOutput, ComposedReadAdmission, ReadIdentity, ReadOperation,
    profile::ScopedReadConfig,
};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    io::Read,
    sync::Arc,
    time::{Duration, Instant},
};

/// Owner-selected authority, separate from the data files and request body.
#[derive(Clone)]
pub(crate) struct ReadConfig {
    pub policy: ScopedReadConfig,
    pub registry: String,
    pub timeout: Duration,
}

#[derive(Clone)]
struct Serving {
    dataset: Arc<PreparedDataset>,
    config: ReadConfig,
    admission: ComposedReadAdmission,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    query: String,
    graphs: Vec<String>,
}

fn separate_policy(dataset: &PreparedDataset, config: &ReadConfig) -> quipu::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let denied = || quipu::Error::CannotVerify("separate read authority unavailable".into());
    let policy = std::fs::metadata(&config.policy.policy_path).map_err(|_| denied())?;
    if !policy.is_file() {
        return Err(denied());
    }
    for file in &dataset.native_files {
        let data = std::fs::metadata(file).map_err(|_| denied())?;
        if policy.dev() == data.dev() && policy.ino() == data.ino() {
            return Err(denied());
        }
    }
    Ok(())
}

fn load_registry(path: &str) -> quipu::Result<quipu::crew_credentials::CredentialRegistry> {
    let deny = || quipu::Error::PolicyDenied("composed read denied".into());
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| deny())?
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|_| deny())?;
    if bytes.len() > 1_048_576 {
        return Err(deny());
    }
    quipu::crew_credentials::CredentialRegistry::parse(&bytes, "quipu").map_err(|_| deny())
}

fn identity(
    registry: &str,
    header: &str,
    evidence: &quipu::transaction_auth::Identity,
) -> quipu::Result<ReadIdentity> {
    let deny = || quipu::Error::PolicyDenied("composed read denied".into());
    if evidence.auth_class != "named_bearer" {
        return Err(deny());
    }
    let registry = load_registry(registry)?;
    let principal = registry.authenticate(Some(header)).ok_or_else(deny)?;
    if principal.iri != evidence.principal
        || Some(&principal.credential_id) != evidence.credential_id.as_ref()
    {
        return Err(deny());
    }
    ReadIdentity::authenticate_named(&registry, Some(header))
}

pub(super) fn routes(
    dataset: Arc<PreparedDataset>,
    config: ReadConfig,
    admission: ComposedReadAdmission,
) -> Result<Router, AppError> {
    if config.timeout.is_zero() || config.timeout > Duration::from_secs(30) {
        return Err(refuse("bounded composed read timeout required"));
    }
    separate_policy(&dataset, &config)?;
    config.policy.open_policy()?.snapshot()?;
    load_registry(&config.registry)?;
    Ok(dataset.routes().merge(
        Router::new()
            .route("/owner/query", post(query))
            .layer(axum::extract::DefaultBodyLimit::max(131_072))
            .with_state(Serving {
                dataset,
                config,
                admission,
            }),
    ))
}

// Bytes retains the original query lease through HTTP body consumption, without
// copying a bounded response into a second, unaccounted buffer.
struct Output(ComposedQueryOutput);
impl AsRef<[u8]> for Output {
    fn as_ref(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

fn unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({"error":"composed read unavailable", "outcome":"UNKNOWN"})),
    )
        .into_response()
}

async fn query(
    State(serving): State<Serving>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    Json(input): Json<Input>,
) -> Response {
    let deny = || {
        (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error":"composed read denied"})),
        )
            .into_response()
    };
    let Some(evidence) = crate::auth::request_identity() else {
        return deny();
    };
    if uri.query().is_some()
        || headers.get_all(header::AUTHORIZATION).iter().count() != 1
        || input.query.len() > 65_536
        || input.graphs.is_empty()
        || input.graphs.len() > 2
    {
        return deny();
    }
    let selected: BTreeSet<String> = input.graphs.iter().cloned().collect();
    if selected.len() != input.graphs.len() {
        return deny();
    }
    let Some(header) = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned)
    else {
        return deny();
    };
    // Compute the deadline before the blocking queue, not after queue wait.
    let deadline = Instant::now() + serving.config.timeout;
    // Hold one reservation across the blocking queue, not only evaluation.
    let Ok(permit) = serving.admission.try_reserve() else {
        return unavailable();
    };
    let result = crate::base::spawn_tracked(move || -> quipu::Result<ComposedQueryOutput> {
        let now = quipu::time::epoch_secs();
        let who = identity(&serving.config.registry, &header, &evidence)?;
        separate_policy(&serving.dataset, &serving.config)?;
        let policy = serving.config.policy.open_policy()?.snapshot()?;
        let authorized = policy.authorize(&who, ReadOperation::Query, Some(&selected), now)?;
        let graphs: Vec<&str> = input.graphs.iter().map(String::as_str).collect();
        serving
            .dataset
            .read(&graphs, deadline, |reads| {
                let view = authorized.open_composed_cut(reads, permit, now, deadline)?;
                let output = view.query(&input.query, false, now)?;
                separate_policy(&serving.dataset, &serving.config)?;
                let current = serving.config.policy.open_policy()?.snapshot()?;
                let who = identity(&serving.config.registry, &header, &evidence)?;
                view.recheck(&current, &who, reads, quipu::time::epoch_secs())?;
                Ok(output)
            })
            .map_err(|_| quipu::Error::CannotVerify("composed read unavailable".into()))
    })
    .await;
    match result {
        Ok(Ok(output)) => {
            let content_type = output.content_type();
            (
                [
                    (header::CONTENT_TYPE, content_type),
                    (header::CACHE_CONTROL, "private, no-store"),
                ],
                Body::from(Bytes::from_owner(Output(output))),
            )
                .into_response()
        }
        Ok(Err(quipu::Error::PolicyDenied(_) | quipu::Error::InvalidValue(_))) => deny(),
        _ => unavailable(),
    }
}
