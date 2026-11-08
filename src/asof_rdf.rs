//! Offline, bounded RDF projection of one complete as-of share.
//!
//! The caller authenticates/acquires the transport and stages emitted chunks.
//! Chunks are NOT an accepted dataset until `convert` returns successfully.
//! No network request, production-store opening, or validation happens here.
//! The supplied store must be a new private disk-backed scratch replica; its
//! SQLite temporary directory and filesystem budget belong to the caller.

use oxrdf::{NamedNode, NamedOrBlankNode, Term, Triple};
use serde_json::Value as Json;
use sha2::{Digest, Sha256};

use crate::applier::{Applier, Outcome, Source, record_to_datum};
use crate::{Error, Result, Store, Value};

#[cfg(unix)]
#[path = "asof_rdf_files.rs"]
pub mod files;

/// Exact acquisition binding, obtained outside the conversion operation.
pub struct Binding {
    /// Admitted source lineage and epoch.
    pub source: Source,
    /// One positive source transaction, shared by every chunk.
    pub watermark_tx: i64,
    /// Exactly one named graph, without implicit ROOT or context expansion.
    pub graph: String,
    /// Hash of the complete SHACL profile at `watermark_tx`, not today's profile.
    pub shapes_hash: String,
}

/// Aggregate input limits; the existing compact RDF limit still applies to
/// EVERY output chunk. A large dataset is never passed to that serializer.
pub struct Limits {
    /// Maximum JSON data bytes of one transport frame (at most 1 MiB).
    pub frame_bytes: usize,
    /// Maximum aggregate JSON data bytes across the complete share.
    pub total_bytes: usize,
    /// Maximum number of transport frames including begin and commit.
    pub frames: usize,
}

/// Completion evidence. This is coherence/content evidence, not an assertion
/// that the transport was authenticated or that SHACL validation passed.
#[derive(Debug, serde::Serialize)]
pub struct Receipt {
    /// Verified source watermark, repeated in the acquisition receipt.
    pub source_tx: i64,
    /// Number of input frames consumed through EOF.
    pub frames: usize,
    /// Number of independently encoded compact RDF chunks.
    pub chunks: usize,
    /// Aggregate JSON data bytes consumed.
    pub input_bytes: usize,
    /// Aggregate compact RDF bytes emitted.
    pub output_bytes: usize,
    /// SHA-256 over successive big-endian u64 lengths and exact chunk bytes.
    pub output_sha256: String,
}

fn invalid(message: &str) -> Error {
    Error::InvalidValue(message.into())
}

fn check_manifest(frame: &Json, binding: &Binding) -> Result<()> {
    let manifest: crate::replica_manifest::Manifest =
        serde_json::from_value(frame["manifest"].clone())
            .map_err(|_| invalid("as-of RDF requires a complete typed manifest"))?;
    if manifest.source.store_id != binding.source.store_id
        || manifest.source.log_epoch != binding.source.log_epoch
        || manifest.watermark_tx != binding.watermark_tx
        || manifest.scope != [binding.graph.clone()]
        || !manifest.context_graphs.is_empty()
        || manifest.shapes_hash != binding.shapes_hash
        || manifest.completeness.graphs.len() != 1
        || !manifest.completeness.graphs.contains_key(&binding.graph)
    {
        return Err(invalid(
            "as-of RDF manifest differs from the admitted cut/profile/scope",
        ));
    }
    Ok(())
}

fn triples(store: &Store, frame: &Json) -> Result<Vec<Triple>> {
    let records = frame["records"].as_array().map_or(&[][..], Vec::as_slice);
    let mut output = Vec::with_capacity(records.len());
    for record in records {
        let datum = record_to_datum(store, record, "")?
            .map_err(|_| invalid("as-of RDF record cannot be reproduced exactly"))?;
        // Match current_facts_in_graph's RDF projection at the frozen cut:
        // closed valid-time claims are retained by bootstrap but not RDF export.
        if datum.valid_to.is_some() {
            continue;
        }
        let subject = match crate::rdf::value_to_term(store, &Value::Ref(datum.entity))? {
            Term::NamedNode(node) => NamedOrBlankNode::NamedNode(node),
            Term::BlankNode(node) => NamedOrBlankNode::BlankNode(node),
            Term::Literal(_) => return Err(invalid("unsupported as-of RDF subject")),
            #[cfg(feature = "shacl")]
            Term::Triple(_) => return Err(invalid("unsupported as-of RDF subject")),
        };
        output.push(Triple {
            subject,
            predicate: NamedNode::new(store.resolve(datum.attribute)?)
                .map_err(|_| invalid("invalid as-of RDF predicate"))?,
            object: crate::rdf::value_to_term(store, &datum.value)?,
        });
    }
    Ok(output)
}

