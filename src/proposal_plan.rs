//! Blast-radius plan and risk score for a schema-change proposal (aegis-493).
//!
//! Before a proposal is accepted, [`plan`] reports what it touches, without
//! writing anything:
//!
//! - the classes and properties the proposed Turtle names, and (for a shape
//!   proposal) the terms the current version names that the proposal drops;
//! - how many existing ROOT instances the proposal would make NEWLY fail SHACL:
//!   the focus nodes its targets select are validated under the current shapes
//!   and under the shapes as they would stand after acceptance, and only issues
//!   the current shapes do not already report are counted;
//! - the stored queries whose template names a changed term;
//! - the named graphs holding data that uses a changed term, and which of
//!   those are frozen snapshots;
//! - a 0-100 risk score, with every input and its points shown.
//!
//! The verdict is `pass` (score at or under the threshold), `fail` (over it)
//! or `undetermined` (the plan could not be computed honestly: SHACL compiled
//! out, a SPARQL-based `sh:target`, a diff that is not Turtle, or a focus set
//! over [`MAX_FOCUS`]). An
//! undetermined plan never passes. Accept surfaces call [`gate`], which
//! refuses anything but `pass` unless the caller accepts the risk with a reason.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::proposal::{ProposalKind, ProposalStatus};
use crate::proposal_migration::{Issue, issue_key, refusals, target_queries};
use crate::store::Store;
use crate::types::Value;

#[path = "proposal_plan_scan.rs"]
mod scan;
#[path = "proposal_plan_terms.rs"]
mod terms;
#[cfg(test)]
#[path = "proposal_plan_tests.rs"]
mod tests;

#[cfg(test)]
use scan::names_prefixed;
pub use scan::render_text;
use scan::{graphs_using, stored_queries_naming};
pub use terms::{Terms, terms};

/// Default risk threshold: a plan scoring above it is refused at accept.
pub const DEFAULT_RISK_THRESHOLD: u8 = 40;
/// Environment variable that overrides [`DEFAULT_RISK_THRESHOLD`] for a deployment.
pub const RISK_THRESHOLD_ENV: &str = "QUIPU_PROPOSAL_RISK_THRESHOLD";
/// Above this many focus nodes the plan is `undetermined` rather than slow.
pub const MAX_FOCUS: usize = 50_000;
/// Newly failing issues listed in a plan (the counts are always exact).
const SAMPLE: usize = 20;

/// The threshold in force when the caller names none.
#[must_use]
pub fn default_threshold() -> u8 {
    std::env::var(RISK_THRESHOLD_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<u8>().ok())
        .map_or(DEFAULT_RISK_THRESHOLD, |t| t.min(100))
}

/// One input to the risk score.
#[derive(Debug, Serialize)]
pub struct RiskInput {
    /// What was counted.
    pub name: &'static str,
    /// The count.
    pub value: usize,
    /// Points it contributes.
    pub points: u8,
    /// Most points it can contribute.
    pub max: u8,
}

/// The score, its inputs, and the verdict against `threshold`.
#[derive(Debug, Serialize)]
pub struct Risk {
    /// Sum of the inputs' points, capped at 100.
    pub score: u8,
    /// A score above this fails.
    pub threshold: u8,
    /// The judgement.
    pub verdict: Verdict,
    /// Every input, with its points.
    pub inputs: Vec<RiskInput>,
}

/// `pass` 0, `fail` 1, `undetermined` 2: the CLI's exit status.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// Determined, at or under the threshold.
    Pass,
    /// Determined, over the threshold.
    Fail,
    /// Could not be computed honestly; never passes.
    Undetermined,
}

impl Verdict {
    /// 0 pass, 1 fail, 2 undetermined.
    #[must_use]
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Pass => 0,
            Self::Fail => 1,
            Self::Undetermined => 2,
        }
    }
}

/// Instances that would newly fail SHACL.
#[derive(Debug, Default, Serialize)]
pub struct NewlyFailing {
    /// Distinct focus nodes with at least one new violation.
    pub instances: usize,
    /// New violations in total.
    pub violations: usize,
    /// At most a few of them, in engine order.
    pub sample: Vec<Issue>,
}

/// The blast-radius report for one pending proposal.
#[derive(Debug, Serialize)]
pub struct ProposalPlan {
    /// The proposal planned.
    pub proposal_id: i64,
    /// Its kind.
    pub kind: ProposalKind,
    /// Its target (the shapes name for a shape proposal).
    pub target: String,
    /// Classes the proposed Turtle names.
    pub classes: BTreeSet<String>,
    /// Properties the proposed Turtle names.
    pub properties: BTreeSet<String>,
    /// Terms the current version of the target shapes names and the proposal drops.
    pub removed_terms: BTreeSet<String>,
    /// ROOT focus nodes the proposed shapes select.
    pub instances_in_scope: usize,
    /// Instances that would newly fail SHACL.
    pub newly_failing: NewlyFailing,
    /// Stored queries whose template names a changed term.
    pub stored_queries: Vec<String>,
    /// Named graphs (not frozen) holding data that uses a changed term.
    pub named_graphs: Vec<String>,
    /// Frozen snapshot graphs holding data that uses a changed term.
    pub snapshots: Vec<String>,
    /// Operations in the proposal's migration program.
    pub migration_ops: usize,
    /// Why the plan is `undetermined`; empty when it is not.
    pub undetermined: Vec<String>,
    /// The score and verdict.
    pub risk: Risk,
}

