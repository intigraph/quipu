//! Scheduled materialisation with independent admission and short apply batches.

use serde_json::{Value, json};

use super::SharedStore;
use super::base::{AppError, blocking};
use quipu::owl::snapshot::Snapshot;

#[path = "owl_publish.rs"]
mod publish;

// Not WRITE_ADMISSION: holding that permit during derivation would block every
// ordinary write even with the store mutex released. Reject overlap rather than
// queueing additional uncancellable, memory-heavy derivations.
static MATERIALIZE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

pub(crate) async fn materialize(
    store: SharedStore,
    input: Value,
) -> Result<axum::Json<Value>, AppError> {
    let permit = admit(&store)?;
    run(store, timestamp_of(&input), permit).await
}

/// Single-flight admission plus the read-pool requirement, separate from the
/// run so a deferred `/ontology` load (aegis-fej9) can be refused for either
/// reason BEFORE it persists anything.
pub(crate) fn admit(
    store: &SharedStore,
) -> Result<tokio::sync::SemaphorePermit<'static>, quipu::Error> {
    let permit = MATERIALIZE
        .try_acquire()
        .map_err(|_| quipu::Error::InvalidValue("OWL materialisation already running".into()))?;
    if store.readers.len() == 0 {
        return Err(quipu::Error::InvalidValue(
            "scheduled OWL materialisation requires a read pool; refusing writer fallback".into(),
        ));
    }
    Ok(permit)
}

pub(crate) fn timestamp_of(input: &Value) -> String {
    input
        .get("timestamp")
        .and_then(Value::as_str)
        .map_or_else(quipu::time::now_iso, str::to_owned)
}

pub(crate) async fn run(
    store: SharedStore,
    timestamp: String,
    permit: tokio::sync::SemaphorePermit<'static>,
) -> Result<axum::Json<Value>, AppError> {
    let started = std::time::Instant::now();
    let reader = store.clone();
    // Move the permit into blocking work: a disconnected client must not release
    // single-flight admission while its derivation still consumes resources.
    let (snapshot, permit) = blocking(move || {
        let mut snapshot = {
            let source = reader.read();
            Snapshot::capture_or_resume(&source, &timestamp)?
        };
        snapshot.derive_or_resume()?;
        Ok((snapshot, permit))
    })
    .await?;
    let publish::Published {
        snapshot,
        permit,
        applied,
        max_writer_batch_micros,
    } = publish::publish(store, snapshot, permit).await?;
    blocking(move || {
        let _permit = permit;
        let r = &snapshot.report;
        let response = axum::Json(json!({
            "action": "materialize", "ontologies": snapshot.ontology_count(),
            "complete": true, "snapshot_tx": snapshot.premise_head,
            "elapsed_ms": started.elapsed().as_millis(), "applied_proposals": applied,
            "max_writer_batch_micros": max_writer_batch_micros,
            "materialized": if snapshot.ontology_count() == 0 { Value::Null } else { json!({
                "total": r.total, "passes": r.passes,
                "raw_proposals": r.raw_proposals, "known_proposals": r.known_proposals,
                "repeated_proposals": r.repeated_proposals,
                "subclass_inferences": r.subclass_inferences,
                "same_as_inferences": r.same_as_inferences,
                "sub_property_inferences": r.sub_property_inferences,
                "inverse_inferences": r.inverse_inferences,
                "symmetric_inferences": r.symmetric_inferences,
                "equivalent_class_inferences": r.equivalent_class_inferences,
                "domain_range_inferences": r.domain_range_inferences,
                "transitive_inferences": r.transitive_inferences,
                "equivalent_property_inferences": r.equivalent_property_inferences
            })}
        }));
        // Close the completed scratch database before releasing single-flight;
        // the next invocation may replace that completed generation's file.
        drop(snapshot);
        Ok(response)
    })
    .await
}

/// `/ontology` dispatch: `materialize` and a deferred `load` take the off-lock
/// path here; every other action is the locked library tool, unchanged.
#[cfg(feature = "owl")]
pub(crate) async fn dispatch(
    s: SharedStore,
    headers: axum::http::HeaderMap,
    matched: Option<axum::extract::MatchedPath>,
    i: Value,
) -> Result<axum::Json<Value>, AppError> {
    match (
        i.get("action").and_then(Value::as_str),
        i.get("materialize").and_then(Value::as_str),
    ) {
        (Some("materialize"), _) => materialize(s, i).await,
        (Some("load"), Some("deferred")) => load_deferred(s, headers, matched, i).await,
        _ => {
            super::tools::ontology_locked(axum::extract::State(s), headers, matched, axum::Json(i))
                .await
        }
    }
}

// aegis-fej9 (gju5 Q1). An inline load derives every entailment while it holds
// the writer: measured on a copy of a production store, 44 s for a 40-axiom
// ontology and 414 s for the full bridge, with every write blocked throughout.
// Under validate_on_write, that one transaction also exceeded the post-state
// SHACL cap, so it failed after holding the writer. A deferred load persists
// the ontology under the lock (cheap), then derives off-lock from a snapshot
// and applies it in short batches. This is the same path as `action: materialize`,
// over the combined ontology the write gate already reasons with.
//
// Order matters: admission (single flight + read pool) is taken BEFORE the
// persist, so the two predictable refusals write nothing. A derivation failure
// AFTER the persist cannot un-load the ontology. It is reported as a successful
// load with `materialization.complete = false` and the retry action, never as
// an error status for a write that committed.
#[cfg(feature = "owl")]
pub(crate) async fn load_deferred(
    s: SharedStore,
    headers: axum::http::HeaderMap,
    matched: Option<axum::extract::MatchedPath>,
    i: Value,
) -> Result<axum::Json<Value>, AppError> {
    let permit = admit(&s)?;
    let timestamp = timestamp_of(&i);
    let mut input = i;
    // The load and its derivation share one timestamp.
    input["timestamp"] = Value::String(timestamp.clone());
    let mut loaded = super::tools::ontology_locked(
        axum::extract::State(s.clone()),
        headers,
        matched,
        axum::Json(input),
    )
    .await?
    .0;
    match run(s, timestamp, permit).await {
        Ok(axum::Json(run)) => {
            loaded["materialized"] = run["materialized"].clone();
            loaded["materialization"] = json!({
                "complete": run["complete"],
                "snapshot_tx": run["snapshot_tx"],
                "elapsed_ms": run["elapsed_ms"],
                "applied_proposals": run["applied_proposals"],
            });
        }
        Err(error) => {
            loaded["materialization"] = json!({
                "complete": false,
                "error": error.message(),
                "retry": {"action": "materialize"},
            });
        }
    }
    Ok(axum::Json(loaded))
}

#[cfg(test)]
#[path = "owl_materialize_tests.rs"]
mod tests;
