//! Arms that run on the model graphs, and the scoring every arm shares.
//!
//! An arm's run is a *trajectory*: for each promotion in agent order, the
//! slots the arm asked a person about and the slots the oracle says a person
//! had to decide, plus ROOT's final state. Every metric below is a function of
//! the trajectory and the scripted writes; none reads arm internals.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::generate::ground_truth;
use crate::model::{Graph, Slot, Term, by_slot};
use crate::scenario::{Kind, Scenario, apply, plain, slot_key};
use crate::shapes;
use crate::strategies;

/// One promotion as an arm saw it.
#[derive(Debug, Clone, Default)]
pub struct Promotion {
    /// Slots the arm asked a person to decide.
    pub asked: BTreeSet<Slot>,
    /// Slots the oracle says needed a person, on this arm's own inputs.
    pub needed: BTreeSet<Slot>,
    /// Promote attempts (1 = landed first time).
    pub rounds: usize,
}

/// A whole run of one arm.
#[derive(Debug, Clone, Default)]
pub struct Trajectory {
    /// One entry per agent, in promotion order.
    pub promotions: Vec<Promotion>,
    /// ROOT before the first promotion, then after each one (`n + 1` graphs).
    pub states: Vec<Graph>,
}

impl Trajectory {
    /// ROOT after the last promotion.
    #[must_use]
    pub fn final_root(&self) -> Graph {
        self.states.last().cloned().unwrap_or_default()
    }
}

/// The oracle for one promotion: the merge paper's `ground_truth` over
/// `(base, ROOT now, the fork)`. Every agent forks at base.
#[must_use]
pub fn needed(base: &Graph, ours: &Graph, theirs: &Graph) -> BTreeSet<Slot> {
    let none = BTreeMap::new();
    ground_truth(base, ours, theirs, &none, &none)
        .into_keys()
        .collect()
}

/// The model graph each agent's fork ends up as.
#[must_use]
pub fn forks(sc: &Scenario) -> Vec<Graph> {
    sc.agents
        .iter()
        .map(|ops| {
            let mut g = sc.base.clone();
            for op in ops {
                apply(&mut g, op);
            }
            g
        })
        .collect()
}

/// ROOT after the live writer, before any promotion.
#[must_use]
pub fn root_before_promotions(sc: &Scenario) -> Graph {
    let mut g = sc.base.clone();
    for op in &sc.root {
        apply(&mut g, op);
    }
    g
}

/// Replace `slot` in `g` with `theirs`' values for it — the person's ruling.
pub fn take_theirs(g: &mut Graph, theirs: &Graph, slot: &Slot) {
    g.retain(|t| !(t.s == slot.0 && t.p == slot.1));
    for t in theirs {
        if t.s == slot.0 && t.p == slot.1 {
            g.insert(t.clone());
        }
    }
}

/// The model arms: the merge paper's strategies run end to end over the N
/// promotions, plus the pre-#888 two-way promote (`two-way`: ROOT becomes the
/// fork). A conflicted slot is settled by the scripted person, who takes the
/// promoting agent's value (`take_theirs`) — the same ruling every arm gets.
#[must_use]
pub fn run_model_arm(sc: &Scenario, arm: &str) -> Trajectory {
    let forks = forks(sc);
    let mut root = root_before_promotions(sc);
    let mut states = vec![root.clone()];
    let mut promotions = Vec::with_capacity(forks.len());
    for fork in &forks {
        let need = needed(&sc.base, &root, fork);
        if arm == "two-way" {
            root = fork.clone();
            states.push(root.clone());
            promotions.push(Promotion {
                asked: BTreeSet::new(),
                needed: need,
                rounds: 1,
            });
            continue;
        }
        let out = strategies::run(arm, &sc.base, &root, fork);
        let mut next = out.merged;
        for slot in &out.conflicts {
            take_theirs(&mut next, fork, slot);
        }
        root = next;
        states.push(root.clone());
        promotions.push(Promotion {
            asked: out.conflicts,
            needed: need,
            rounds: 1,
        });
    }
    Trajectory { promotions, states }
}

/// What one writer meant a slot to end up holding.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Intent {
    /// The slot holds exactly this value.
    Is(Term),
    /// The slot is empty.
    Empty,
    /// The slot contains this value (multi-valued).
    Has(Term),
}

/// Every write's intent. A writer's later write to a single-valued slot
/// supersedes its own earlier one; additions each stand alone.
fn intents(sc: &Scenario) -> Vec<(String, Slot, Intent)> {
    let mut last: BTreeMap<(String, Slot), Intent> = BTreeMap::new();
    let mut adds: BTreeSet<(String, Slot, Intent)> = BTreeSet::new();
    for op in sc.root.iter().chain(sc.agents.iter().flatten()) {
        let slot = op.slot();
        match (op.kind, &op.value) {
            (Kind::Add, Some(v)) => {
                adds.insert((op.writer.clone(), slot, Intent::Has(v.clone())));
            }
            (Kind::Set | Kind::Correct, Some(v)) => {
                last.insert((op.writer.clone(), slot), Intent::Is(v.clone()));
            }
            _ => {
                last.insert((op.writer.clone(), slot), Intent::Empty);
            }
        }
    }
    last.into_iter()
        .map(|((w, s), i)| (w, s, i))
        .chain(adds)
        .collect()
}

