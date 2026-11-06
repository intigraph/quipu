//! One merge write core (aegis-karb.2, slice S1).
//!
//! Turns a three-way slot merge into the ordered fact operations that
//! `docs/design/merge-behaviour.md` specifies, independent of which merge
//! command asked for it. The value-set rule is [`crate::merge3::merge_slots`],
//! unchanged; this module adds what the spec adds on top of it:
//!
//! - **time** (spec §3.1): an incoming one-side change keeps its authored time;
//!   a value received in a decided conflict starts at the decision time `Td`;
//!   an incoming close is clamped so no row is reversed or emptied (R5, R6);
//! - **sequencing** (R3, ruling B on aegis-karb.2): a `receive` step (the clean
//!   part, plus a receipt `[Td, Td)` for every incoming value of a decided
//!   slot) and a `decide` step (losing ours values closed, every chosen value
//!   ROOT lacks asserted live);
//! - **reporting** (R2, R8): convergent slots are flagged with default decision
//!   `continue`, and every plan carries `applied` / `held` counts;
//! - **identity** (spec §2): a merge id that names one merge of one input.
//!
//! Slice S1 is pure and has no caller yet: share merge, fork promote and the
//! pendant merge move onto it in later slices. The caller supplies the slot's
//! cardinality, so tests can state it as a fixture does, without shapes.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::error::{Error, Result};

/// The cardinality a slot merges under (spec R4). List slots are slice S5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Card {
    /// `sh:maxCount n`; `One` is `Many(Some(1))` decided as a whole slot.
    Many(Option<usize>),
}

impl Card {
    pub(crate) const ONE: Card = Card::Many(Some(1));

    fn bound(self) -> Option<usize> {
        match self {
            Card::Many(b) => b,
        }
    }
}

/// How a slot changed relative to base.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Change {
    Unchanged,
    Ours,
    Theirs,
    Convergent,
    /// Both sides changed differently; merged value by value, no conflict.
    Divergent,
    /// Both sides changed differently and the cardinality refused the union.
    MaxcountExceeded,
    /// `card: one`, base set, one side deleted while the other replaced.
    DeleteReplace,
}

impl Change {
    fn is_conflict(self) -> bool {
        matches!(self, Change::MaxcountExceeded | Change::DeleteReplace)
    }
}

/// An operator decision for one slot (merge-reconcile.md §3.1, plus R2's
/// `continue` for a convergent slot).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Decision<V> {
    Ours,
    Theirs,
    Base,
    Both,
    Values(BTreeSet<V>),
    // Explicit convergent decisions arrive with the decisions schema slice.
    #[cfg_attr(not(test), allow(dead_code))]
    Continue,
    Defer,
}

/// One slot's three sides, with the time facts the spec needs.
#[derive(Debug, Clone)]
pub(crate) struct SlotInput<V> {
    pub(crate) base: BTreeSet<V>,
    /// ROOT's current values and each row's `valid_from`.
    pub(crate) ours: BTreeMap<V, String>,
    /// The incoming side's current values and each one's authored time
    /// (fork fact `valid_from`, or the share's `created_at`).
    pub(crate) theirs: BTreeMap<V, String>,
    /// When the incoming side closed a value it removed from base, if known.
    pub(crate) theirs_closed: BTreeMap<V, String>,
    pub(crate) card: Card,
}

/// One fact operation of a plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub(crate) enum FactOp<V> {
    /// Assert `value`, valid from `from`.
    Assert { value: V, from: String },
    /// Close the current row of `value` at `at`.
    Close { value: V, at: String },
    /// Record an incoming `value` as received with the empty interval
    /// `[at, at)`: in transaction history, current at no valid time, so the
    /// write gate never sees two open values on one slot (aegis-karb.2,
    /// malcolm's ruling B).
    Receipt { value: V, at: String },
}

/// What the core decided for one slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SlotPlan<V> {
    pub(crate) change: Change,
    pub(crate) receive: Vec<FactOp<V>>,
    pub(crate) decide: Vec<FactOp<V>>,
    /// Undecided (or deferred) conflict: nothing is written for this slot.
    pub(crate) held: bool,
}

/// The value-set step, shared with every merge command: `merge3`'s rule on a
/// single slot, so the conflict semantics cannot drift from it.
fn value_merge<V: Ord + Clone>(
    base: &BTreeSet<V>,
    ours: &BTreeSet<V>,
    theirs: &BTreeSet<V>,
    card: Card,
) -> Result<(BTreeSet<V>, bool)> {
    let one = |set: &BTreeSet<V>| BTreeMap::from([((), set.clone())]);
    let (b, o, t) = (one(base), one(ours), one(theirs));
    let empty = BTreeSet::new();
    let (merged, held) = crate::merge3::merge_slots(&b, &o, &t, |_| Ok(card.bound()), &empty)?;
    Ok((
        merged.get(&()).cloned().unwrap_or_default(),
        !held.is_empty(),
    ))
}

