//! The spec fixtures of `docs/design/merge-behaviour.md`, at the plan level.
//! Cardinality is stated by each fixture, never read from shapes (spec §1).

use std::collections::{BTreeMap, BTreeSet};

use super::*;

const T0: &str = "2026-01-01T00:00:00Z";
const TO: &str = "2026-01-02T00:00:00Z";
const TT: &str = "2026-01-03T00:00:00Z";
const TF: &str = "2026-01-04T00:00:00Z";
const TR: &str = "2026-01-05T00:00:00Z";
const TD: &str = "2026-01-10T00:00:00Z";
const TZ: &str = "2026-02-01T00:00:00Z";

type V = &'static str;

fn set(vs: &[V]) -> BTreeSet<V> {
    vs.iter().copied().collect()
}

fn rows(vs: &[(V, &str)]) -> BTreeMap<V, String> {
    vs.iter().map(|(v, t)| (*v, t.to_string())).collect()
}

fn slot(base: &[V], ours: &[(V, &str)], theirs: &[(V, &str)], card: Card) -> SlotInput<V> {
    SlotInput {
        base: set(base),
        ours: rows(ours),
        theirs: rows(theirs),
        theirs_closed: BTreeMap::new(),
        card,
    }
}

fn assert_(value: V, from: &str) -> FactOp<V> {
    FactOp::Assert {
        value,
        from: from.into(),
    }
}

fn close(value: V, at: &str) -> FactOp<V> {
    FactOp::Close {
        value,
        at: at.into(),
    }
}

// By value so fixtures read `Some(Decision::Ours)`; test helper only.
#[allow(clippy::needless_pass_by_value)]
fn plan(input: &SlotInput<V>, decision: Option<Decision<V>>) -> SlotPlan<V> {
    plan_slot(input, decision.as_ref(), TD).unwrap()
}

#[test]
fn c1a_one_side_theirs_keeps_authored_time_and_closes_base_at_their_close() {
    let mut s = slot(&["a"], &[("a", T0)], &[("c", TT)], Card::ONE);
    s.theirs_closed = rows(&[("a", TT)]);
    let p = plan(&s, None);
    assert_eq!(p.change, Change::Theirs);
    assert_eq!(p.receive, vec![close("a", TT), assert_("c", TT)]);
    assert!(p.decide.is_empty() && !p.held);
}

#[test]
fn c1a_share_without_a_close_time_closes_at_td() {
    // A share carries no per-fact close; the caller passes created_at when it
    // has it. With none, the close falls back to the decision time.
    let p = plan(&slot(&["a"], &[("a", T0)], &[("c", TT)], Card::ONE), None);
    assert_eq!(p.receive, vec![close("a", TD), assert_("c", TT)]);
}

#[test]
fn c1b_ours_only_change_writes_nothing() {
    let p = plan(&slot(&["a"], &[("b", TO)], &[("a", T0)], Card::ONE), None);
    assert_eq!(p.change, Change::Ours);
    assert!(p.receive.is_empty() && p.decide.is_empty());
}

#[test]
fn c1c_one_side_delete_closes_at_their_close() {
    let mut s = slot(&["a"], &[("a", T0)], &[], Card::ONE);
    s.theirs_closed = rows(&[("a", TT)]);
    assert_eq!(plan(&s, None).receive, vec![close("a", TT)]);
}

#[test]
fn c2_convergent_is_flagged_writes_nothing_and_value_overrides() {
    let s = slot(&["a"], &[("c", TO)], &[("c", TT)], Card::ONE);
    let p = plan(&s, None);
    assert_eq!(p.change, Change::Convergent);
    assert!(p.receive.is_empty() && p.decide.is_empty() && !p.held);
    assert_eq!(plan(&s, Some(Decision::Continue)), p);
    // C2b: both deleted.
    let p = plan(&slot(&["a"], &[], &[], Card::ONE), None);
    assert_eq!(p.change, Change::Convergent);
    // C2c: override with a typed value.
    let p = plan(&s, Some(Decision::Values(set(&["d"]))));
    assert_eq!(p.decide, vec![close("c", TD), assert_("d", TD)]);
    assert!(plan_slot(&s, Some(&Decision::Theirs), TD).is_err());
}

