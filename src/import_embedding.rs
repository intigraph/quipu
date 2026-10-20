//! Index only eligible staged entities, without adopting them into ROOT.
use crate::embedding_runtime::EmbeddingRecipe;
use crate::share_import::ShareImportResult;
use crate::{DeferredEmbed, Result, Store};
use serde::{Deserialize, Serialize};

/// Index readiness is distinct from graph staging and ROOT admission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportEmbeddingStatus {
    /// ready, pending, partial, disabled, unavailable, failed, external or quarantined.
    pub state: String,
    /// Eligible entities with searchable text in the selected staging graph.
    pub candidates: usize,
    /// Entities whose current vectors have the selected graph text.
    pub indexed: usize,
    /// Entities already owned by ROOT or another graph; their vectors are preserved.
    pub skipped_shared_entities: usize,
    /// Identity of the actual local provider, when it can prove its file bytes.
    pub recipe: Option<EmbeddingRecipe>,
    /// Explicit limitations or failure details; staging alone is not readiness.
    pub message: Option<String>,
}
impl Default for ImportEmbeddingStatus {
    fn default() -> Self {
        Self {
            state: "not_reported".into(),
            candidates: 0,
            indexed: 0,
            skipped_shared_entities: 0,
            recipe: None,
            message: None,
        }
    }
}

pub(crate) fn verify_recipe(
    store: &Store,
    recipe: Option<&EmbeddingRecipe>,
    automatic: bool,
) -> Result<()> {
    if let Some(recipe) = recipe {
        recipe.validate()?;
        if automatic && let Some(provider) = store.embedding_provider() {
            match provider.recipe() {
                Some(actual) if actual == *recipe => {},
                _ => return Err(crate::Error::InvalidValue("pendant embedding model/tokenizer digest or dimension does not match the available provider; nothing was staged".into())),
            }
        }
    }
    Ok(())
}

fn documents(store: &Store, graph: i64) -> Result<(Vec<(i64, String)>, usize)> {
    let mut out = Vec::new();
    let mut skipped = 0;
    for entity in store.entities_in_graph(graph)? {
        // There is one current vector per entity. Never overwrite a ROOT or
        // other graph's vector using an unadopted incoming description.
        let elsewhere: bool = store.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM facts WHERE e=?1 AND g!=?2 AND op=1 AND valid_to IS NULL)",
            rusqlite::params![entity, graph],
            |r| r.get(0),
        )?;
        if elsewhere {
            skipped += 1;
            continue;
        }
        let text = crate::embedding::build_entity_text_in_graph(store, entity, graph)?;
        if !text.is_empty() {
            out.push((entity, text));
        }
    }
    Ok((out, skipped))
}

pub(crate) fn prepare(
    store: &mut Store,
    result: &mut ShareImportResult,
    timestamp: &str,
) -> Result<()> {
    let status = &mut result.embedding;
    if !result.promotion.eligible {
        status.state = "quarantined".into();
        return Ok(());
    }
    if !store.embedding_config().auto_embed {
        status.state = "disabled".into();
        return Ok(());
    }
    let Some(provider) = store.embedding_provider() else {
        status.state = "unavailable".into();
        status.message = Some("No embedding model/provider configured: graph staged, semantic index unavailable; configure model_path and tokenizer_path or explicitly set auto_embed=false".into());
        return Ok(());
    };
    if store.vector_delegate.is_some() {
        status.state = "external".into();
        status.message = Some("Vector indexing belongs to the configured external delegate; local readiness not established".into());
        return Ok(());
    }
    status.recipe = provider.recipe();
    let graph = store
        .lookup(&result.staging_graph)?
        .ok_or_else(|| crate::Error::Store("missing staging graph".into()))?;
    let (docs, skipped) = documents(store, graph)?;
    status.candidates = docs.len();
    status.skipped_shared_entities = skipped;
    let mut pending = Vec::new();
    let vectors = store.vector_store();
    // A text match cannot prove that a vector used the current model. Real
    // providers are re-run on import; no cache or configured filename is
    // accepted as provider-byte provenance.
    let same_model = status.recipe.is_none();
    for (entity, text) in docs {
        if same_model && vectors.current_embedding_text(entity)?.as_deref() == Some(text.as_str()) {
            status.indexed += 1;
        } else {
            pending.push((entity, text));
        }
    }
    let work = DeferredEmbed::for_graph(pending, graph, timestamp);
    let queued = !work.is_empty();
    if store.defer_auto_embed {
        if !work.is_empty() {
            match &mut store.pending_embed {
                Some(previous) => previous.merge(work),
                None => store.pending_embed = Some(work),
            }
        }
    } else {
        let texts = work.texts();
        let mut embeddings = Vec::new();
        for chunk in texts.chunks(store.embedding_config().embed_batch_size.max(1)) {
            embeddings.extend(provider.embed_batch(chunk)?);
        }
        store.apply_deferred_embed(&work, &embeddings)?;
    }
    if store.defer_auto_embed && queued {
        status.state = "pending".into();
        // Old vectors, even with identical text, do not certify a newly
        // selected model before its lock-free inference has completed.
        return Ok(());
    }
    refresh(store, result)
}

/// Called after the server's lock-free deferred drain, so pending is not ready.
pub fn refresh(store: &Store, result: &mut ShareImportResult) -> Result<()> {
    if matches!(
        result.embedding.state.as_str(),
        "disabled" | "unavailable" | "external" | "quarantined"
    ) {
        return Ok(());
    }
    let graph = store
        .lookup(&result.staging_graph)?
        .ok_or_else(|| crate::Error::Store("missing staging graph".into()))?;
    let (docs, skipped) = documents(store, graph)?;
    let vectors = store.vector_store();
    let mut indexed = 0;
    for (entity, text) in &docs {
        if vectors.current_embedding_text(*entity)?.as_deref() == Some(text.as_str()) {
            indexed += 1;
        }
    }
    result.embedding.candidates = docs.len();
    result.embedding.indexed = if result.embedding.state == "failed" {
        0
    } else {
        indexed
    };
    result.embedding.skipped_shared_entities = skipped;
    let failed = result.embedding.state == "failed";
    result.embedding.state = if failed {
        "failed"
    } else if indexed < docs.len() {
        "pending"
    } else if skipped > 0 {
        "partial"
    } else {
        "ready"
    }
    .into();
    if skipped > 0 && !failed {
        result.embedding.message = Some("Shared entity IRIs were not reindexed from unadopted text; existing vectors preserved. Promotion remains a separate explicit operation".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
