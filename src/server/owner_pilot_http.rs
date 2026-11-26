//! HTTP adapter for one prepared owner. Main mounts it only when the default-OFF
//! `[quipu.ingest_pilot]` startup admits an owner (aegis-4qv), inside the
//! ordinary authentication middleware. Its paths are distinct from legacy
//! `/knot`, so a request never selects the other writer by accident.
use super::{Pin, PreparedOwner, refuse};
use crate::base::AppError;
use axum::{Json, Router, extract::State, http::HeaderMap, routing::post};
use serde_json::Value;
use std::sync::Arc;

fn one<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, AppError> {
    if headers.get_all(name).iter().count() != 1 {
        return Err(refuse("exactly one owner request pin required"));
    }
    headers
        .get(name)
        .and_then(|h| h.to_str().ok())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| refuse("invalid owner request pin"))
}

fn pin(headers: &HeaderMap) -> Result<Pin, AppError> {
    if !crate::knot_durable::requested(headers)? {
        return Err(refuse("owner HTTP requires v2 completion contract"));
    }
    let number = |name| {
        one(headers, name)?
            .parse::<u64>()
            .map_err(|_| refuse("invalid owner revision or generation"))
    };
    Ok(Pin {
        catalog_revision: number("x-quipu-catalog-revision")?,
        generation: number("x-quipu-owner-generation")?,
        policy: one(headers, "x-quipu-owner-policy")?.to_owned(),
        signed: headers.contains_key(crate::auth::write_attest::HEADER),
        cross_graph_context: false,
    })
}

/// No listener or fallback is installed here; the caller mounts this router.
/// The same Arc serves effect/status/seal; none acquire legacy admission.
pub(crate) fn routes(owner: Arc<PreparedOwner>) -> Router {
    Router::new()
        .route("/owner/knot", post(owner_knot))
        .route("/owner/knot/status", post(owner_status))
        .route("/owner/knot/seal", post(owner_seal))
        .with_state(owner)
}

async fn owner_knot(
    State(owner): State<Arc<PreparedOwner>>,
    headers: HeaderMap,
    Json(input): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let pin = pin(&headers)?;
    let id = one(&headers, "x-quipu-write-id")?.to_owned();
    owner.knot(pin, input, id).await.map(Json)
}

fn binding(input: &Value) -> Result<(Value, String), AppError> {
    let id = input
        .get("write_id")
        .and_then(Value::as_str)
        .ok_or_else(|| refuse("missing original owner write ID"))?;
    let request = input
        .get("request")
        .ok_or_else(|| refuse("missing original owner request"))?;
    Ok((request.clone(), id.to_owned()))
}

async fn owner_status(
    State(owner): State<Arc<PreparedOwner>>,
    headers: HeaderMap,
    Json(input): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let pin = pin(&headers)?;
    let (request, id) = binding(&input)?;
    owner.status(pin, request, id).await.map(Json)
}

async fn owner_seal(
    State(owner): State<Arc<PreparedOwner>>,
    headers: HeaderMap,
    Json(input): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let pin = pin(&headers)?;
    let (request, id) = binding(&input)?;
    owner.seal(pin, request, id).await.map(Json)
}