/// `a >= b` in valid time. Two RFC 3339 instants compare as instants (an
/// offset is honoured); anything else compares as the store compares valid
/// time, as text, so a date-only stamp such as `2026-01-05` keeps working.
fn not_before(a: &str, b: &str) -> bool {
    match (
        crate::time::epoch_of_rfc3339(a),
        crate::time::epoch_of_rfc3339(b),
    ) {
        (Some(x), Some(y)) => x >= y,
        _ => a >= b,
    }
}

/// The time at which an incoming close ends a ROOT row (spec §3.1 clamp). An
/// incoming close at or before the row's own start would erase or reverse its
/// history, so it closes at `td` instead; a later close is applied as is.
fn clamp_close(incoming: Option<&String>, row_from: &str, td: &str) -> String {
    match incoming {
        Some(at) if not_before(at, row_from) && !not_before(row_from, at) => at.clone(),
        _ => td.to_string(),
    }
}

/// Plan one slot at decision time `td`.
///
/// # Errors
/// [`Error::InvalidValue`] for a decision the slot cannot take (`continue` on
/// a conflict, a selected value set above the bound, a conflict decision on a clean slot).
/// Nothing is planned in that case.
pub(crate) fn plan_slot<V: Ord + Clone>(
    input: &SlotInput<V>,
    decision: Option<&Decision<V>>,
    td: &str,
) -> Result<SlotPlan<V>> {
    let bv = &input.base;
    let ov: BTreeSet<V> = input.ours.keys().cloned().collect();
    let tv: BTreeSet<V> = input.theirs.keys().cloned().collect();
    let (merged, conflict) = value_merge(bv, &ov, &tv, input.card)?;
    let change = if ov == *bv && tv == *bv {
        Change::Unchanged
    } else if tv == *bv {
        Change::Ours
    } else if ov == *bv {
        Change::Theirs
    } else if ov == tv {
        Change::Convergent
    } else if !conflict {
        Change::Divergent
    } else if input.card == Card::ONE && !bv.is_empty() && (ov.is_empty() || tv.is_empty()) {
        Change::DeleteReplace
    } else {
        Change::MaxcountExceeded
    };
    let mut plan = SlotPlan {
        change,
        receive: Vec::new(),
        decide: Vec::new(),
        held: false,
    };

    if !change.is_conflict() {
        if change == Change::Convergent {
            return convergent(plan, bv, &ov, input.card, decision, td);
        }
        if let Some(d) = decision
            && *d != Decision::Continue
        {
            return Err(Error::InvalidValue(
                "merge core: a decision names a slot that is not in conflict".into(),
            ));
        }
        // Clean: ROOT moves to the merged set. Incoming additions keep their
        // authored time; ROOT values the incoming side removed close at the
        // incoming close time, clamped.
        let added: Vec<(V, String)> = merged
            .difference(&ov)
            .map(|v| {
                let from = input.theirs.get(v).cloned();
                (v.clone(), from.unwrap_or_else(|| td.to_string()))
            })
            .collect();
        // R6 on a slot of at most one value: the replaced value ends where its
        // replacement starts. With no incoming close time (a share carries
        // none per fact), it closes at the replacement's authored start, and
        // the replacement never starts before that close (lex, #1003).
        let single = input.card.bound() == Some(1);
        let replacement_start = added
            .iter()
            .map(|(_, from)| from)
            .filter(|_| single)
            .min_by(|a, b| {
                if not_before(a, b) && not_before(b, a) {
                    std::cmp::Ordering::Equal
                } else if not_before(a, b) {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Less
                }
            });
        let mut last_close: Option<String> = None;
        for v in ov.difference(&merged) {
            let incoming = input.theirs_closed.get(v).or(replacement_start);
            let at = clamp_close(incoming, &input.ours[v], td);
            if last_close.as_ref().is_none_or(|c| !not_before(c, &at)) {
                last_close = Some(at.clone());
            }
            plan.receive.push(FactOp::Close {
                value: v.clone(),
                at,
            });
        }
        for (v, from) in added {
            let from = match &last_close {
                Some(close) if single && !not_before(&from, close) => close.clone(),
                _ => from,
            };
            plan.receive.push(FactOp::Assert { value: v, from });
        }
        return Ok(plan);
    }

    let target = match decision {
        None | Some(Decision::Defer) => {
            plan.held = true;
            return Ok(plan);
        }
        Some(Decision::Continue) => {
            return Err(Error::InvalidValue(
                "merge core: `continue` is only for a convergent slot; a conflict needs ours/theirs/base/both/value".into(),
            ));
        }
        Some(Decision::Ours) => ov.clone(),
        Some(Decision::Theirs) => tv.clone(),
        Some(Decision::Base) => bv.clone(),
        Some(Decision::Both) => ov.union(&tv).cloned().collect(),
        Some(Decision::Values(values)) => values.clone(),
    };
    check_bound(&target, input.card)?;
    // R3 (ruling B): every incoming value ROOT lacks is received as a receipt
    // `[td, td)`, so a loser never holds valid time and the store never holds
    // two open values between the steps.
    for v in tv.difference(&ov) {
        plan.receive.push(FactOp::Receipt {
            value: v.clone(),
            at: td.to_string(),
        });
    }
    // Decide: a losing ours closes at `td` (R5); every chosen value ROOT does
    // not hold, a received winner included, is asserted live from `td` (R6:
    // it starts where the loser ends).
    for v in ov.difference(&target) {
        plan.decide.push(FactOp::Close {
            value: v.clone(),
            at: td.to_string(),
        });
    }
    for v in target.difference(&ov) {
        plan.decide.push(FactOp::Assert {
            value: v.clone(),
            from: td.to_string(),
        });
    }
    Ok(plan)
}

