//! `GET /namespaces`: the entity-namespace registry as a plain read. The POST
//! form and `/entity_ref` stay in tools.rs with the other tool handlers.
use axum::extract::State;
use serde_json::{Value as JsonValue, json};

use super::SharedStore;
use super::admission::read_blocking;
use super::base::AppError;

pub(crate) async fn namespaces_get(
    State(store): State<SharedStore>,
) -> Result<axum::Json<JsonValue>, AppError> {
    read_blocking(move || {
        Ok(axum::Json(quipu::entity_reference::tool_namespaces(
            &store.read(),
            &json!({}),
        )?))
    })
    .await
}
