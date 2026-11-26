//! Bounded refresh of EXISTING entity vectors whose text is stale (aegis-xc7).
//!
//! `/embed_backfill_graph` fills only MISSING vectors, and `/embed_backfill`
//! rebuilds the whole store in one request. Neither re-embeds just the
//! entities whose stored vector text no longer matches what the current text
//! builder produces (for example after #1280 started keeping every comment of
//! the newest comment transaction). This path does exactly that, per call
//! bounded, resumable by cursor, and dry-run by default.

use super::super::SharedStore;
use super::super::base::{AppError, blocking};
use axum::extract::State;
use serde_json::{Value as JsonValue, json};

/// Hard per-call cap on entities examined.
pub(crate) const MAX_EXAMINED: usize = 256;
const BATCH_SIZE: usize = 32;

/// Which entities one call examines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Selection {
    /// Explicit entity ids (already resolved), at most [`MAX_EXAMINED`].
    Entities(Vec<i64>),
    /// Entities holding a current vector with id strictly after this cursor.
    After(i64),
}

/// Outcome of one bounded refresh call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RefreshOutcome {
    pub(crate) examined: usize,
    /// Re-embedded (or, on a dry run, would be re-embedded).
    pub(crate) changed: usize,
    pub(crate) unchanged: usize,
    /// No current vector: filling holes belongs to the backfill paths.
    pub(crate) missing_skipped: usize,
    /// No embeddable text any more: closing belongs to the write path.
    pub(crate) empty_skipped: usize,
    /// Text or vector changed concurrently between snapshot and apply.
    pub(crate) stale_skipped: usize,
    /// Resume point for the next call; `None` when the selection is exhausted.
    pub(crate) next_cursor: Option<i64>,
}

/// The text the entity's vector should be built from. ROOT entities get
/// exactly [`quipu::build_entity_text`]; a named-graph-only entity gets the
/// named-graph text its vector was prepared from by graph backfill.
fn entity_text(s: &quipu::Store, eid: i64) -> std::result::Result<String, String> {
    quipu::build_entity_text_for_graph_backfill(s, eid).map_err(|e| e.to_string())
}

/// Re-embed the selected entities whose current vector text differs from the
/// rebuilt text. Same guards as graph backfill: snapshot under the lock, ONNX
/// outside it, recheck before apply. Superseded rows are closed, not
/// overwritten, with ISO-8601 stamps comparable to fact cutoffs.
pub(crate) fn refresh_embeddings(
    store: &SharedStore,
    selection: &Selection,
    max_entities: usize,
    dry_run: bool,
) -> std::result::Result<RefreshOutcome, String> {
    let cap = max_entities.clamp(1, MAX_EXAMINED);
    let mut out = RefreshOutcome::default();
    let (provider, ids) = {
        let s = store.lock();
        let provider = if dry_run {
            None
        } else {
            Some(s.embedding_provider().ok_or(quipu::NO_PROVIDER_HELP)?)
        };
        let ids = match selection {
            Selection::Entities(ids) if ids.len() > cap => {
                return Err(format!(
                    "{} entities requested; at most {cap} per call",
                    ids.len()
                ));
            }
            Selection::Entities(ids) => ids.clone(),
            Selection::After(after) => {
                let mut page = s
                    .current_vector_entities_after(*after, cap + 1)
                    .map_err(|e| e.to_string())?;
                if page.len() > cap {
                    page.truncate(cap);
                    out.next_cursor = page.last().copied();
                }
                page
            }
        };
        (provider, ids)
    };
    for chunk in ids.chunks(BATCH_SIZE) {
        // (entity, rebuilt text, stored text it replaces)
        let work: Vec<(i64, String, String)> = {
            let s = store.lock();
            let vs = s.vector_store();
            let mut work = Vec::new();
            for &eid in chunk {
                out.examined += 1;
                let Some(stored) = vs.current_embedding_text(eid).map_err(|e| e.to_string())?
                else {
                    out.missing_skipped += 1;
                    continue;
                };
                let text = entity_text(&s, eid)?;
                if text.is_empty() {
                    out.empty_skipped += 1;
                } else if text == stored {
                    out.unchanged += 1;
                } else {
                    work.push((eid, text, stored));
                }
            }
            work
        };
        let Some(provider) = provider.as_ref().filter(|_| !work.is_empty()) else {
            out.changed += work.len();
            continue;
        };
        let texts: Vec<&str> = work.iter().map(|(_, text, _)| text.as_str()).collect();
        let embs = provider.embed_batch(&texts).map_err(|e| e.to_string())?;
        let s = store.lock();
        let vs = s.vector_store();
        for ((eid, text, stored), emb) in work.iter().zip(embs.iter()) {
            let still_stored = vs.current_embedding_text(*eid).map_err(|e| e.to_string())?;
            if entity_text(&s, *eid)? != *text || still_stored.as_deref() != Some(stored.as_str()) {
                out.stale_skipped += 1;
                continue;
            }
            let ts = quipu::time::now_iso();
            vs.close_embedding(*eid, &ts).map_err(|e| e.to_string())?;
            vs.embed_entity(*eid, text, emb, &ts)
                .map_err(|e| e.to_string())?;
            out.changed += 1;
        }
    }
    Ok(out)
}

fn parse_selection(s: &quipu::Store, input: &JsonValue) -> quipu::Result<(Selection, Vec<String>)> {
    let Some(list) = input.get("entities") else {
        let after = input.get("after").and_then(JsonValue::as_i64).unwrap_or(0);
        return Ok((Selection::After(after), Vec::new()));
    };
    if input.get("after").is_some() {
        return Err(quipu::Error::InvalidValue(
            "give entities or after, not both".into(),
        ));
    }
    let list = list
        .as_array()
        .ok_or_else(|| quipu::Error::InvalidValue("entities must be an array of IRIs".into()))?;
    let (mut ids, mut unknown) = (Vec::new(), Vec::new());
    for iri in list {
        let iri = iri
            .as_str()
            .ok_or_else(|| quipu::Error::InvalidValue("entities must be IRI strings".into()))?;
        match s.lookup(iri)? {
            Some(id) if !ids.contains(&id) => ids.push(id),
            Some(_) => {}
            None => unknown.push(iri.to_string()),
        }
    }
    Ok((Selection::Entities(ids), unknown))
}

pub(crate) async fn embed_refresh(
    State(store): State<SharedStore>,
    axum::Json(input): axum::Json<JsonValue>,
) -> std::result::Result<axum::Json<JsonValue>, AppError> {
    blocking(move || {
        let max_entities = input
            .get("max_entities")
            .and_then(JsonValue::as_u64)
            .map_or(MAX_EXAMINED, |n| usize::try_from(n).unwrap_or(usize::MAX))
            .clamp(1, MAX_EXAMINED);
        let dry_run = input
            .get("dry_run")
            .and_then(JsonValue::as_bool)
            .unwrap_or(true);
        let (selection, unknown) = parse_selection(&store.lock(), &input)?;
        let o = refresh_embeddings(&store, &selection, max_entities, dry_run)
            .map_err(quipu::Error::InvalidValue)?;
        let mut body = json!({
            "status": "ok",
            "dry_run": dry_run,
            "max_entities": max_entities,
            "examined": o.examined,
            "unchanged": o.unchanged,
            "missing_skipped": o.missing_skipped,
            "empty_skipped": o.empty_skipped,
            "stale_skipped": o.stale_skipped,
            "next_cursor": o.next_cursor,
            "unknown_entities": unknown,
        });
        body[if dry_run { "would_change" } else { "changed" }] = json!(o.changed);
        Ok(axum::Json(body))
    })
    .await
}