/// Plan proposal `id` against the current store. Writes nothing.
///
/// # Errors
/// The proposal does not exist or is not pending, its Turtle does not parse,
/// or a store read fails.
pub fn plan(store: &Store, id: i64, threshold: u8) -> Result<ProposalPlan> {
    let p = store
        .get_proposal(id)?
        .ok_or_else(|| Error::InvalidValue(format!("proposal {id} not found")))?;
    if p.status != ProposalStatus::Pending {
        return Err(Error::InvalidValue(format!(
            "proposal {id} is not pending; a plan only previews an acceptance"
        )));
    }
    // A diff that is not Turtle (the schema allows a JSON patch) cannot be
    // planned: that is `undetermined`, never an error that blocks every accept.
    let (proposed, unparsed) = match terms(&p.diff) {
        Ok(t) => (t, None),
        Err(e) => (
            Terms::default(),
            Some(format!(
                "the diff does not parse as Turtle ({e}); nothing in it can be planned"
            )),
        ),
    };
    let mut undetermined = proposed.undetermined.clone();
    undetermined.extend(unparsed.clone());
    let mut removed_terms = BTreeSet::new();
    let mut newly_failing = NewlyFailing::default();
    let mut in_scope = 0;
    if p.kind == ProposalKind::Shape && unparsed.is_none() {
        let current = store
            .list_shapes()?
            .into_iter()
            .find(|(n, _, _)| *n == p.target)
            .map(|(_, t, _)| t);
        if let Some(current) = &current {
            let old = terms(current)?;
            let new = proposed.all();
            removed_terms = old
                .all()
                .difference(&new)
                .map(|s| (*s).to_owned())
                .collect();
        }
        let focus = focus(store, &p.diff, &proposed)?;
        in_scope = focus.len();
        if !cfg!(feature = "shacl") {
            undetermined.push("SHACL is compiled out of this build".into());
        } else if in_scope > MAX_FOCUS {
            undetermined.push(format!(
                "{in_scope} focus nodes exceed the plan bound of {MAX_FOCUS}"
            ));
        } else if undetermined.is_empty() {
            newly_failing = newly_failing_under(store, &p.target, &p.diff, &focus)?;
        }
    }
    let changed: BTreeSet<String> = proposed
        .all()
        .into_iter()
        .map(str::to_owned)
        .chain(removed_terms.iter().cloned())
        .collect();
    let stored_queries = stored_queries_naming(store, &changed)?;
    let (named_graphs, snapshots) = graphs_using(store, &proposed, &removed_terms)?;
    let migration_ops = store.get_proposal_migration(id)?.map_or(0, |m| m.len());
    let risk = score(
        &[
            ("newly_failing_instances", newly_failing.instances),
            ("stored_queries", stored_queries.len()),
            ("named_graphs", named_graphs.len()),
            ("snapshots", snapshots.len()),
            ("removed_terms", removed_terms.len()),
            ("migration_ops", migration_ops),
        ],
        threshold,
        undetermined.is_empty(),
    );
    Ok(ProposalPlan {
        proposal_id: id,
        kind: p.kind,
        target: p.target,
        classes: proposed.classes,
        properties: proposed.properties,
        removed_terms,
        instances_in_scope: in_scope,
        newly_failing,
        stored_queries,
        named_graphs,
        snapshots,
        migration_ops,
        undetermined,
        risk,
    })
}

/// Points per input: `newly_failing_instances` is logarithmic (1 → 20,
/// 10 → 30, 100 → 40, 1000+ → 50); the others are linear and capped.
fn points(name: &str, n: usize) -> (u8, u8) {
    let linear = |per: usize, max: u8| {
        (
            u8::try_from((n * per).min(usize::from(max))).unwrap_or(max),
            max,
        )
    };
    match name {
        "newly_failing_instances" if n == 0 => (0, 50),
        "newly_failing_instances" => {
            let digits = u8::try_from(n.ilog10()).unwrap_or(3).min(3);
            (20 + 10 * digits, 50)
        }
        "stored_queries" => linear(5, 15),
        "named_graphs" => linear(2, 10),
        "snapshots" => linear(5, 10),
        "removed_terms" => linear(5, 10),
        _ => linear(2, 5),
    }
}

