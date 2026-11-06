//! Bind a declared write base to the writer's captured store coordinates.
//!
//! Pure preparation only: this module acquires no reservation, reads no facts,
//! validates no slice hash and grants no write authority. The admission writer
//! must supply authentic coordinates and subsequently verify the touched slice.

use crate::resolver_contract::SourceBinding;
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

#[cfg(test)]
#[path = "write_base_tests.rs"]
mod tests;