#[test]
fn c3_set_valued_divergence_unions_without_conflict() {
    let many = Card::Many(None);
    // C3a
    let p = plan(
        &slot(
            &["a"],
            &[("a", T0), ("b", TO)],
            &[("a", T0), ("c", TT)],
            many,
        ),
        None,
    );
    assert_eq!(p.change, Change::Divergent);
    assert_eq!(p.receive, vec![assert_("c", TT)]);
    // C3b: ROOT already closed a; only c arrives.
    let p = plan(&slot(&["a"], &[("b", TO)], &[("c", TT)], many), None);
    assert_eq!(p.receive, vec![assert_("c", TT)]);
    // C3c: theirs removed a while ours kept it and added b.
    let mut s = slot(&["a"], &[("a", T0), ("b", TO)], &[], many);
    s.theirs_closed = rows(&[("a", TT)]);
    assert_eq!(plan(&s, None).receive, vec![close("a", TT)]);
    // C3d: a value both added is never asserted twice.
    let p = plan(
        &slot(&[], &[("d", TO)], &[("d", TT), ("e", TT)], many),
        None,
    );
    assert_eq!(p.receive, vec![assert_("e", TT)]);
}

#[test]
fn c3e_bounded_union_over_the_bound_conflicts() {
    let s = slot(
        &["a"],
        &[("a", T0), ("b", TO)],
        &[("a", T0), ("c", TT)],
        Card::Many(Some(2)),
    );
    let p = plan(&s, None);
    assert_eq!(p.change, Change::MaxcountExceeded);
    assert!(p.held);
}

fn c4() -> SlotInput<V> {
    slot(&["a"], &[("b", TO)], &[("c", TT)], Card::ONE)
}

#[test]
fn c4_each_decision_is_sequenced_receive_then_decide() {
    let s = c4();
    let undecided = plan(&s, None);
    assert_eq!(undecided.change, Change::MaxcountExceeded);
    assert!(undecided.held && undecided.receive.is_empty());
    assert!(plan(&s, Some(Decision::Defer)).held);

    // Every decided row receives c from Td, never from its authored Tt.
    let received = vec![assert_("c", TD)];
    let ours = plan(&s, Some(Decision::Ours));
    assert_eq!(
        (ours.receive, ours.decide),
        (received.clone(), vec![close("c", TD)])
    );
    let theirs = plan(&s, Some(Decision::Theirs));
    assert_eq!(
        (theirs.receive, theirs.decide),
        (received.clone(), vec![close("b", TD)])
    );
    let base = plan(&s, Some(Decision::Base));
    assert_eq!(
        (base.receive, base.decide),
        (
            received.clone(),
            vec![close("b", TD), close("c", TD), assert_("a", TD)]
        )
    );
    let value = plan(&s, Some(Decision::Values(set(&["d"]))));
    assert_eq!(
        (value.receive, value.decide),
        (
            received,
            vec![close("b", TD), close("c", TD), assert_("d", TD)]
        )
    );
    assert!(plan_slot(&s, Some(&Decision::Both), TD).is_err());
    assert!(plan_slot(&s, Some(&Decision::Continue), TD).is_err());
}

#[test]
fn c4_theirs_never_overlaps_ours_in_valid_time() {
    // The measured fork-promote defect: the winner must start where the loser
    // ends, not at its authored time.
    let p = plan(&c4(), Some(Decision::Theirs));
    let starts: Vec<_> = p
        .receive
        .iter()
        .filter_map(|op| match op {
            FactOp::Assert { from, .. } => Some(from.as_str()),
            FactOp::Close { .. } => None,
        })
        .collect();
    assert_eq!(starts, vec![TD]);
    assert!(p.decide.contains(&close("b", TD)));
}

