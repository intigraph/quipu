//! Whole-store embedding backfill and its `/embed_backfill` handler. Split
//! from `tools.rs`.

use axum::extract::State;
use serde_json::{Value as JsonValue, json};

use super::super::SharedStore;
use super::super::base::{AppError, blocking};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BackfillResult {
    pub(crate) embedded: usize,
    pub(crate) stale_retries: usize,
}

/// Rebuild the semantic-search corpus without monopolising the writer lock.
///
/// Each bounded batch is snapshotted under the lock, embedded outside it, and
/// conditionally applied under a fresh lock.  An entity changed during the
/// unlocked ONNX pass is requeued with its new text/version; a vector computed
/// from an obsolete snapshot is never written and the edit is never silently
/// omitted from the completed backfill.
pub(crate) fn backfill_embeddings(
    store: &SharedStore,
) -> std::result::Result<BackfillResult, String> {
    const BATCH_SIZE: usize = 32;

    let (provider, entity_ids) = {
        let s = store.lock();
        let provider = s.embedding_provider().ok_or(quipu::NO_PROVIDER_HELP)?;
        // Do not route a whole-store maintenance scan through the SPARQL evaluator:
        // the production graph exceeds its query budget before embedding starts.
        let entity_ids: std::collections::BTreeSet<i64> = s
            .current_facts()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|fact| fact.entity)
            .collect();
        (provider, entity_ids)
    };
    if entity_ids.is_empty() {
        return Ok(BackfillResult {
            embedded: 0,
            stale_retries: 0,
        });
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();
    let mut embedded = 0usize;
    let mut stale_retries = 0usize;
    let mut pending: std::collections::VecDeque<i64> = entity_ids.into_iter().collect();

    while !pending.is_empty() {
        let ids: Vec<i64> = (0..BATCH_SIZE)
            .filter_map(|_| pending.pop_front())
            .collect();
        // BOUNDED writer-lock window: capture both the exact text and the
        // newest contributing fact transaction.  The transaction version
        // catches change-then-revert races that text equality alone misses.
        let snapshots: Vec<(i64, String, i64)> = {
            let s = store.lock();
            let mut snapshots = Vec::with_capacity(ids.len());
            for eid in ids {
                let facts = s.entity_facts(eid).map_err(|e| e.to_string())?;
                let Some(version) = facts.iter().map(|f| f.tx).max() else {
                    continue;
                };
                let text = quipu::build_entity_text(&s, eid).map_err(|e| e.to_string())?;
                if !text.is_empty() {
                    snapshots.push((eid, text, version));
                }
            }
            snapshots
        };
        if snapshots.is_empty() {
            continue;
        }
        let texts: Vec<&str> = snapshots.iter().map(|(_, text, _)| text.as_str()).collect();
        // Expensive ONNX work is deliberately LOCK-FREE.  BATCH_SIZE remains
        // the existing RSS bound: one call can never scale with corpus size.
        let embs = provider.embed_batch(&texts).map_err(|e| e.to_string())?;

        // BOUNDED writer-lock window: apply only snapshots that are still
        // current.  Requeue stale entities so a concurrent edit is eventually
        // embedded rather than overwritten or silently skipped.
        {
            let s = store.lock();
            let vs = s.vector_store();
            for ((eid, text, version), emb) in snapshots.iter().zip(embs.iter()) {
                let current_facts = s.entity_facts(*eid).map_err(|e| e.to_string())?;
                let current_version = current_facts.iter().map(|f| f.tx).max();
                let current_text = quipu::build_entity_text(&s, *eid).map_err(|e| e.to_string())?;
                if current_version != Some(*version) || current_text != *text {
                    pending.push_back(*eid);
                    stale_retries += 1;
                    continue;
                }
                // Backfill is also the repair path for stale corpus entries. Close
                // the prior current row first; otherwise both remain current and
                // the search text lookup may return the obsolete one (aegis-eldb1).
                vs.close_embedding(*eid, &ts).map_err(|e| e.to_string())?;
                vs.embed_entity(*eid, text, emb, &ts)
                    .map_err(|e| e.to_string())?;
                embedded += 1;
            }
        }
    }
    Ok(BackfillResult {
        embedded,
        stale_retries,
    })
}

pub(crate) async fn embed_backfill(
    State(store): State<SharedStore>,
) -> std::result::Result<axum::Json<JsonValue>, AppError> {
    blocking(move || match backfill_embeddings(&store) {
        Ok(outcome) => Ok(axum::Json(json!({
            "status": "ok",
            "entities_embedded": outcome.embedded,
            "stale_retries": outcome.stale_retries,
        }))),
        Err(e) => Ok(axum::Json(json!({"status": "error", "error": e}))),
    })
    .await
}