/// R2: a convergent slot writes nothing under `continue`; `base` or `value`
/// override it as an ordinary decision at `td`.
fn convergent<V: Ord + Clone>(
    mut plan: SlotPlan<V>,
    bv: &BTreeSet<V>,
    ov: &BTreeSet<V>,
    card: Card,
    decision: Option<&Decision<V>>,
    td: &str,
) -> Result<SlotPlan<V>> {
    let target = match decision {
        None | Some(Decision::Continue) => return Ok(plan),
        Some(Decision::Base) => bv.clone(),
        Some(Decision::Values(values)) => values.clone(),
        Some(_) => {
            return Err(Error::InvalidValue(
                "merge core: a convergent slot takes continue, base or value".into(),
            ));
        }
    };
    check_bound(&target, card)?;
    for v in ov.difference(&target) {
        plan.decide.push(FactOp::Close {
            value: v.clone(),
            at: td.to_string(),
        });
    }
    for v in target.difference(ov) {
        plan.decide.push(FactOp::Assert {
            value: v.clone(),
            from: td.to_string(),
        });
    }
    Ok(plan)
}

fn check_bound<V>(values: &BTreeSet<V>, card: Card) -> Result<()> {
    if let Some(n) = card.bound()
        && values.len() > n
    {
        return Err(Error::InvalidValue(format!(
            "merge core: decision would hold {} values, above sh:maxCount {n}",
            values.len(),
        )));
    }
    Ok(())
}

/// A whole merge's plan: per-slot plans, the two ordered steps, and counts.
#[derive(Debug, Clone)]
pub(crate) struct MergePlan<S, V> {
    pub(crate) slots: BTreeMap<S, SlotPlan<V>>,
    /// Atomic mode (spec §6): when any slot is held, nothing is written.
    pub(crate) receive: Vec<(S, FactOp<V>)>,
    pub(crate) decide: Vec<(S, FactOp<V>)>,
    /// Slots written (a convergent `continue` is not a write).
    pub(crate) applied: usize,
    pub(crate) held: usize,
}

/// Plan a whole merge, atomically: if any slot is held, both steps are empty
/// and `applied` is 0 (spec §6 `atomic`, R8).
///
/// # Errors
/// As [`plan_slot`], and a decision naming a slot that is not in the merge.
pub(crate) fn plan_merge<S: Ord + Clone, V: Ord + Clone>(
    slots: &BTreeMap<S, SlotInput<V>>,
    decisions: &BTreeMap<S, Decision<V>>,
    td: &str,
) -> Result<MergePlan<S, V>> {
    if decisions.keys().any(|s| !slots.contains_key(s)) {
        return Err(Error::InvalidValue(
            "merge core: a decision names a slot that is not in the merge".into(),
        ));
    }
    let mut out = MergePlan {
        slots: BTreeMap::new(),
        receive: Vec::new(),
        decide: Vec::new(),
        applied: 0,
        held: 0,
    };
    for (slot, input) in slots {
        let plan = plan_slot(input, decisions.get(slot), td)?;
        out.held += usize::from(plan.held);
        if !plan.receive.is_empty() || !plan.decide.is_empty() {
            out.applied += 1;
        }
        out.receive
            .extend(plan.receive.iter().map(|op| (slot.clone(), op.clone())));
        out.decide
            .extend(plan.decide.iter().map(|op| (slot.clone(), op.clone())));
        out.slots.insert(slot.clone(), plan);
    }
    if out.held > 0 {
        out.receive.clear();
        out.decide.clear();
        out.applied = 0;
    }
    Ok(out)
}

/// The identity of one merge (spec §2): path, incoming identity, base
/// identity and the decisions file's SHA-256 (of `[]` for a clean merge).
/// Two different merges never share it; the same input merged again does,
/// which is what lets a re-apply report `already-applied` (R7).
#[must_use]
pub(crate) fn merge_id(path: &str, incoming: &str, base: &str, decisions_sha256: &str) -> String {
    let record = serde_json::json!({
        "schema": "quipu.merge-id/v1",
        "path": path,
        "incoming": incoming,
        "base": base,
        "decisions_sha256": decisions_sha256,
    });
    crate::share::sha256(record.to_string().as_bytes())
}

#[cfg(test)]
#[path = "merge_core_tests.rs"]
mod tests;