#[test]
fn c5_delete_vs_replace() {
    // C5a: ours deleted, theirs replaced.
    let s = slot(&["a"], &[], &[("c", TT)], Card::ONE);
    assert_eq!(plan(&s, None).change, Change::DeleteReplace);
    let p = plan(&s, Some(Decision::Ours));
    assert_eq!(
        (p.receive, p.decide),
        (vec![assert_("c", TD)], vec![close("c", TD)])
    );
    let p = plan(&s, Some(Decision::Base));
    assert_eq!(p.decide, vec![close("c", TD), assert_("a", TD)]);
    // C5b: ours replaced, theirs deleted. Nothing incoming to receive; a
    // losing ours closes at Td, not at the incoming close time.
    let mut s = slot(&["a"], &[("c", TO)], &[], Card::ONE);
    s.theirs_closed = rows(&[("a", TT)]);
    let p = plan(&s, Some(Decision::Theirs));
    assert_eq!(p.change, Change::DeleteReplace);
    assert_eq!((p.receive, p.decide), (vec![], vec![close("c", TD)]));
    // C5c: unbounded, no conflict.
    let p = plan(&slot(&["a"], &[], &[("c", TT)], Card::Many(None)), None);
    assert_eq!(
        (p.change, p.receive),
        (Change::Divergent, vec![assert_("c", TT)])
    );
}

#[test]
fn c7_root_reclaim_after_the_fork_is_clamped_to_td() {
    // aegis-0s2a: the fork's close Tf predates ROOT's row (re-claimed at Tr).
    let mut s = slot(&["a"], &[("a", TR)], &[], Card::ONE);
    s.theirs_closed = rows(&[("a", TF)]);
    assert_eq!(plan(&s, None).receive, vec![close("a", TD)]);
}

#[test]
fn c7b_a_close_later_than_td_is_applied_as_is() {
    let mut s = slot(&["a"], &[("a", T0)], &[], Card::ONE);
    s.theirs_closed = rows(&[("a", TZ)]);
    assert_eq!(plan(&s, None).receive, vec![close("a", TZ)]);
}

#[test]
fn a_decision_on_a_clean_slot_is_refused() {
    let s = slot(&["a"], &[("a", T0)], &[("c", TT)], Card::ONE);
    assert!(plan_slot(&s, Some(&Decision::Ours), TD).is_err());
    assert!(plan_slot(&s, None, "not a time").is_err());
}

#[test]
fn plan_merge_is_atomic_and_counts_applied_and_held() {
    let clean = slot(&["a"], &[("a", T0)], &[("c", TT)], Card::ONE);
    let mut slots = BTreeMap::from([("s1", c4()), ("s2", c4()), ("s3", clean)]);
    let mut decisions = BTreeMap::from([("s1", Decision::Theirs)]);
    let held = plan_merge(&slots, &decisions, TD).unwrap();
    assert_eq!((held.applied, held.held), (0, 1));
    assert!(held.receive.is_empty() && held.decide.is_empty());

    decisions.insert("s2", Decision::Ours);
    let done = plan_merge(&slots, &decisions, TD).unwrap();
    assert_eq!((done.applied, done.held), (3, 0));
    // Receive holds the clean slot and both received values; decide follows.
    assert_eq!(done.receive.len(), 4);
    assert_eq!(
        done.decide,
        vec![("s1", close("b", TD)), ("s2", close("c", TD))]
    );

    decisions.insert("nope", Decision::Ours);
    assert!(plan_merge(&slots, &decisions, TD).is_err());
    slots.clear();
    decisions.clear();
    let empty = plan_merge(&slots, &decisions, TD).unwrap();
    assert_eq!((empty.applied, empty.held), (0, 0));
}

#[test]
fn c8d_merge_id_names_one_merge_of_one_input() {
    let empty = crate::share::sha256(b"[]");
    let a = merge_id("share", "share-A", "share-0", &empty);
    let b = merge_id("share", "share-B", "share-0", &empty);
    assert_ne!(a, b, "two different clean merges must not share an id");
    assert_eq!(a, merge_id("share", "share-A", "share-0", &empty));
    assert_ne!(a, merge_id("fork", "share-A", "share-0", &empty));
    assert_ne!(a, merge_id("share", "share-A", "share-1", &empty));
    assert_ne!(
        a,
        merge_id("share", "share-A", "share-0", &crate::share::sha256(b"{}"))
    );
}
