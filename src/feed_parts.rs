//! Oversized-transaction parts: the one chunking scheme for feed frames.
//!
//! The change stream splits a transaction too large for one frame into
//! numbered parts (local-sync-mode.md §6, F6), and the as-of share sends a cut
//! the same way. Both build their parts here, so a follower checks one digest
//! scheme whichever surface the parts came from.
//!
//! Parts are built one record at a time and handed out as each one closes, so
//! a caller can write them to disk as they are made: memory holds one part,
//! never the whole transaction or cut.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Transport frame ceiling. A frame's data must fit with room for the SSE
/// `id:`/`event:` lines around it.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// Whether a serialized frame body of `len` bytes fits the frame budget.
#[must_use]
pub fn fits_frame_len(len: usize) -> bool {
    len <= MAX_FRAME_BYTES.saturating_sub(64)
}

/// One record is larger than a frame can carry, so it cannot be split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordTooLarge;

/// The totals a `begin` frame and a `commit` barrier repeat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartsSummary {
    /// Number of parts.
    pub parts: usize,
    /// Number of records across every part.
    pub records: usize,
    /// Total bytes of every part's data.
    pub bytes: usize,
    /// SHA-256 over the concatenated data of every part, in order, exactly as
    /// sent.
    pub digest: String,
}

/// Builds the parts of one transaction (or one cut) record by record.
///
/// Each part is `{"source_tx", "index", "digest_before", "records"}`, where
/// `digest_before` is the running digest over the parts before it, so a
/// consumer can check the stream as it arrives without a part hashing itself.
pub struct PartBuilder {
    source_tx: i64,
    max_len: usize,
    hasher: Sha256,
    chunk: Vec<Value>,
    size: usize,
    index: usize,
    records: usize,
    bytes: usize,
}

impl PartBuilder {
    /// A builder for the parts of `source_tx`, at the transport frame budget.
    #[must_use]
    pub fn new(source_tx: i64) -> Self {
        Self::with_max_len(source_tx, MAX_FRAME_BYTES.saturating_sub(64))
    }

    /// A builder whose parts are at most `max_len` bytes. Tests use a small
    /// budget to exercise splitting without megabyte fixtures.
    #[must_use]
    pub fn with_max_len(source_tx: i64, max_len: usize) -> Self {
        let mut builder = Self {
            source_tx,
            max_len,
            hasher: Sha256::new(),
            chunk: Vec::new(),
            size: 0,
            index: 0,
            records: 0,
            bytes: 0,
        };
        builder.size = builder.empty_len();
        builder
    }

    fn part_data(&self, chunk: &[Value], before: &str) -> String {
        json!({"source_tx": self.source_tx, "index": self.index, "digest_before": before, "records": chunk})
            .to_string()
    }

    /// The empty part, sized with a placeholder digest of the final length,
    /// so the real part can never be larger than the one that was measured.
    fn empty_len(&self) -> usize {
        self.part_data(&[], &"0".repeat(64)).len()
    }

    /// Add one record. Returns the part this record closed, if the record did
    /// not fit beside the records already waiting.
    ///
    /// # Errors
    ///
    /// [`RecordTooLarge`] when the record alone exceeds a part.
    pub fn push(&mut self, record: Value) -> Result<Option<String>, RecordTooLarge> {
        // Sizes add exactly: an array serializes as `[`, the items joined by
        // `,`, and `]`, so a part is its empty frame plus each record plus a
        // comma.
        let text = record.to_string().len();
        let mut closed = None;
        if !self.chunk.is_empty() && self.size + text + 1 > self.max_len {
            closed = Some(self.close());
        }
        let len = text + usize::from(!self.chunk.is_empty());
        if self.size + len > self.max_len {
            return Err(RecordTooLarge);
        }
        self.size += len;
        self.records += 1;
        self.chunk.push(record);
        Ok(closed)
    }

    fn close(&mut self) -> String {
        let before = hex::encode(self.hasher.clone().finalize());
        let chunk = std::mem::take(&mut self.chunk);
        let data = self.part_data(&chunk, &before);
        self.hasher.update(data.as_bytes());
        self.bytes += data.len();
        self.index += 1;
        self.size = self.empty_len();
        data
    }

    /// Close the last part and return it with the totals.
    #[must_use]
    pub fn finish(mut self) -> (Option<String>, PartsSummary) {
        let last = (!self.chunk.is_empty()).then(|| self.close());
        let summary = PartsSummary {
            parts: self.index,
            records: self.records,
            bytes: self.bytes,
            digest: hex::encode(self.hasher.finalize()),
        };
        (last, summary)
    }
}