/// Sum the inputs' points (capped at 100) and judge against `threshold`.
fn score(inputs: &[(&'static str, usize)], threshold: u8, determined: bool) -> Risk {
    let inputs: Vec<RiskInput> = inputs
        .iter()
        .map(|&(name, value)| {
            let (points, max) = points(name, value);
            RiskInput {
                name,
                value,
                points,
                max,
            }
        })
        .collect();
    let total: u32 = inputs.iter().map(|i| u32::from(i.points)).sum();
    let score = u8::try_from(total.min(100)).unwrap_or(100);
    let verdict = match (determined, score > threshold) {
        (false, _) => Verdict::Undetermined,
        (true, true) => Verdict::Fail,
        (true, false) => Verdict::Pass,
    };
    Risk {
        score,
        threshold,
        verdict,
        inputs,
    }
}

/// The ROOT nodes the proposed shapes select, explicit and implicit targets.
fn focus(store: &Store, diff: &str, proposed: &Terms) -> Result<BTreeSet<String>> {
    let mut queries = target_queries(diff)?;
    queries.extend(
        proposed
            .implicit_targets
            .iter()
            .map(|c| format!("SELECT DISTINCT ?n WHERE {{ ?n a <{c}> }}")),
    );
    let mut focus = BTreeSet::new();
    for q in queries {
        for row in crate::sparql::query(store, &q)?.rows() {
            if let Some(Value::Ref(id)) = row.get("n") {
                focus.insert(store.resolve(*id)?);
            }
        }
    }
    Ok(focus)
}

/// Violations among `focus` under the post-acceptance shapes that the current
/// shapes do not already report.
fn newly_failing_under(
    store: &Store,
    target: &str,
    diff: &str,
    focus: &BTreeSet<String>,
) -> Result<NewlyFailing> {
    let rows = store.list_shapes()?;
    let current = (!rows.is_empty()).then(|| {
        rows.iter()
            .map(|(_, t, _)| t.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    });
    let after: Vec<&str> = rows
        .iter()
        .filter(|(n, _, _)| n != target)
        .map(|(_, t, _)| t.as_str())
        .chain(std::iter::once(diff))
        .collect();
    let before = refusals(store, current.as_deref(), focus, None)?;
    let after = refusals(store, Some(&after.join("\n\n")), focus, None)?;
    let seen: BTreeSet<_> = before.iter().map(issue_key).collect();
    let new: Vec<Issue> = after
        .into_iter()
        .filter(|i| !seen.contains(&issue_key(i)))
        .collect();
    let instances: BTreeSet<&str> = new.iter().map(|i| i.focus_node.as_str()).collect();
    Ok(NewlyFailing {
        instances: instances.len(),
        violations: new.len(),
        sample: new.iter().take(SAMPLE).cloned().collect(),
    })
}

/// The plan an accept surface gates on, or `None` when proposal `id` is not
/// pending, does not exist, or is a shape proposal whose Turtle does not
/// parse: the accept itself reports each of those as before.
///
/// # Errors
/// As [`plan`], for a pending proposal.
pub fn plan_for_accept(store: &Store, id: i64, threshold: u8) -> Result<Option<ProposalPlan>> {
    match store.get_proposal(id)? {
        // A shape proposal whose Turtle does not parse is refused by the
        // accept itself, with its own message; the plan would only mask it.
        Some(p) if p.kind == ProposalKind::Shape && terms(&p.diff).is_err() => Ok(None),
        Some(p) if p.status == ProposalStatus::Pending => plan(store, id, threshold).map(Some),
        _ => Ok(None),
    }
}

/// Refuse acceptance unless the plan passes, or the caller accepts the risk
/// with a non-empty reason. Returns the note to record on the decision.
///
/// # Errors
/// The plan fails or is undetermined and no reason was given.
pub fn gate(
    plan: &ProposalPlan,
    accept_risk: Option<&str>,
    note: Option<&str>,
) -> Result<Option<String>> {
    let reason = accept_risk.map(str::trim).filter(|r| !r.is_empty());
    match (plan.risk.verdict, reason) {
        (Verdict::Pass, _) => Ok(note.map(str::to_owned)),
        (verdict, Some(reason)) => {
            let at_risk = format!(
                "accepted at risk {}/{} ({verdict:?}): {reason}",
                plan.risk.score, plan.risk.threshold
            );
            Ok(Some(note.map_or_else(
                || at_risk.clone(),
                |n| format!("{n}; {at_risk}"),
            )))
        }
        (verdict, None) => Err(Error::InvalidValue(format!(
            "proposal {} refused, it remains pending: risk {}/{} is {verdict:?}{}; \
             review the plan, then accept with --accept-risk --reason <why> \
             (MCP: accept_risk) to proceed",
            plan.proposal_id,
            plan.risk.score,
            plan.risk.threshold,
            if plan.undetermined.is_empty() {
                String::new()
            } else {
                format!(" ({})", plan.undetermined.join("; "))
            },
        ))),
    }
}
