//! HTTP transport for resolving share-merge conflicts (aegis-yavo9c).
//!
//! The handlers call the same `tool_merge_*` functions the MCP tools do.
//! `/merge/decisions` and `/merge/resolve` are READs (resolve edits only the
//! posted file); `/merge/apply` is the only writer.

use axum::{Json, Router, extract::State, routing::post};
use serde_json::Value as JsonValue;

use crate::{
    SharedStore,
    base::{AppError, blocking},
};

pub(crate) fn routes() -> Router<SharedStore> {
    Router::new()
        .route("/merge/decisions", post(merge_decisions))
        .route("/merge/apply", post(merge_apply))
        .route("/merge/resolve", post(merge_resolve))
}

/// READ: emits (and optionally proposes) decisions; writes nothing. Served from
/// the read pool: a full ROOT export must not hold the writer (as `/export`).
async fn merge_decisions(
    State(store): State<SharedStore>,
    Json(input): Json<JsonValue>,
) -> Result<Json<JsonValue>, AppError> {
    blocking(move || {
        let st = store.read();
        Ok(Json(crate::input_fields::annotate(
            "quipu_merge_decisions",
            &input,
            quipu::tool_merge_decisions(&st, &input)?,
        )))
    })
    .await
}

/// WRITE: one transaction, or nothing.
async fn merge_apply(
    State(store): State<SharedStore>,
    Json(input): Json<JsonValue>,
) -> Result<Json<JsonValue>, AppError> {
    blocking(move || {
        let mut st = store.lock();
        Ok(Json(crate::input_fields::annotate(
            "quipu_merge_apply",
            &input,
            quipu::tool_merge_apply(&mut st, &input)?,
        )))
    })
    .await
}

/// Pure: records one choice in the posted decisions file; touches no store.
async fn merge_resolve(Json(input): Json<JsonValue>) -> Result<Json<JsonValue>, AppError> {
    blocking(move || {
        Ok(Json(crate::input_fields::annotate(
            "quipu_merge_resolve",
            &input,
            quipu::tool_merge_resolve(&input)?,
        )))
    })
    .await
}
