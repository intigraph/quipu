//! Source-stage observations through the real merge core; no writer hook.
//!
//! The future producer must obtain the base and shape bound from its admitted
//! snapshot/profile. These evidence labels describe that input; they are not
//! authentication or a substitute for validating the producer's binding.

use std::collections::BTreeMap;

use serde::Serialize;

use super::{Card, Change, SlotInput, plan_slot};
use crate::{Error, Result};

const MAX_SLOTS: usize = 256;
const MAX_VALUES: usize = 4096;
const MAX_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SlotKey {
    pub graph: i64,
    pub entity: i64,
    pub predicate: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum BaseEvidence {
    Missing,
    Declared,
    RetainedSnapshot,
}

pub(crate) struct ObservationInput<'a> {
    pub slot: &'a SlotInput<String>,
    pub base: BaseEvidence,
    pub shape_bound_known: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub(crate) enum BoundEvidence {
    Unknown,
    Known { max_count: Option<usize> },
}

/// Value-free output. Entity identity, term bytes and times are not serialized.
#[derive(Debug, Serialize)]
pub(crate) struct SlotReport {
    pub graph_id: i64,
    pub predicate_id: i64,
    pub base_evidence: BaseEvidence,
    pub shape_bound: BoundEvidence,
    pub classification: Option<Change>,
    pub would_hold_slot: Option<bool>,
    pub unknown_reason: Option<&'static str>,
}

fn budget(slots: &BTreeMap<SlotKey, ObservationInput<'_>>, td: &str) -> Result<()> {
    let mut bytes = td.len();
    let mut values = 0_usize;
    if slots.len() > MAX_SLOTS || bytes > MAX_BYTES {
        return Err(Error::InvalidValue(
            "shadow census: batch budget exceeded".into(),
        ));
    }
    for input in slots.values() {
        let slot = input.slot;
        for value in slot
            .base
            .iter()
            .chain(slot.ours.keys())
            .chain(slot.theirs.keys())
            .chain(slot.theirs_closed.keys())
        {
            values = values.saturating_add(1);
            bytes = bytes.saturating_add(value.len());
            if values > MAX_VALUES || bytes > MAX_BYTES {
                return Err(Error::InvalidValue(
                    "shadow census: batch budget exceeded".into(),
                ));
            }
        }
        for time in slot
            .ours
            .values()
            .chain(slot.theirs.values())
            .chain(slot.theirs_closed.values())
        {
            bytes = bytes.saturating_add(time.len());
            if bytes > MAX_BYTES {
                return Err(Error::InvalidValue(
                    "shadow census: batch budget exceeded".into(),
                ));
            }
        }
    }
    Ok(())
}

/// Observe per-slot default decisions, never executing any planned operations.
/// A held slot does not assert what a future atomic batch would commit.
///
/// # Errors
/// Refuses the entire result when input budgets or native planning fail.
pub(crate) fn observe_batch(
    slots: &BTreeMap<SlotKey, ObservationInput<'_>>,
    td: &str,
) -> Result<Vec<SlotReport>> {
    budget(slots, td)?;
    slots
        .iter()
        .map(|(key, input)| {
            let unknown = if input.base == BaseEvidence::Missing {
                Some("read-base-unknown")
            } else if !input.shape_bound_known {
                Some("shape-bound-unknown")
            } else {
                None
            };
            let plan = if unknown.is_none() {
                Some(plan_slot(input.slot, None, td)?)
            } else {
                None
            };
            let bound = if input.shape_bound_known {
                let Card::Many(max_count) = input.slot.card;
                BoundEvidence::Known { max_count }
            } else {
                BoundEvidence::Unknown
            };
            Ok(SlotReport {
                graph_id: key.graph,
                predicate_id: key.predicate,
                base_evidence: input.base,
                shape_bound: bound,
                classification: plan.as_ref().map(|p| p.change),
                would_hold_slot: plan.as_ref().map(|p| p.held),
                unknown_reason: unknown,
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "census_tests.rs"]
mod tests;
