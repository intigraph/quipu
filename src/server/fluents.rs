//! HTTP adapter to the native append-only fluent contract.
use super::{
    SharedStore, admission, auth,
    base::{AppError, blocking},
};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use quipu::fluent::transport::AppendRequest;
use serde::Deserialize;
use serde_json::json;

pub(crate) fn append_route() -> axum::routing::MethodRouter<SharedStore> {
    axum::routing::post(append).layer(axum::extract::DefaultBodyLimit::max(65536))
}

pub(crate) async fn append(
    State(store): State<SharedStore>,
    axum::Json(request): axum::Json<AppendRequest>,
) -> Result<Response, AppError> {
    // An open-write server or a legacy shared token supplies no native IRI.
    // Never derive this identity from headers declared by the client or JSON.
    let Some(identity) = auth::request_identity() else {
        return Ok((
            StatusCode::UNAUTHORIZED,
            "authenticated writer IRI required",
        )
            .into_response());
    };
    if oxrdf::NamedNode::new(&identity.principal).is_err() {
        return Ok((
            StatusCode::UNAUTHORIZED,
            "authenticated writer IRI required",
        )
            .into_response());
    }
    admission::write_blocking(move || {
        let mut live = store.lock();
        let tx = live.append_fluent_observation(&request)?;
        Ok(axum::Json(json!({"tx": tx})).into_response())
    })
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CurrentQuery {
    graph: String,
    fluent: String,
}

pub(crate) async fn current(
    State(store): State<SharedStore>,
    Query(query): Query<CurrentQuery>,
) -> Result<Response, AppError> {
    blocking(move || {
        let live = store.read();
        Ok(axum::Json(live.fluent_snapshot(&query.graph, &query.fluent)?).into_response())
    })
    .await
}

#[cfg(test)]
#[path = "fluent_transport_tests.rs"]
mod tests;
