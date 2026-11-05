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

fn receipt(value: V, at: &str) -> FactOp<V> {
    FactOp::Receipt {
        value,
        at: at.into(),
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
fn c1a_share_without_a_close_time_closes_at_the_replacement_start() {
    // A share carries no per-fact close. On a single-valued slot the replaced
    // value must end where its replacement starts (R6), never at Td.
    let p = plan(&slot(&["a"], &[("a", T0)], &[("c", TT)], Card::ONE), None);
    assert_eq!(p.receive, vec![close("a", TT), assert_("c", TT)]);
}

#[test]
fn a_single_value_replacement_never_starts_before_the_close() {
    // ROOT's row starts after the incoming value's authored time (a re-claim),
    // so the close clamps to Td and the replacement starts there too.
    let p = plan(&slot(&["a"], &[("a", TR)], &[("c", TT)], Card::ONE), None);
    assert_eq!(p.receive, vec![close("a", TD), assert_("c", TD)]);
    // An incoming close later than the replacement's authored start wins, and
    // the replacement waits for it.
    let mut s = slot(&["a"], &[("a", T0)], &[("c", TT)], Card::ONE);
    s.theirs_closed = rows(&[("a", TF)]);
    assert_eq!(
        plan(&s, None).receive,
        vec![close("a", TF), assert_("c", TF)]
    );
    // A many-valued slot has no such coupling: unrelated values keep their
    // own times.
    let p = plan(
        &slot(&["a"], &[("a", T0)], &[("c", TT)], Card::Many(None)),
        None,
    );
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
fn c2_convergent_base_restores_base_at_decision_time() {
    for (ours, theirs, expected) in [
        (
            vec![("c", TO)],
            vec![("c", TT)],
            vec![close("c", TD), assert_("a", TD)],
        ),
        (vec![], vec![], vec![assert_("a", TD)]),
    ] {
        let s = slot(&["a"], &ours, &theirs, Card::ONE);
        let result = plan_slot(&s, Some(&Decision::Base), TD).unwrap();
        assert_eq!(result.change, Change::Convergent);
        assert!(!result.held);
        assert!(result.receive.is_empty());
        assert_eq!(result.decide, expected);
    }
}

#[test]
fn explicit_values_refuse_over_cardinality_in_conflicts_and_convergence() {
    for theirs in ["c", "b"] {
        let s = slot(&["a"], &[("b", TO)], &[(theirs, TT)], Card::ONE);
        let decision = Decision::Values(set(&["d", "e"]));
        assert!(plan_slot(&s, Some(&decision), TD).is_err());
        assert!(plan_slot(&s, Some(&Decision::Values(set(&["d"]))), TD).is_ok());
    }
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

    // Every decided row receives c as a receipt [Td, Td) (ruling B): never
    // open, never at its authored Tt.
    let received = vec![receipt("c", TD)];
    let ours = plan(&s, Some(Decision::Ours));
    assert_eq!((ours.receive, ours.decide), (received.clone(), vec![]));
    let theirs = plan(&s, Some(Decision::Theirs));
    assert_eq!(
        (theirs.receive, theirs.decide),
        (received.clone(), vec![close("b", TD), assert_("c", TD)])
    );
    let base = plan(&s, Some(Decision::Base));
    assert_eq!(
        (base.receive, base.decide),
        (received.clone(), vec![close("b", TD), assert_("a", TD)])
    );
    let value = plan(&s, Some(Decision::Values(set(&["d"]))));
    assert_eq!(
        (value.receive, value.decide),
        (received, vec![close("b", TD), assert_("d", TD)])
    );
    assert!(plan_slot(&s, Some(&Decision::Both), TD).is_err());
    assert!(plan_slot(&s, Some(&Decision::Continue), TD).is_err());
}

#[test]
fn c4_theirs_never_overlaps_ours_in_valid_time() {
    // The measured fork-promote defect: the winner must start where the loser
    // ends, not at its authored time.
    let p = plan(&c4(), Some(Decision::Theirs));
    let open_starts: Vec<_> = p
        .receive
        .iter()
        .chain(&p.decide)
        .filter_map(|op| match op {
            FactOp::Assert { from, .. } => Some(from.as_str()),
            FactOp::Close { .. } | FactOp::Receipt { .. } => None,
        })
        .collect();
    assert_eq!(open_starts, vec![TD]);
    assert!(p.decide.contains(&close("b", TD)));
    // No open assertion in receive: the gate never sees b and c both open.
    assert!(
        p.receive
            .iter()
            .all(|op| matches!(op, FactOp::Receipt { .. }))
    );
}

#[test]
fn c5_delete_vs_replace() {
    // C5a: ours deleted, theirs replaced.
    let s = slot(&["a"], &[], &[("c", TT)], Card::ONE);
    assert_eq!(plan(&s, None).change, Change::DeleteReplace);
    let p = plan(&s, Some(Decision::Ours));
    assert_eq!((p.receive, p.decide), (vec![receipt("c", TD)], vec![]));
    let p = plan(&s, Some(Decision::Base));
    assert_eq!(p.decide, vec![assert_("a", TD)]);
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
        vec![("s1", close("b", TD)), ("s1", assert_("c", TD))]
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

#[test]
fn date_only_stamps_clamp_in_the_store_s_text_order() {
    // The store accepts and compares date-only valid times as text.
    let mut s = slot(&["a"], &[("a", "2026-01-05")], &[], Card::ONE);
    s.theirs_closed = rows(&[("a", "2026-01-04")]);
    assert_eq!(
        plan_slot(&s, None, "2026-01-10").unwrap().receive,
        vec![close("a", "2026-01-10")]
    );
    s.theirs_closed = rows(&[("a", "2026-01-06")]);
    assert_eq!(
        plan_slot(&s, None, "2026-01-10").unwrap().receive,
        vec![close("a", "2026-01-06")]
    );
}

#[test]
fn decision_override_demo() {
    let s = slot(&["a"], &[("c", TO)], &[("c", TT)], Card::ONE);
    let base = plan_slot(&s, Some(&Decision::Base), TD);
    let oversized = plan_slot(&s, Some(&Decision::Values(set(&["d", "e"]))), TD);
    let keep = plan_slot(&s, Some(&Decision::Continue), TD).unwrap();
    println!("Base override: {base:?}");
    println!("Two explicit values under maxCount=1: {oversized:?}");
    println!("Continue control: {keep:?}");
    assert_eq!(base.unwrap().decide, vec![close("c", TD), assert_("a", TD)]);
    assert!(oversized.is_err());
    assert!(keep.receive.is_empty() && keep.decide.is_empty());
}