/// Convert exactly one share using the shared applier's completeness checks
/// and record decoder. Dictionary IDs are local to each emitted Turtle chunk.
/// Discover validation targets over their UNION only after successful EOF.
///
/// The callback must stage each chunk privately and discard acceptance on ANY
/// error, including a callback error. It must not validate chunks independently:
/// a target declaration, inverse edge, or required value may arrive much later.
/// The caller owns wall/disk/RSS limits and may make the iterator/callback fail.
///
/// # Errors
/// Refuses an occupied scratch replica, wrong binding, bad/incomplete/multiple
/// cuts, unsupported RDF values, exceeded input limits or failed chunk writes.
pub fn convert<I, F>(
    store: &mut Store,
    binding: &Binding,
    limits: &Limits,
    frames: I,
    emit: F,
) -> Result<Receipt>
where
    I: IntoIterator<Item = Result<(String, String)>>,
    F: FnMut(usize, &[u8]) -> Result<()>,
{
    if !store.conn.is_autocommit() {
        return Err(invalid(
            "as-of RDF scratch store already has a transaction open",
        ));
    }
    let result = convert_inner(store, binding, limits, frames, emit);
    if result.is_err() && !store.conn.is_autocommit() {
        // The shared applier owns rollback of an interrupted bootstrap. Reopen
        // it to discard a savepoint left by a failed iterator or output sink.
        // A completed private replica is retained as evidence, never accepted
        // merely because its commit preceded a later output/EOF failure.
        Applier::open(store)?;
    }
    result
}

fn convert_inner<I, F>(
    store: &mut Store,
    binding: &Binding,
    limits: &Limits,
    frames: I,
    mut emit: F,
) -> Result<Receipt>
where
    I: IntoIterator<Item = Result<(String, String)>>,
    F: FnMut(usize, &[u8]) -> Result<()>,
{
    if binding.watermark_tx <= 0
        || binding.graph.is_empty()
        || binding.shapes_hash.is_empty()
        || limits.frame_bytes == 0
        || limits.frame_bytes > crate::feed_parts::MAX_FRAME_BYTES
        || limits.total_bytes == 0
        || limits.frames == 0
    {
        return Err(invalid("invalid as-of RDF binding or limits"));
    }
    if Applier::new_replica_refusal(store)?.is_some() {
        return Err(invalid("as-of RDF requires an empty scratch replica"));
    }
    store.set_term_cache_limit(8192);
    let mut applier = Applier::open(store)?;
    let mut started = false;
    let mut committed = false;
    let mut receipt = Receipt {
        source_tx: binding.watermark_tx,
        frames: 0,
        chunks: 0,
        input_bytes: 0,
        output_bytes: 0,
        output_sha256: String::new(),
    };
    let mut digest = Sha256::new();
    for frame in frames {
        let (event, data) = frame?;
        receipt.frames += 1;
        receipt.input_bytes = receipt
            .input_bytes
            .checked_add(data.len())
            .ok_or_else(|| invalid("as-of RDF byte count overflow"))?;
        if committed
            || receipt.frames > limits.frames
            || data.len() > limits.frame_bytes
            || receipt.input_bytes > limits.total_bytes
        {
            return Err(invalid("as-of RDF sequence or input budget exceeded"));
        }
        let json: Json =
            serde_json::from_str(&data).map_err(|_| invalid("as-of RDF frame is not JSON"))?;
        match event.as_str() {
            "quipu.asof" | "quipu.asof.begin" if !started => {
                check_manifest(&json, binding)?;
                started = true;
            }
            "quipu.asof.part" | "quipu.asof.commit" if started => {}
            _ => return Err(invalid("as-of RDF requires one uninterrupted share")),
        }
        let outcome = applier.apply_frame(store, &event, &data, 0)?;
        match outcome {
            Outcome::Buffered { source_tx } if source_tx == binding.watermark_tx => {}
            Outcome::Bootstrapped { source_tx } if source_tx == binding.watermark_tx => {
                committed = true;
            }
            _ => return Err(invalid("shared applier did not accept the as-of RDF frame")),
        }
        if matches!(event.as_str(), "quipu.asof" | "quipu.asof.part") {
            let compact = crate::rdf_compact::serialize(
                &triples(store, &json)?,
                crate::rdf_compact::COMPACT_RDF_LIMIT,
            )?;
            emit(receipt.chunks, &compact.bytes)?;
            receipt.chunks += 1;
            receipt.output_bytes = receipt
                .output_bytes
                .checked_add(compact.bytes.len())
                .ok_or_else(|| invalid("as-of RDF output byte count overflow"))?;
            // Length-delimit chunks: dictionaries may restart at zero.
            digest.update((compact.bytes.len() as u64).to_be_bytes());
            digest.update(&compact.bytes);
        }
    }
    if !committed
        || applier
            .progress()
            .is_none_or(|p| p.source != binding.source || p.source_tx != binding.watermark_tx)
    {
        return Err(invalid("as-of RDF share ended without a verified commit"));
    }
    receipt.output_sha256 = format!("{:x}", digest.finalize());
    Ok(receipt)
}

#[cfg(test)]
#[path = "asof_rdf_tests.rs"]
mod tests;
