//! The three mapping routes share the native MCP handlers and their semantics.

use axum::{Json, Router, routing::post};
use serde_json::Value;

use super::{
    SharedStore,
    base::{AppError, blocking},
};

pub(crate) fn routes() -> Router<SharedStore> {
    Router::new()
        .route("/mapping/propose", post(super::tools::mapping_propose))
        .route("/mapping/decide", post(decide))
        .route("/mapping/apply", post(super::tools::mapping_apply))
}

async fn decide(Json(input): Json<Value>) -> Result<Json<Value>, AppError> {
    blocking(move || Ok(Json(quipu::tool_mapping_decide(&input)?))).await
}

#[cfg(test)]
#[path = "mapping_tests.rs"]
mod tests;
