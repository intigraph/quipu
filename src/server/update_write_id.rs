//! Durable write IDs on routed `/update` (aegis-n0wc.5 H3): the retry answer
//! mapping and the `/update/status` and `/update/seal` handlers. Split from
//! `update.rs` to keep it under the file-size cap.
use axum::{
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
};

use super::update_route;
use crate::base::{AppError, blocking};

/// A repeated write ID's answer: the original complete receipt when the
/// attempt finished, the commit witness when it committed but its receipt was
/// never recorded, else 409 with its state (admitted = UNKNOWN custody).
pub(super) fn retried(status: serde_json::Value) -> axum::response::Response {
    match status["state"].as_str() {
        Some("finished") if status.get("receipt").is_some() => {
            axum::Json(status["receipt"].clone()).into_response()
        }
        Some("committed") => axum::Json(status).into_response(),
        _ => (StatusCode::CONFLICT, axum::Json(status)).into_response(),
    }
}

/// `/update/status` and `/update/seal` input: the original write ID and the
/// exact update text (aegis-n0wc.5 H3). The text is re-based on `/update` as
/// the original request was, so the binding matches only the same Host.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WriteIdInput {
    write_id: String,
    update: String,
}

fn rebased(headers: &HeaderMap, update: &str) -> String {
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    format!("BASE <http://{host}/update>\n{update}")
}

/// Exact recorded status of a routed update's write ID; `unknown` for any
/// other binding, principal or a non-routed update. Never a replay.
pub(crate) async fn update_status_post(
    headers: HeaderMap,
    axum::Json(input): axum::Json<WriteIdInput>,
) -> Result<axum::Json<serde_json::Value>, AppError> {
    let update = rebased(&headers, &input.update);
    blocking(move || write_id_status(&input.write_id, &update)).await
}

/// Seal a routed update's UNKNOWN (admitted, uncommitted) write ID so a late
/// worker can never commit it; the caller may then retry with a NEW ID.
pub(crate) async fn update_seal_post(
    headers: HeaderMap,
    axum::Json(input): axum::Json<WriteIdInput>,
) -> Result<axum::Json<serde_json::Value>, AppError> {
    let update = rebased(&headers, &input.update);
    blocking(move || write_id_seal(&input.write_id, &update)).await
}

#[cfg(unix)]
fn write_id_status(
    write_id: &str,
    update: &str,
) -> Result<axum::Json<serde_json::Value>, AppError> {
    Ok(axum::Json(
        match update_route::routed_binding(update_route::router(), write_id, update)? {
            Some((owner, binding)) => owner.update_status(&binding)?,
            None => serde_json::json!({
                "contract": quipu::store::knot_completion::update::STATUS_CONTRACT,
                "write_id": write_id, "state": "unknown"}),
        },
    ))
}

#[cfg(unix)]
fn write_id_seal(write_id: &str, update: &str) -> Result<axum::Json<serde_json::Value>, AppError> {
    let (owner, binding) = update_route::routed_binding(update_route::router(), write_id, update)?
        .ok_or_else(|| {
            quipu::Error::InvalidValue("write ID absent, committed or terminal".into())
        })?;
    Ok(axum::Json(owner.seal_update(&binding)?))
}

#[cfg(not(unix))]
fn write_id_status(_: &str, _: &str) -> Result<axum::Json<serde_json::Value>, AppError> {
    Err(quipu::Error::InvalidValue("routed write IDs need a unix build".into()).into())
}

#[cfg(not(unix))]
fn write_id_seal(_: &str, _: &str) -> Result<axum::Json<serde_json::Value>, AppError> {
    Err(quipu::Error::InvalidValue("routed write IDs need a unix build".into()).into())
}
