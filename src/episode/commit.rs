//! The commit half of an episode write whose resolution is already known.

use super::{Episode, IngestResult, ingest_episode_outcome_with_hints};
use crate::error::Result;
use crate::resolution;
use crate::store::Store;

/// Commit an episode whose resolution was already computed.
///
/// The second half of [`ingest_episode_with_resolution`], and the commit phase
/// of a two-phase `/episode` (aegis-xi0) once the caller has revalidated a
/// resolution read outside the writer. A strict-mode refusal still refuses
/// before anything is written.
///
/// # Errors
/// Refuses on a strict-mode match; propagates ingest failures.
pub fn ingest_episode_resolved(
    store: &mut Store,
    episode: &Episode,
    timestamp: &str,
    base_ns: &str,
    resolved: resolution::EpisodeResolution,
) -> Result<IngestResult> {
    if let Some(msg) = resolved.refusal {
        return Err(crate::error::Error::InvalidValue(msg));
    }
    let (tx_id, count, outcome, label_hints) =
        ingest_episode_outcome_with_hints(store, episode, timestamp, base_ns)?;
    let mut hints = resolved.hints;
    hints.extend(label_hints);
    Ok(IngestResult {
        tx_id,
        count,
        outcome,
        resolution_hints: hints,
        resolution_contentions: resolved.contentions,
    })
}
