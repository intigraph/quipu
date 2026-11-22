//! Authenticated owner observation, without enrollment or opening another store.
use axum::response::{IntoResponse, Response};
use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    http::StatusCode,
    routing::get,
};
use quipu::http_auth::AuthenticatedPrincipal;
use serde::Deserialize;

use super::{SharedStore, admission::read_blocking, base::AppError};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    graph: String,
}

pub(super) fn routes() -> Router<SharedStore> {
    Router::new().route("/graph/writer-instance", get(observe))
}

async fn observe(
    State(store): State<SharedStore>,
    identity: Option<Extension<AuthenticatedPrincipal>>,
    Query(input): Query<Input>,
) -> Response {
    // Defense in depth: an accidentally unguarded router cannot expose pins.
    if identity.is_none() {
        return (StatusCode::UNAUTHORIZED, "authentication required").into_response();
    }
    if input.graph.is_empty() || input.graph.len() > 2048 {
        return AppError::from(quipu::Error::InvalidValue("invalid graph selector".into()))
            .into_response();
    }
    read_blocking(move || {
        let instance = store.read().writer_graph_instance(&input.graph)?;
        Ok(Json(serde_json::json!(instance)))
    })
    .await
    .into_response()
}

#[cfg(test)]
#[path = "writer_instance_tests.rs"]
mod tests;
