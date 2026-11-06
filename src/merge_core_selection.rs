//! Source-stage selection for spec §6. No caller persists partial merges yet.
//!
//! Selection does not create pending rows, resume identities or transactions.
//! Adapters must retain held input at base and bind pending rows to the actual
//! receive transaction. The existing atomic entry point remains unchanged.
use std::collections::{BTreeMap, BTreeSet};

use super::{Decision, MergePlan, SlotInput, plan_merge};
use crate::error::{Error, Result};

/// Which otherwise valid slot plans may be applied together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Mode {
    Atomic,
    PartialEntity,
    PartialSlot,
}

impl Default for Mode {
    fn default() -> Self {
        Self::Atomic
    }
}

/// Select a closed set of slots without splitting any supplied merge unit.
///
/// Entity identity and units are explicit inputs, not inferred from `sh:group`.
/// Unknown or empty unit declarations are refused before returning operations.
/// Closure is repeated: a unit crossing entities may hold another entity,
/// which can in turn hold another unit. Held plans expose no write operations.
pub(crate) fn select<S: Ord + Clone, V: Ord + Clone, E: Ord>(
    inputs: &BTreeMap<S, SlotInput<V>>,
    decisions: &BTreeMap<S, Decision<V>>,
    entities: &BTreeMap<S, E>,
    units: &[BTreeSet<S>],
    mode: Mode,
    td: &str,
) -> Result<MergePlan<S, V>> {
    if !entities.keys().eq(inputs.keys())
        || units
            .iter()
            .any(|unit| unit.is_empty() || unit.iter().any(|slot| !inputs.contains_key(slot)))
    {
        return Err(Error::InvalidValue(
            "merge selection: exact entity mapping and nonempty known-slot units required".into(),
        ));
    }
    let mut plan = plan_merge(inputs, decisions, td)?;
    if mode == Mode::Atomic {
        return Ok(plan);
    }
    let mut held: BTreeSet<S> = plan
        .slots
        .iter()
        .filter(|(_, slot)| slot.held)
        .map(|(key, _)| key.clone())
        .collect();
    loop {
        let before = held.len();
        if mode == Mode::PartialEntity {
            let blocked: BTreeSet<_> = held.iter().map(|slot| &entities[slot]).collect();
            let extra: Vec<_> = entities
                .iter()
                .filter(|(_, entity)| blocked.contains(entity))
                .map(|(slot, _)| slot.clone())
                .collect();
            held.extend(extra);
        }
        for unit in units {
            if unit.iter().any(|slot| held.contains(slot)) {
                held.extend(unit.iter().cloned());
            }
        }
        if held.len() == before {
            break;
        }
    }
    // plan_merge clears the aggregate vectors in atomic mode, but retains the
    // individual plans. Rebuild only after selecting the complete held closure.
    plan.receive.clear();
    plan.decide.clear();
    plan.applied = 0;
    plan.held = held.len();
    for (key, slot) in &mut plan.slots {
        if held.contains(key) {
            slot.held = true;
            slot.receive.clear();
            slot.decide.clear();
            continue;
        }
        plan.applied += usize::from(!slot.receive.is_empty() || !slot.decide.is_empty());
        plan.receive
            .extend(slot.receive.iter().cloned().map(|op| (key.clone(), op)));
        plan.decide
            .extend(slot.decide.iter().cloned().map(|op| (key.clone(), op)));
    }
    Ok(plan)
}

#[cfg(test)]
#[path = "merge_core_selection_tests.rs"]
mod tests;
