//! Authenticated reads of one routed graph-set owner (aegis-n0wc.5.2).
//!
//! Mounted ONLY with `[quipu.graph_sets.reads]`: `POST /graph-sets/query`.
//! The authority sequence is the pilot's `/owner/query` (`owner_pilot_read`)
//! and reuses its helpers: one named bearer resolved in a separate registry, a
//! policy file on no data inode, a grant for every selected graph, an
//! admission permit held across the blocking queue, one strict snapshot, and
//! a recheck of policy and identity after evaluation. Nothing is discovered
//! from a request.
//!
//! The selection is the members of exactly ONE routed owner. ROOT, legacy
//! members, two owners or a mix refuse 409 after authorization; there is no
//! legacy fallback. An optional composition `cut` (a routed `/update`
//! receipt's `owner.cut`) answers only at the owner's published head: a cut
//! behind head refuses 409 naming the requested and current cut, because
//! historical evaluation (vector mode) does not exist. Every answer carries
//! the cut it was read at in [`CUT_HEADER`]. Authorization runs BEFORE any cut
//! is interpreted, so an unauthorized caller never learns a head.
use crate::{
    base::AppError,
    knot_durable::owner::dataset::read::{
        ReadConfig, identity, load_registry, separate_policy_files,
    },
};
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{OriginalUri, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use quipu::{
    read_authorization::{ComposedQueryOutput, ComposedReadAdmission, ReadOperation},
    store::{
        composition_cut::CompositionCut,
        graph_owner::{GraphSetRouter, SetTarget},
    },
};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

/// Response header naming the cut an answer was read at.
pub(crate) const CUT_HEADER: &str = "quipu-composition-cut";
/// Most graphs one read may select (one owner's members).
const MAX_GRAPHS: usize = 64;
/// Longest accepted cut token.
const MAX_CUT_BYTES: usize = 16 * 1024;

#[derive(Clone)]
struct Serving {
    router: &'static GraphSetRouter,
    files: Arc<Vec<PathBuf>>,
    config: ReadConfig,
    admission: ComposedReadAdmission,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    query: String,
    graphs: Vec<String>,
    #[serde(default)]
    cut: Option<String>,
}

/// What an authorized read produced. Cut and selection refusals are outcomes,
/// not errors, so they keep their own status instead of collapsing to 503.
enum Outcome {
    Answer(ComposedQueryOutput, String),
    Spans,
    BadCut(String),
    CutRefused(String),
    Behind { requested: String, head: String },
}

/// The `/graph-sets/query` router over the process-lifetime `router`. Startup
/// refuses (never mounts a weaker read) when the authority is unusable.
/// `files` are every data file of the catalog: the policy may share none.
pub(crate) fn routes(
    router: &'static GraphSetRouter,
    files: Vec<PathBuf>,
    config: ReadConfig,
    admission: ComposedReadAdmission,
) -> Result<Router, AppError> {
    if config.timeout.is_zero() || config.timeout > Duration::from_secs(30) {
        return Err(refuse("bounded graph-set read timeout required"));
    }
    separate_policy_files(&files, &config)?;
    config.policy.open_policy()?.snapshot()?;
    load_registry(&config.registry)?;
    Ok(Router::new()
        .route("/graph-sets/query", post(query))
        .layer(axum::extract::DefaultBodyLimit::max(131_072))
        .with_state(Serving {
            router,
            files: Arc::new(files),
            config,
            admission,
        }))
}

fn refuse(message: &str) -> AppError {
    quipu::Error::PolicyDenied(format!("graph-set reads: {message}")).into()
}

/// The named kind in a `composition cut refused (<kind>): ...` error.
fn cut_kind(error: &quipu::Error) -> String {
    let text = error.to_string();
    text.split_once("composition cut refused (")
        .and_then(|(_, rest)| rest.split_once(')'))
        .map_or_else(|| "unknown".to_owned(), |(kind, _)| kind.to_owned())
}

struct Output(ComposedQueryOutput);
impl AsRef<[u8]> for Output {
    fn as_ref(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

fn status(code: StatusCode, body: serde_json::Value) -> Response {
    (code, Json(body)).into_response()
}

fn unavailable() -> Response {
    status(
        StatusCode::SERVICE_UNAVAILABLE,
        serde_json::json!({"error":"composed read unavailable", "outcome":"UNKNOWN"}),
    )
}

async fn query(
    State(serving): State<Serving>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    Json(input): Json<Input>,
) -> Response {
    let deny = || {
        status(
            StatusCode::FORBIDDEN,
            serde_json::json!({"error":"composed read denied"}),
        )
    };
    let Some(evidence) = crate::auth::request_identity() else {
        return deny();
    };
    if uri.query().is_some()
        || headers.get_all(header::AUTHORIZATION).iter().count() != 1
        || input.query.len() > 65_536
        || input.graphs.is_empty()
        || input.graphs.len() > MAX_GRAPHS
        || input.cut.as_ref().is_some_and(|c| c.len() > MAX_CUT_BYTES)
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
    // Deadline before the blocking queue; one reservation across it.
    let deadline = Instant::now() + serving.config.timeout;
    let Ok(permit) = serving.admission.try_reserve() else {
        return unavailable();
    };
    let graphs = input.graphs.clone();
    let result = crate::base::spawn_tracked(move || -> quipu::Result<Outcome> {
        let now = quipu::time::epoch_secs();
        let config = &serving.config;
        let who = identity(&config.registry, &header, &evidence)?;
        separate_policy_files(&serving.files, config)?;
        let policy = config.policy.open_policy()?.snapshot()?;
        let authorized = policy.authorize(&who, ReadOperation::Query, Some(&selected), now)?;
        // Authorized from here: selection and cut refusals may name graphs
        // and heads.
        let named: BTreeSet<Option<String>> = selected.iter().cloned().map(Some).collect();
        let owner = match serving.router.target_graphs(&named) {
            Ok(SetTarget::Routed(key)) => serving.router.routed_owner(key),
            _ => None,
        };
        let Some(owner) = owner else {
            return Ok(Outcome::Spans);
        };
        let requested = match input.cut.as_deref().map(CompositionCut::from_token) {
            None => None,
            Some(Ok(cut)) => Some(cut),
            Some(Err(error)) => return Ok(Outcome::BadCut(cut_kind(&error))),
        };
        owner
            .read_members(&selected, deadline, |reads| {
                let at = CompositionCut::capture(reads)?;
                if let Some(cut) = &requested {
                    if let Err(error) = cut.admit(reads) {
                        return Ok(Outcome::CutRefused(cut_kind(&error)));
                    }
                    if !cut.is_at_head(reads) {
                        return Ok(Outcome::Behind {
                            requested: cut.to_token(),
                            head: at.to_token(),
                        });
                    }
                }
                let view = authorized.open_composed_cut(reads, permit, now, deadline)?;
                let output = view.query(&input.query, false, now)?;
                separate_policy_files(&serving.files, &serving.config)?;
                let current = serving.config.policy.open_policy()?.snapshot()?;
                let who = identity(&serving.config.registry, &header, &evidence)?;
                view.recheck(&current, &who, reads, quipu::time::epoch_secs())?;
                Ok(Outcome::Answer(output, at.to_token()))
            })
            .map_err(|_| quipu::Error::CannotVerify("composed read unavailable".into()))
    })
    .await;
    match result {
        Ok(Ok(Outcome::Answer(output, cut))) => {
            let Ok(cut) = HeaderValue::from_str(&cut) else {
                return unavailable();
            };
            let content_type = output.content_type();
            (
                [
                    (header::CONTENT_TYPE, HeaderValue::from_static(content_type)),
                    (
                        header::CACHE_CONTROL,
                        HeaderValue::from_static("private, no-store"),
                    ),
                    (header::HeaderName::from_static(CUT_HEADER), cut),
                ],
                Body::from(Bytes::from_owner(Output(output))),
            )
                .into_response()
        }
        Ok(Ok(Outcome::Spans)) => status(
            StatusCode::CONFLICT,
            serde_json::json!({"error":"graph-set read spans files", "graphs": graphs}),
        ),
        Ok(Ok(Outcome::BadCut(kind))) => status(
            StatusCode::BAD_REQUEST,
            serde_json::json!({"error":"composition cut refused", "kind": kind}),
        ),
        Ok(Ok(Outcome::CutRefused(kind))) => status(
            StatusCode::CONFLICT,
            serde_json::json!({"error":"composition cut refused", "kind": kind}),
        ),
        Ok(Ok(Outcome::Behind { requested, head })) => status(
            StatusCode::CONFLICT,
            serde_json::json!({
                "error":"historical cut needs vector mode (not implemented)",
                "kind":"behind-head", "requested": requested, "head": head,
            }),
        ),
        Ok(Err(quipu::Error::PolicyDenied(_) | quipu::Error::InvalidValue(_))) => deny(),
        _ => unavailable(),
    }
}

#[cfg(test)]
#[path = "graph_set_read_tests.rs"]
mod tests;
