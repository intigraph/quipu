//! Scoped prepared cuts -> bounded physical slot reads, without writer admission.
use super::Store;
use super::slot_reads::SlotReadLimits;
use crate::resolver_contract::Slot;
use crate::write_base::PreparedSlotReads;
use crate::{Error, Fact, Result};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy)]
pub(crate) struct BatchReadLimits {
    pub claims_per_read: usize,
    pub bytes_per_read: usize,
    /// Shared by all physical reads at BOTH cuts, before visible-value dedup.
    pub total_claims: usize,
    pub total_bytes: usize,
}

pub(crate) struct SlotViews {
    pub slot: Slot,
    pub base: Vec<Fact>,
    pub current: Vec<Fact>,
    pub current_claims_digest: [u8; 32],
}

pub(crate) struct BatchViews {
    pub slots: Vec<SlotViews>,
    pub physical_claims: usize,
    pub encoded_bytes: usize,
}

fn refused() -> Error {
    Error::InvalidValue("write base: prepared slot batch unavailable".into())
}

impl Store {
    /// Caller must hold the existing writer reservation and prove retained
    /// completeness/hash/authentication independently. Checks are read evidence,
    /// not a lock or a capability. Partial results never escape an error.
    pub(crate) fn read_prepared_slot_batch(
        &self,
        prepared: &PreparedSlotReads<'_>,
        limits: BatchReadLimits,
    ) -> Result<BatchViews> {
        if prepared.touched().len() > 256
            || !(1..=4096).contains(&limits.claims_per_read)
            || !(1..=4 * 1024 * 1024).contains(&limits.bytes_per_read)
            || !(1..=65536).contains(&limits.total_claims)
            || !(1..=16 * 1024 * 1024).contains(&limits.total_bytes)
        {
            return Err(refused());
        }
        let snapshot = prepared.writer_snapshot();
        let matches = || -> Result<bool> {
            Ok(self.store_id()? == snapshot.store_id
                && self.log_epoch()? == snapshot.log_epoch
                && u64::try_from(self.latest_tx_id()?).ok() == Some(snapshot.head_tx))
        };
        if !matches()? {
            return Err(refused());
        }
        let graph = if snapshot.graph == crate::schema::ROOT_GRAPH_IRI {
            crate::schema::ROOT_GRAPH
        } else {
            self.lookup(&snapshot.graph)?.ok_or_else(refused)?
        };
        let mut out = BatchViews {
            slots: Vec::new(),
            physical_claims: 0,
            encoded_bytes: 0,
        };
        for slot in prepared.touched() {
            // Stable named resources only. Never intern terms while preparing a read.
            let subject = slot.subject.parse::<oxrdf::Term>().map_err(|_| refused())?;
            let oxrdf::Term::NamedNode(subject) = subject else {
                return Err(refused());
            };
            if subject.to_string() != slot.subject
                || oxrdf::NamedNode::new(slot.predicate.clone()).is_err()
            {
                return Err(refused());
            }
            let ids = (
                self.lookup(subject.as_str())?,
                self.lookup(&slot.predicate)?,
            );
            let mut sides = Vec::new();
            for cut in [prepared.base_cut(), prepared.current_cut()] {
                let facts = if let (Some(entity), Some(attribute)) = ids {
                    let read = self.bounded_slot_read_at(
                        graph,
                        entity,
                        attribute,
                        cut,
                        SlotReadLimits {
                            max_claims: limits
                                .claims_per_read
                                .min(limits.total_claims - out.physical_claims),
                            max_bytes: limits
                                .bytes_per_read
                                .min(limits.total_bytes - out.encoded_bytes),
                        },
                    )?;
                    out.physical_claims += read.claims;
                    out.encoded_bytes += read.bytes;
                    (read.facts, read.claims_digest)
                } else {
                    (Vec::new(), Sha256::digest(b"quipu-slot-claims/v1\0").into())
                };
                sides.push(facts);
            }
            let current = sides.pop().ok_or_else(refused)?;
            let base = sides.pop().ok_or_else(refused)?;
            out.slots.push(SlotViews {
                slot: slot.clone(),
                base: base.0,
                current: current.0,
                current_claims_digest: current.1,
            });
        }
        if !matches()? {
            return Err(refused());
        }
        Ok(out)
    }
}

#[cfg(test)]
#[path = "slot_read_batch_tests.rs"]
mod tests;
