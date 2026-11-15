//! Benchmark-only bridge to the actual common planner, not a second merge rule.
//!
//! This projection has no store, profile admission, history or commit evidence.
//! Values are opaque ground-term strings; cardinality is supplied by the caller.

use std::collections::{BTreeMap, BTreeSet};

use crate::Result;
use crate::merge_core::{Card, Change, FactOp, SlotInput, plan_slot};

/// Value-set projection of one undecided slot, evaluated at a synthetic later cut.
#[derive(Debug, PartialEq, Eq)]
pub struct Projection {
    /// Current values; held conflicts use base as the benchmark display policy.
    pub current: BTreeSet<String>,
    /// The real planner refused to settle this slot without a decision.
    pub held: bool,
    /// Both writers independently reached the same changed value set.
    pub convergent: bool,
}

/// Plan one ground-value slot with explicit fixture cardinality and no decision.
///
/// Uses fixed ordered synthetic times: all assertions precede the decision cut.
/// This is intentionally NOT an adapter for temporal/history competency questions.
/// Errors propagate; they are never converted into empty successful output.
pub fn project_slot(
    base: &BTreeSet<String>,
    ours: &BTreeSet<String>,
    theirs: &BTreeSet<String>,
    max_count: Option<usize>,
) -> Result<Projection> {
    let rows = |values: &BTreeSet<String>| -> BTreeMap<String, String> {
        values
            .iter()
            .map(|v| (v.clone(), "2026-01-01T00:00:00Z".into()))
            .collect()
    };
    let plan = plan_slot(
        &SlotInput {
            base: base.clone(),
            ours: rows(ours),
            theirs: rows(theirs),
            theirs_closed: BTreeMap::new(),
            card: Card::Many(max_count),
        },
        None,
        "2026-01-02T00:00:00Z",
    )?;
    let mut current = ours.clone();
    if plan.held {
        // Report/display convention, not a claim the writer rolled ROOT back.
        current = base.clone();
    } else {
        for op in plan.receive.iter().chain(&plan.decide) {
            match op {
                FactOp::Assert { value, .. } => {
                    current.insert(value.clone());
                }
                FactOp::Close { value, .. } => {
                    current.remove(value);
                }
                FactOp::Receipt { .. } => {} // Empty intervals never become current.
            }
        }
    }
    Ok(Projection {
        current,
        held: plan.held,
        convergent: plan.change == Change::Convergent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn set(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|s| (*s).into()).collect()
    }

    #[test]
    fn theirs_only_uses_core_close_and_assert_projection() {
        let p = project_slot(&set(&["a"]), &set(&["a"]), &set(&["b"]), Some(1)).unwrap();
        assert_eq!(p.current, set(&["b"]));
        assert!(!p.held && !p.convergent);
    }

    #[test]
    fn delete_replace_is_held_even_when_union_fits_bound() {
        let p = project_slot(&set(&["a"]), &set(&[]), &set(&["b"]), Some(1)).unwrap();
        assert!(p.held);
        assert_eq!(p.current, set(&["a"]));
    }

    #[test]
    fn convergent_overbound_is_flagged_without_inventing_conflict() {
        let p = project_slot(&set(&[]), &set(&["a", "b"]), &set(&["a", "b"]), Some(1)).unwrap();
        assert!(p.convergent && !p.held);
        assert_eq!(p.current, set(&["a", "b"]));
    }

    #[test]
    fn unbounded_divergence_keeps_both_new_values() {
        let p = project_slot(&set(&["a"]), &set(&["b"]), &set(&["c"]), None).unwrap();
        assert!(!p.held && !p.convergent);
        assert_eq!(p.current, set(&["b", "c"]));
    }
}