fn reflected(intent: &Intent, values: Option<&BTreeSet<Term>>) -> bool {
    let empty = BTreeSet::new();
    let values = values.unwrap_or(&empty);
    match intent {
        Intent::Is(v) => values.len() == 1 && values.contains(v),
        Intent::Empty => values.is_empty(),
        Intent::Has(v) => values.contains(v),
    }
}

/// Whether a write was lost without a question. A write is *due* in ROOT
/// from the state its writer's promotion produced (the live writer's, from
/// before the first promotion). Walking forward from there, the first state
/// that does not reflect it is where it was dropped: that drop is silent unless
/// the promotion that produced the state asked a person about the slot. A
/// write that is reflected in every state from its due point was not lost.
fn silently_lost(
    writer: &str,
    slot: &Slot,
    intent: &Intent,
    states: &[BTreeMap<Slot, BTreeSet<Term>>],
    promotions: &[Promotion],
) -> bool {
    let due = writer
        .strip_prefix("agent-")
        .and_then(|a| a.parse::<usize>().ok())
        .map_or(0, |a| a + 1);
    for (k, state) in states.iter().enumerate().skip(due) {
        if !reflected(intent, state.get(slot)) {
            // State k was produced by promotion k - 1.
            return k == 0 || !promotions[k - 1].asked.contains(slot);
        }
    }
    false
}

/// The scored result of one arm.
#[derive(Debug, Clone, Serialize)]
pub struct ArmMetrics {
    /// Arm name.
    pub arm: String,
    /// `model` (merge run on the model graphs) or `store` (real `quipu::Store`).
    pub runs_on: &'static str,
    /// Slots asked of a person, summed over promotions.
    pub questions: usize,
    /// Slots the oracle says needed a person.
    pub needed: usize,
    /// Asked and needed.
    pub caught: usize,
    /// Needed but not asked.
    pub missed: usize,
    /// Asked but not needed.
    pub unnecessary: usize,
    /// Scripted writes in the run.
    pub writes: usize,
    /// Writes absent from final ROOT whose slot was never put to a person.
    pub silent_losses: usize,
    /// Final-ROOT slots over their `sh:maxCount`.
    pub violations: usize,
    /// Promote attempts, summed over agents.
    pub rounds: usize,
    /// Every slot asked, `e<n>|<predicate>`, for the cross-scorer check.
    pub asked: Vec<String>,
    /// Final ROOT, slot -> plain values, for the cross-scorer check.
    pub final_root: BTreeMap<String, Vec<String>>,
    /// Slots asked at each promotion, for the cross-scorer check.
    pub asked_by_promotion: Vec<Vec<String>>,
    /// ROOT before and after each promotion, as the slots that differ from
    /// base (an emptied slot is `[]`), for the cross-scorer check.
    pub states: Vec<BTreeMap<String, Vec<String>>>,
}

fn delta(
    base: &BTreeMap<Slot, BTreeSet<Term>>,
    state: &BTreeMap<Slot, BTreeSet<Term>>,
) -> BTreeMap<String, Vec<String>> {
    let empty = BTreeSet::new();
    let mut out = BTreeMap::new();
    for slot in base.keys().chain(state.keys()) {
        let (b, s) = (
            base.get(slot).unwrap_or(&empty),
            state.get(slot).unwrap_or(&empty),
        );
        if b != s {
            out.insert(slot_key(slot), s.iter().map(|v| plain(v)).collect());
        }
    }
    out
}

/// Score a trajectory.
#[must_use]
pub fn score(sc: &Scenario, arm: &str, runs_on: &'static str, t: &Trajectory) -> ArmMetrics {
    let base_slots = by_slot(&sc.base);
    let mut questions = 0;
    let mut needed_n = 0;
    let mut caught = 0;
    let mut rounds = 0;
    let mut asked_all: BTreeSet<Slot> = BTreeSet::new();
    for p in &t.promotions {
        questions += p.asked.len();
        needed_n += p.needed.len();
        caught += p.asked.intersection(&p.needed).count();
        rounds += p.rounds;
        asked_all.extend(p.asked.iter().cloned());
    }
    let states: Vec<_> = t.states.iter().map(by_slot).collect();
    let final_slots = states.last().cloned().unwrap_or_default();
    let silent_losses = intents(sc)
        .iter()
        .filter(|(writer, slot, intent)| {
            silently_lost(writer, slot, intent, &states, &t.promotions)
        })
        .count();
    let violations = final_slots
        .iter()
        .filter(|(slot, vs)| shapes::max_count(&slot.1).is_some_and(|n| vs.len() > n))
        .count();
    ArmMetrics {
        arm: arm.to_string(),
        runs_on,
        questions,
        needed: needed_n,
        caught,
        missed: needed_n - caught,
        unnecessary: questions - caught,
        writes: sc.root.len() + sc.agents.iter().map(Vec::len).sum::<usize>(),
        silent_losses,
        violations,
        rounds,
        asked: asked_all.iter().map(slot_key).collect(),
        asked_by_promotion: t
            .promotions
            .iter()
            .map(|p| p.asked.iter().map(slot_key).collect())
            .collect(),
        states: states.iter().map(|st| delta(&base_slots, st)).collect(),
        final_root: final_slots
            .iter()
            .filter(|(slot, _)| !slot.1.ends_with("#type"))
            .map(|(slot, vs)| (slot_key(slot), vs.iter().map(|v| plain(v)).collect()))
            .collect(),
    }
}
