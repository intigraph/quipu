//! Explicit v2 transport. Legacy v1 never claims durable admission retroactively.
use super::{SharedStore, base::AppError};
use axum::{Json, http::HeaderMap};
use quipu::store::knot_completion::Binding;
use serde_json::Value;

pub(crate) const CONTRACT: &str = "quipu.knot-completion/v2";
const CONTRACT_HEADER: &str = "x-quipu-completion-contract";
const ID_HEADER: &str = "x-quipu-write-id";

pub(crate) fn requested(headers: &HeaderMap) -> Result<bool, AppError> {
    let Some(value) = headers.get(CONTRACT_HEADER) else {
        return Ok(false);
    };
    if value.to_str().ok() != Some(CONTRACT) || headers.get_all(CONTRACT_HEADER).iter().count() != 1
    {
        return Err(quipu::Error::InvalidValue("unsupported completion contract".into()).into());
    }
    Ok(true)
}

fn binding(id: String, request: &Value) -> Result<Binding, AppError> {
    Ok(quipu::transaction_auth::with_identity(
        super::auth::request_identity(),
        || Binding::for_request(id, request),
    )?)
}

pub(crate) async fn knot(
    store: SharedStore,
    headers: HeaderMap,
    input: Value,
) -> Result<Json<Value>, AppError> {
    let id = headers
        .get(ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|_| headers.get_all(ID_HEADER).iter().count() == 1)
        .ok_or_else(|| {
            quipu::Error::InvalidValue("one write ID is required for durable admission".into())
        })?
        .to_owned();
    let bound = binding(id, &input)?;
    // Retained legacy IDs cannot be upgraded into another execution. Keep the
    // same process-local worker truth while the durable path owns the effect.
    let mut ephemeral = super::knot_completion::begin(&store, &headers, &input)?;
    let receiver = store.clone();
    // One permit owns admission, effect, deferred work and terminal recording.
    // Cancellation cannot release it while the blocking worker is still alive.
    // The durable admission still commits before the effect: a crash between
    // them retains admitted/UNKNOWN custody, never permission to retry.
    super::admission::write_blocking(move || {
        let ticket = receiver.write_lock()?.admit_knot_attempt(bound)?;
        if let Some(worker) = &mut ephemeral {
            worker.running();
        }
        let outcome = (|| {
            let (result, work) = {
                let mut writer = store.write_lock()?;
                let targets = super::update::writer_targets::json(&input);
                let result = writer.with_writer_scope(
                    quipu::writer_scope::WriteOperation::Knot,
                    &targets,
                    |writer| quipu::mcp::knot::tool_knot_with_completion(writer, &input, &ticket),
                );
                let work = if result.is_ok() {
                    if quipu::writer_scope::is_scoped_request() {
                        let _ = writer.take_deferred_embed();
                        None
                    } else {
                        writer.take_deferred_embed()
                    }
                } else {
                    None
                };
                (result?, work)
            };
            if let Some(work) = work {
                super::tools::finish_deferred_embed(&store, &work)?;
            }
            Ok::<Value, AppError>(super::input_fields::annotate("quipu_knot", &input, result))
        })();
        // Any persistence failure leaves the atomic commit witness available;
        // never fall back to a process-local finished claim.
        store
            .write_lock()?
            .finish_knot_attempt(&ticket, outcome.as_ref().ok())?;
        if let Some(worker) = &mut ephemeral {
            worker.finish(outcome.as_ref().ok());
        }
        outcome.map(Json)
    })
    .await
}

pub(crate) async fn status(store: SharedStore, input: Value) -> Result<Json<Value>, AppError> {
    let id = input
        .get("write_id")
        .and_then(Value::as_str)
        .ok_or_else(|| quipu::Error::InvalidValue("missing write_id".into()))?
        .to_owned();
    let request = input
        .get("request")
        .ok_or_else(|| quipu::Error::InvalidValue("missing original request binding".into()))?;
    let bound = binding(id, request)?;
    super::admission::read_blocking(move || {
        Ok(Json(store.point_read().knot_attempt_status(&bound)?))
    })
    .await
}

#[cfg(test)]
#[path = "knot_durable_tests.rs"]
mod tests;
