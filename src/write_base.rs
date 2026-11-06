//! Bind a declared write base to the writer's captured store coordinates.
//!
//! Pure preparation only: this module acquires no reservation, reads no facts,
//! validates no slice hash and grants no write authority. The admission writer
//! must supply authentic coordinates and subsequently verify the touched slice.

use std::collections::BTreeSet;

use crate::resolver_contract::{Slot, SourceBinding};
use crate::{Error, Result};

/// Explicit graph policy; omission never silently chooses compatibility mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyBasePolicy {
    RequireDeclared,
    UseCurrentAndFlag,
}

/// Coordinates captured under the normal single-writer reservation.
///
/// `oldest_complete_tx` is the oldest base with proved complete retained history,
/// not the smallest surviving fact or transaction id. These fields are caller
/// evidence, not a capability or a proof that a reservation is actually held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterSnapshot {
    pub store_id: String,
    pub log_epoch: String,
    pub graph: String,
    pub oldest_complete_tx: u64,
    pub head_tx: u64,
}

/// One resolved base. A declared source still needs scope/hash verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWriteBase<'a> {
    pub base_tx: u64,
    pub legacy_unbased: bool,
    pub source: Option<&'a SourceBinding>,
}

/// Reject a base from another store, epoch or graph, future/expired history,
/// malformed writer coordinates, or an unbased write under strict policy.
///
/// This does not reconstruct an incoming value set or substitute the current
/// slot for missing historical evidence. No server write path calls it yet.
pub fn resolve_write_base<'a>(
    declared: Option<&'a SourceBinding>,
    current: &WriterSnapshot,
    legacy: LegacyBasePolicy,
) -> Result<ResolvedWriteBase<'a>> {
    if current.store_id.is_empty()
        || current.log_epoch.is_empty()
        || current.graph.is_empty()
        || current.oldest_complete_tx > current.head_tx
    {
        return Err(Error::InvalidValue(
            "write base: invalid writer coordinates".into(),
        ));
    }
    let Some(source) = declared else {
        return match legacy {
            LegacyBasePolicy::RequireDeclared => Err(Error::InvalidValue(
                "write base: declared base required".into(),
            )),
            LegacyBasePolicy::UseCurrentAndFlag => Ok(ResolvedWriteBase {
                base_tx: current.head_tx,
                legacy_unbased: true,
                source: None,
            }),
        };
    };
    if source.store_id != current.store_id
        || source.log_epoch != current.log_epoch
        || source.graph != current.graph
    {
        return Err(Error::InvalidValue(
            "write base: store, epoch or graph mismatch".into(),
        ));
    }
    if source.base_tx < current.oldest_complete_tx || source.base_tx > current.head_tx {
        return Err(Error::InvalidValue(
            "write base: history unavailable or base is in the future".into(),
        ));
    }
    Ok(ResolvedWriteBase {
        base_tx: source.base_tx,
        legacy_unbased: false,
        source: Some(source),
    })
}

/// Resolve a base only for an explicitly bounded set of request slots.
///
/// The request adapter must canonicalize terms before calling this function.
/// This checks exact scope equality, not RDF syntax, slice hashes, authorization,
/// history completeness or reservation ownership. Empty scope is a no-op scope.
/// A legacy request still supplies its touched slots; it never implies a scan.
pub fn prepare_scoped_write_base<'a>(
    declared: Option<&'a SourceBinding>,
    current: &WriterSnapshot,
    legacy: LegacyBasePolicy,
    touched: &[Slot],
    max_slots: usize,
) -> Result<ResolvedWriteBase<'a>> {
    if max_slots == 0 || touched.len() > max_slots {
        return Err(Error::InvalidValue(
            "write base: invalid or exceeded slot budget".into(),
        ));
    }
    let keys = |slots: &[Slot]| -> Result<BTreeSet<(String, String)>> {
        let mut keys = BTreeSet::new();
        for slot in slots {
            if slot.subject.is_empty()
                || slot.predicate.is_empty()
                || !keys.insert((slot.subject.clone(), slot.predicate.clone()))
            {
                return Err(Error::InvalidValue(
                    "write base: empty or duplicate slot".into(),
                ));
            }
        }
        Ok(keys)
    };
    let requested = keys(touched)?;
    if let Some(source) = declared {
        let scope = source.scope.as_deref().ok_or_else(|| {
            Error::InvalidValue("write base: explicit touched scope required".into())
        })?;
        if scope.len() > max_slots || keys(scope)? != requested {
            return Err(Error::InvalidValue(
                "write base: declared/request scope mismatch".into(),
            ));
        }
    }
    resolve_write_base(declared, current, legacy)
}

/// Captured coordinates for bounded historical and current slot reconstruction.
///
/// Private fields prevent callers from changing scope or one time cut after
/// preparation. This is read planning, not reservation ownership or a Store
/// query. In particular, neither cut grants authentication or write authority.
pub struct PreparedSlotReads<'a> {
    base: ResolvedWriteBase<'a>,
    touched: Vec<Slot>,
    base_cut: crate::store::AsOf,
    current_cut: crate::store::AsOf,
}

impl<'a> PreparedSlotReads<'a> {
    /// Coordinate binding, still subject to authentic slice/hash verification.
    pub fn base(&self) -> &ResolvedWriteBase<'a> {
        &self.base
    }

    /// Exact bounded request slots, copied to avoid later request mutation.
    pub fn touched(&self) -> &[Slot] {
        &self.touched
    }

    /// Explicit transaction and valid-time coordinates for the declared base.
    pub fn base_cut(&self) -> &crate::store::AsOf {
        &self.base_cut
    }

    /// Reserved head at the same valid time, never a fresh wall-clock read.
    pub fn current_cut(&self) -> &crate::store::AsOf {
        &self.current_cut
    }
}

/// Prepare independent transaction cuts at one explicit canonical valid time.
///
/// Store adapters must read only `touched()` at these cuts, preserving the
/// interval known at each transaction. Passing the cuts to a full-graph query
/// and filtering afterward does not satisfy this bounded-read contract.
pub fn prepare_slot_reads<'a>(
    declared: Option<&'a SourceBinding>,
    current: &WriterSnapshot,
    legacy: LegacyBasePolicy,
    touched: &[Slot],
    max_slots: usize,
    valid_at: &str,
) -> Result<PreparedSlotReads<'a>> {
    if crate::time::normalize_rfc3339_utc(valid_at).as_deref() != Some(valid_at) {
        return Err(Error::InvalidValue(
            "write base: explicit canonical UTC valid time required".into(),
        ));
    }
    let base = prepare_scoped_write_base(declared, current, legacy, touched, max_slots)?;
    let tx = |value: u64| {
        i64::try_from(value)
            .map_err(|_| Error::InvalidValue("write base: transaction exceeds Store range".into()))
    };
    let base_cut = crate::store::AsOf {
        tx: Some(tx(base.base_tx)?),
        valid_at: Some(valid_at.to_owned()),
    };
    let current_cut = crate::store::AsOf {
        tx: Some(tx(current.head_tx)?),
        valid_at: Some(valid_at.to_owned()),
    };
    Ok(PreparedSlotReads {
        base,
        touched: touched.to_vec(),
        base_cut,
        current_cut,
    })
}

#[cfg(test)]
#[path = "write_base_tests.rs"]
mod tests;
