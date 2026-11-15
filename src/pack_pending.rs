//! Explicit evidence-only materialization. No facts, configuration or grants.
use rusqlite::OptionalExtension;

use super::{PackOptions, carriers};
use crate::share_scrub::ShareDestination;
use crate::{Error, Result, Store};

/// Exact pending manifests retained by [`materialize_pending_carriers`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingCarrierReport {
    /// Verified pendant identity; this is not an origin or admission receipt.
    pub content_hash: String,
    /// Complete manifests newly retained in the receiver.
    pub retained: usize,
    /// Complete manifests already present byte for byte.
    pub unchanged: usize,
}

/// Verify a pendant and atomically retain only its explicit pending evidence.
///
/// The caller supplies an already-open receiver and its transfer destination.
/// No graph remapping is available: signed logical scope and proof strings are
/// preserved verbatim. No facts, shapes, queries, observable definitions, source
/// permissions or authentication records are loaded. Native admission remains
/// a separate, locally governed operation. Ordinary unpack still refuses these
/// pendants. Conflicting complete manifests refuse the entire batch.
pub fn materialize_pending_carriers(
    pack_path: &str,
    receiver: &mut Store,
    destination: ShareDestination,
) -> Result<PendingCarrierReport> {
    // One pinned source connection supplies the format, hash and retained bytes;
    // reopening a pathname between verification and copying could mix artifacts.
    let source = Store::open_immutable(pack_path)?;
    let manifest = super::read_manifest_on(&source.conn)?;
    crate::pack_restore::require_format(&manifest, crate::pack_restore::Verb::Unpack)?;
    let selected = carriers::read(&source.conn)?;
    if selected.is_empty() {
        return Err(Error::InvalidValue(
            "pack: no explicit pending fluent carriers to materialize".into(),
        ));
    }
    let shapes = source
        .list_shapes()?
        .into_iter()
        .map(|(name, _, _)| name)
        .collect::<Vec<_>>();
    let queries = source
        .query_list()?
        .into_iter()
        .map(|query| query.name)
        .collect::<Vec<_>>();
    let mut canonical =
        super::canonical_content(&source, &manifest.source_graph, &shapes, &queries)?;
    carriers::extend_canonical(&mut canonical, &selected);
    let recomputed = super::content_hash(&canonical);
    if recomputed != manifest.content_hash {
        return Err(Error::CannotVerify(format!(
            "pack: HASH MISMATCH: manifest {}, recomputed {recomputed}",
            manifest.content_hash
        )));
    }
    receiver.ensure_ownership_writable()?;
    crate::namespace_publication::receive_pack(receiver, &manifest, &destination, &canonical)?;
    carriers::enforce_destination(
        receiver,
        &selected,
        &PackOptions {
            destination,
            ..Default::default()
        },
    )?;

    // A SQLite transaction protects all selected manifests, including when a
    // later row conflicts or a receiver-side constraint rejects an insertion.
    let tx = receiver.conn.unchecked_transaction()?;
    let mut report = PendingCarrierReport {
        content_hash: manifest.content_hash,
        retained: 0,
        unchanged: 0,
    };
    for (id, text) in selected {
        let manifest: crate::share::ShareManifest = serde_json::from_str(&text)
            .map_err(|_| Error::CannotVerify("invalid selected manifest".into()))?;
        // Bound the existing value before retrieval; equality to the incoming
        // length is necessary for exact byte equality, never a proof by itself.
        let previous: Option<Option<String>> = tx
            .query_row(
                "SELECT CASE WHEN length(CAST(manifest AS BLOB))=?2 THEN manifest ELSE NULL END FROM share_fluent_carriers WHERE share_id=?1",
                rusqlite::params![id, text.len() as i64],
                |row| row.get(0),
            )
            .optional()?;
        match previous {
            Some(Some(previous)) if previous == text => report.unchanged += 1,
            Some(_) => {
                return Err(Error::InvalidValue(
                    "pack: conflicting retained complete carrier manifest".into(),
                ));
            }
            None => {
                tx.execute(
                    "INSERT INTO share_fluent_carriers(share_id,manifest) VALUES(?1,?2)",
                    rusqlite::params![id, text],
                )?;
                report.retained += 1;
            }
        }
        for raw in &manifest.observable_configs {
            crate::fluent::pending_config::Pending::decode(raw.as_bytes())?.retain(receiver)?;
        }
    }
    receiver.ensure_ownership_writable()?;
    tx.commit()?;
    Ok(report)
}
