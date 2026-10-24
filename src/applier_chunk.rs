//! The digest chain shared by a chunked transaction and a chunked as-of
//! share: numbered parts, each naming the hash of every part before it, and a
//! commit that must close the chain exactly.

use serde_json::Value as Json;
use sha2::{Digest, Sha256};

pub(super) struct Chunk {
    pub(super) source_tx: i64,
    parts: usize,
    /// Parts accepted so far; the next part's index must equal it.
    seen: usize,
    records: usize,
    /// Records received across accepted parts, kept or not.
    received: usize,
    digest: String,
    hasher: Sha256,
    /// The records, when the caller keeps them until the commit. A share
    /// streams its parts into the store instead, so it keeps nothing here.
    pub(super) buffered: Vec<Json>,
    keep: bool,
}

impl Chunk {
    /// The chain a begin frame announces. `keep` buffers each part's records.
    pub(super) fn new(source_tx: i64, parts: u64, records: u64, digest: &str, keep: bool) -> Self {
        Self {
            source_tx,
            parts: usize::try_from(parts).unwrap_or(usize::MAX),
            records: usize::try_from(records).unwrap_or(usize::MAX),
            seen: 0,
            received: 0,
            digest: digest.to_string(),
            hasher: Sha256::new(),
            buffered: Vec::new(),
            keep,
        }
    }

    /// Accept the next part if it continues the chain: same source
    /// transaction, the next index, and a `digest_before` equal to the hash
    /// of every part so far. `false` breaks the chunk.
    pub(super) fn accept(&mut self, part: &Json, data: &str) -> bool {
        let expected_before = hex::encode(self.hasher.clone().finalize());
        let in_order = part["source_tx"].as_i64() == Some(self.source_tx)
            && part["index"].as_u64() == u64::try_from(self.seen).ok()
            && part["digest_before"].as_str() == Some(expected_before.as_str());
        if !in_order {
            return false;
        }
        // The digest covers the part EXACTLY as sent, so it is hashed as text.
        self.hasher.update(data.as_bytes());
        self.seen += 1;
        let records = part["records"].as_array().map_or(&[][..], Vec::as_slice);
        self.received += records.len();
        if self.keep {
            self.buffered.extend(records.iter().cloned());
        }
        true
    }

    /// Whether the commit closes this chunk exactly: every part seen, the
    /// chained digest and record count as announced.
    pub(super) fn complete(&self, commit: &Json) -> bool {
        let digest = hex::encode(self.hasher.clone().finalize());
        commit["source_tx"].as_i64() == Some(self.source_tx)
            && commit["digest"].as_str() == Some(self.digest.as_str())
            && digest == self.digest
            && self.seen == self.parts
            && self.received == self.records
    }
}
