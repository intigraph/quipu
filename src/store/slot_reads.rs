//! Explicit, bounded touched-slot history reads for future writer admission.
//!
//! No endpoint calls this reader. The caller must already hold the normal
//! reservation and validate store identity, retained history and slice hashes.

use std::collections::HashSet;

use rusqlite::{params, types::ValueRef};

use super::{AsOf, Store};
use crate::{Error, Fact, Op, Result, Value};

#[derive(Clone, Copy)]
pub(crate) struct SlotReadLimits {
    /// Physical history rows, including duplicates and invisible assertions.
    pub max_claims: usize,
    /// Encoded value and interval bytes, before allocation or decoding.
    pub max_bytes: usize,
}

pub(super) struct SlotRead {
    pub facts: Vec<Fact>,
    pub claims: usize,
    pub bytes: usize,
}

fn refused() -> Error {
    Error::InvalidValue("write base: bounded slot history unavailable".into())
}

fn canonical(time: &str) -> bool {
    crate::time::normalize_rfc3339_utc(time).as_deref() == Some(time)
}

impl Store {
    /// Read ONE exact physical slot at explicit transaction and valid-time cuts.
    ///
    /// This source-stage reader supports canonical whole-second UTC intervals.
    /// Other stored timestamp spellings refuse instead of losing precision.
    /// It scans at most `max_claims + 1` indexed rows, not a whole graph, and
    /// refuses overflow even if excess history would be invisible at the cut.
    /// Caps bound work/data, not elapsed time, locking, or retained completeness.
    pub(crate) fn bounded_slot_facts_at(
        &self,
        graph: i64,
        entity: i64,
        attribute: i64,
        cut: &AsOf,
        limits: SlotReadLimits,
    ) -> Result<Vec<Fact>> {
        Ok(self
            .bounded_slot_read_at(graph, entity, attribute, cut, limits)?
            .facts)
    }

    pub(super) fn bounded_slot_read_at(
        &self,
        graph: i64,
        entity: i64,
        attribute: i64,
        cut: &AsOf,
        limits: SlotReadLimits,
    ) -> Result<SlotRead> {
        let (Some(tx), Some(valid_at)) = (cut.tx, cut.valid_at.as_deref()) else {
            return Err(refused());
        };
        if graph < 0
            || entity <= 0
            || attribute <= 0
            || tx < 0
            || !canonical(valid_at)
            || !(1..=4096).contains(&limits.max_claims)
            || !(1..=4 * 1024 * 1024).contains(&limits.max_bytes)
        {
            return Err(refused());
        }
        // Limit physical rows BEFORE temporal filtering: a small visible
        // result must not conceal an unbounded historical scan or sort.
        let mut statement = self.conn.prepare(
            "SELECT v, tx, valid_from, valid_to, op, retracted_tx \
             FROM facts INDEXED BY idx_geav \
             WHERE g = ?1 AND e = ?2 AND a = ?3 LIMIT ?4",
        )?;
        let row_limit = i64::try_from(limits.max_claims + 1).map_err(|_| refused())?;
        let mut rows = statement.query(params![graph, entity, attribute, row_limit])?;
        let mut count = 0;
        let mut bytes = 0_usize;
        let mut seen = HashSet::new();
        let mut facts = Vec::new();
        while let Some(row) = rows.next()? {
            count += 1;
            if count > limits.max_claims {
                return Err(refused());
            }
            let ValueRef::Blob(value) = row.get_ref(0)? else {
                return Err(refused());
            };
            let ValueRef::Text(start) = row.get_ref(2)? else {
                return Err(refused());
            };
            let end = match row.get_ref(3)? {
                ValueRef::Null => None,
                ValueRef::Text(end) => Some(end),
                _ => return Err(refused()),
            };
            bytes = bytes
                .saturating_add(value.len())
                .saturating_add(start.len())
                .saturating_add(end.map_or(0, <[u8]>::len));
            if bytes > limits.max_bytes {
                return Err(refused());
            }
            let start = std::str::from_utf8(start).map_err(|_| refused())?;
            let end = end
                .map(std::str::from_utf8)
                .transpose()
                .map_err(|_| refused())?;
            if !canonical(start) || end.is_some_and(|end| !canonical(end) || end < start) {
                return Err(refused());
            }
            let asserted_tx: i64 = row.get(1)?;
            let retracted_tx: Option<i64> = row.get(5)?;
            if asserted_tx < 0
                || retracted_tx.is_some_and(|closed| closed < asserted_tx || end.is_none())
            {
                return Err(refused());
            }
            if row.get::<_, i64>(4)? != 1 || asserted_tx > tx || start > valid_at {
                continue;
            }
            // A future retracted_tx certifies that this assertion's end was
            // open at the earlier tx. Originally finite intervals retain their
            // bound; empty received intervals never become live at/after tx.
            let effective_end = if retracted_tx.is_some_and(|closed| closed > tx) {
                None
            } else {
                end
            };
            if effective_end.is_some_and(|end| end <= valid_at) {
                continue;
            }
            let value = Value::from_bytes(value)?;
            if seen.insert(value.term_key()) {
                facts.push(Fact {
                    entity,
                    attribute,
                    value,
                    tx: asserted_tx,
                    valid_from: start.to_owned(),
                    valid_to: effective_end.map(str::to_owned),
                    op: Op::Assert,
                });
            }
        }
        Ok(SlotRead {
            facts,
            claims: count,
            bytes,
        })
    }
}

#[cfg(test)]
#[path = "slot_reads_tests.rs"]
mod tests;
