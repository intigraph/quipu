//! Time-free pendant projection of the common merge write core.
//!
//! Pendant history belongs to Git. The empty clock below is internal to the
//! projection: no valid time, receipt or transaction is persisted in a pack.
//! Held slots retain base, while the rest of the pack can still be inspected.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::Result;
use crate::merge_core::{Card, Decision, FactOp, SlotInput, plan_slot};
use crate::share_merge::{DecisionRecord, Graph, by_slot, parse_graph};

fn input(
    base: &BTreeSet<String>,
    ours: &BTreeSet<String>,
    theirs: &BTreeSet<String>,
    bound: Option<usize>,
) -> SlotInput<String> {
    let untimed =
        |values: &BTreeSet<String>| values.iter().map(|v| (v.clone(), String::new())).collect();
    SlotInput {
        base: base.clone(),
        ours: untimed(ours),
        theirs: untimed(theirs),
        theirs_closed: BTreeMap::new(),
        card: Card::Many(bound),
    }
}

fn project(
    input: &SlotInput<String>,
    decision: Option<&Decision<String>>,
) -> Result<(BTreeSet<String>, bool)> {
    let plan = plan_slot(input, decision, "")?;
    if plan.held {
        return Ok((input.base.clone(), true));
    }
    let mut values: BTreeSet<_> = input.ours.keys().cloned().collect();
    for op in plan.receive.iter().chain(&plan.decide) {
        match op {
            FactOp::Assert { value, .. } => {
                values.insert(value.clone());
            }
            FactOp::Close { value, .. } => {
                values.remove(value);
            }
            FactOp::Receipt { .. } => {}
        }
    }
    Ok((values, false))
}

/// Prepare the pendant graph through the same per-slot plan as store merges.
pub(super) fn merge(
    base: &Graph,
    ours: &Graph,
    theirs: &Graph,
    shapes: &str,
) -> Result<(Graph, Vec<DecisionRecord>)> {
    let (b, o, t) = (by_slot(base), by_slot(ours), by_slot(theirs));
    let bounds = crate::merge3::ShapeBounds::parse(shapes)?;
    let view = crate::merge3::TripleView::new(base.iter().chain(ours).chain(theirs));
    let keys: BTreeSet<_> = b.keys().chain(o.keys()).chain(t.keys()).collect();
    let empty = BTreeSet::new();
    let mut graph = Graph::new();
    let mut conflicts = Vec::new();
    for slot in keys {
        let bv = b.get(slot).unwrap_or(&empty);
        let ov = o.get(slot).unwrap_or(&empty);
        let tv = t.get(slot).unwrap_or(&empty);
        let bound = bounds.bound(&slot.0, &slot.1, &view)?;
        let (values, held) = project(&input(bv, ov, tv, bound), None)?;
        if held {
            conflicts.push(DecisionRecord {
                subject: slot.0.clone(),
                predicate: slot.1.clone(),
                max_count: bound.unwrap_or(1),
                base: bv.iter().cloned().collect(),
                ours: ov.iter().cloned().collect(),
                theirs: tv.iter().cloned().collect(),
            });
        }
        for value in values {
            graph.extend(parse_graph(
                &format!("{} <{}> {} .", slot.0, slot.1, value),
                "pendant merge operation",
            )?);
        }
    }
    Ok((graph, conflicts))
}

/// Resolve a held slot through the core, including its cardinality gate.
pub(super) fn resolve(record: &DecisionRecord, choice: &str) -> Result<Vec<String>> {
    let decision = match choice {
        "continue" => Decision::Continue,
        "base" => Decision::Base,
        "ours" => Decision::Ours,
        "theirs" => Decision::Theirs,
        "both" => Decision::Both,
        value if value.starts_with("value:") => Decision::Values(
            serde_json::from_str::<Vec<String>>(&value[6..])
                .map_err(|e| super::invalid(format!("invalid value array: {e}")))?
                .into_iter()
                .collect(),
        ),
        _ => {
            return Err(super::invalid(
                "conflict resolution must be base/ours/theirs/both",
            ));
        }
    };
    let set = |values: &[String]| values.iter().cloned().collect();
    let input = input(
        &set(&record.base),
        &set(&record.ours),
        &set(&record.theirs),
        Some(record.max_count),
    );
    Ok(project(&input, Some(&decision))?.0.into_iter().collect())
}

#[cfg(test)]
#[path = "git_merge_core_tests.rs"]
mod tests;
