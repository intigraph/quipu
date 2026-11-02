//! Accepting a proposal with its migration, and the dry run before it.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value as JsonValue, json};

use super::plan::{MigrationPlan, Planned, plan};
use super::{MigrationOp, migration_source};
use crate::error::{Error, Result};
use crate::proposal::{Proposal, ProposalKind, ProposalStatus};
use crate::store::Store;
use crate::types::Value;
use crate::write_gate::{self, TriplesByGraph};

/// ROOT-only change sets keyed the way the write gate takes them.
fn root(nt: String) -> TriplesByGraph {
    if nt.is_empty() {
        BTreeMap::new()
    } else {
        BTreeMap::from([(String::new(), nt)])
    }
}

/// The reject half of `shapes`, which a migration's own write is held to.
fn reject_half(shapes: Option<&str>) -> Option<String> {
    #[cfg(feature = "shacl")]
    return shapes.map(|s| crate::shacl::split_shapes_by_policy(s).reject);
    #[cfg(not(feature = "shacl"))]
    shapes.map(str::to_owned)
}

/// Accept `proposal` and run its migration: the shape change, the decision,
/// and the migration's facts commit together or not at all.
///
/// The program is evaluated against the pre-acceptance state, then inside one
/// savepoint the proposed shapes are loaded, the proposal is marked accepted,
/// the change set is judged by the write gate under the NEW shapes, and it is
/// transacted as one transaction with source `migration:<id>`. Any refusal
/// rolls the savepoint back, leaving the proposal pending, the shapes
/// unchanged and no fact written; a SHACL refusal is then recorded.
pub(crate) fn accept_with_migration(
    store: &mut Store,
    proposal: &Proposal,
    program: &[MigrationOp],
    decided_by: &str,
    decided_at: &str,
    note: Option<&str>,
) -> Result<Option<i64>> {
    let planned = plan(store, program)?;
    let source = migration_source(proposal.id);
    let (asserted, retracted) = planned.n_triples(store)?;
    let (asserted, retracted) = (root(asserted), root(retracted));
    let mut refused = None;
    store
        .conn
        .execute_batch("SAVEPOINT quipu_proposal_migration")?;
    let result = (|| -> Result<Option<i64>> {
        // As without a migration: shapes load only where SHACL is compiled in.
        #[cfg(feature = "shacl")]
        if proposal.kind == ProposalKind::Shape {
            store.load_shapes(&proposal.target, &proposal.diff, decided_at)?;
        }
        store.mark_proposal_accepted(proposal.id, decided_by, decided_at, note)?;
        let shapes = store.get_combined_shapes()?;
        if let Some(refusal) = write_gate::check(store, shapes.as_deref(), &asserted, &retracted)? {
            let reason = refusal.reason.clone();
            refused = Some(refusal);
            return Err(Error::InvalidValue(reason));
        }
        let datums = planned.datums(store, decided_at)?;
        if datums.is_empty() {
            return Ok(None);
        }
        let reject = reject_half(shapes.as_deref());
        store
            .with_write_shapes(reject.as_deref(), |s| {
                s.transact(&datums, decided_at, Some(decided_by), Some(&source))
            })
            .map(Some)
    })();
    match result {
        Ok(tx) => {
            if let Err(error) =
                crate::store::release::release(&store.conn, "quipu_proposal_migration")
            {
                store.clear_pending_write_events();
                return Err(error.into());
            }
            Ok(tx)
        }
        Err(error) => {
            let rollback =
                crate::store::release::rollback_release(&store.conn, "quipu_proposal_migration");
            store.clear_pending_write_events();
            rollback?;
            let error = match refused {
                Some(refusal) => refusal.record(store, Some(decided_by), &source),
                None => error,
            };
            Err(Error::InvalidValue(format!(
                "proposal {} migration refused, proposal remains pending: {error}",
                proposal.id
            )))
        }
    }
}

/// A dry run's findings, as returned by `quipu_dry_run_proposal`.
pub type DryRun = JsonValue;

/// Report, without keeping any write, what accepting proposal `id` would do:
/// which existing facts the new shapes would refuse, what the migration would
/// change (per operation and as samples of at most `limit` facts), what would
/// still be refused afterwards, and whether the acceptance would go through.
///
/// The proposed shapes are staged in a savepoint that is always rolled back,
/// so the write gate judges the migration exactly as the acceptance will:
/// against the new shapes and the vocabulary they sanction.
pub fn dry_run(store: &Store, id: i64, limit: usize) -> Result<DryRun> {
    let proposal = store
        .get_proposal(id)?
        .ok_or_else(|| Error::InvalidValue(format!("proposal {id} not found")))?;
    if proposal.status != ProposalStatus::Pending {
        return Err(Error::InvalidValue(format!(
            "proposal {id} is not pending; a dry run only previews an acceptance"
        )));
    }
    let program = store.get_proposal_migration(id)?.unwrap_or_default();
    let planned = plan(store, &program)?;
    store
        .conn
        .execute_batch("SAVEPOINT quipu_proposal_dry_run")?;
    let report = (|| -> Result<JsonValue> {
        if proposal.kind == ProposalKind::Shape {
            store.load_shapes(&proposal.target, &proposal.diff, &crate::time::now_iso())?;
        }
        judge(store, &proposal, &planned, limit)
    })();
    let rollback = crate::store::release::rollback_release(&store.conn, "quipu_proposal_dry_run");
    // Clear even when the empty release itself is vetoed and settled.
    store.clear_pending_write_events();
    rollback?;
    report
}

/// The dry-run report, with the proposed shapes already in force.
fn judge(
    store: &Store,
    proposal: &Proposal,
    planned: &MigrationPlan,
    limit: usize,
) -> Result<JsonValue> {
    let shapes = store.get_combined_shapes()?;
    let focus = focus_nodes(store, proposal, planned)?;
    let before = refusals(store, shapes.as_deref(), &focus, None)?;
    let after = refusals(store, shapes.as_deref(), &focus, Some(planned))?;
    let (asserted, retracted) = planned.n_triples(store)?;
    let gate = match write_gate::check(store, shapes.as_deref(), &root(asserted), &root(retracted))
    {
        Ok(None) => None,
        Ok(Some(refusal)) => Some(refusal.reason),
        Err(e) => Some(e.to_string()),
    };
    // The transaction's own post-state check refuses what the migration makes
    // newly non-conforming; debt the data already had is admitted.
    let touched: BTreeSet<&str> = planned
        .retract
        .iter()
        .chain(&planned.assert)
        .map(|p| p.subject.as_str())
        .collect();
    let seen: BTreeSet<_> = before.iter().map(issue_key).collect();
    let introduced: Vec<_> = after
        .iter()
        .filter(|r| touched.contains(r.focus_node.as_str()) && !seen.contains(&issue_key(r)))
        .collect();
    let validate = store.shacl_config().validate_on_write;
    let gate = match (gate, introduced.is_empty() || !validate) {
        (Some(reason), _) => json!({ "would_accept": false, "reason": reason }),
        (None, false) => json!({
            "would_accept": false,
            "reason": format!(
                "the migration would introduce {} violation(s) under the new shapes",
                introduced.len()
            ),
            "introduced": introduced.iter().take(limit).collect::<Vec<_>>(),
        }),
        (None, true) => json!({ "would_accept": true }),
    };
    let sample = |facts: &[Planned]| -> Result<Vec<String>> {
        facts
            .iter()
            .take(limit)
            .map(|p| Ok(p.n_triple(store)?.trim_end().to_owned()))
            .collect()
    };
    let listed = |issues: &[Issue]| {
        json!({
            "violations": issues.len(),
            "results": issues.iter().take(limit).collect::<Vec<_>>(),
        })
    };
    Ok(json!({
        "proposal_id": proposal.id,
        "status": "pending",
        "source": migration_source(proposal.id),
        "refused_before_migration": listed(&before),
        "migration": {
            "ops": planned.ops,
            "retracted": planned.retract_count(),
            "asserted": planned.assert_count(),
            "retract_sample": sample(&planned.retract)?,
            "assert_sample": sample(&planned.assert)?,
        },
        "refused_after_migration": listed(&after),
        "gate": gate,
    }))
}

/// Subjects the proposed shapes target in the current ROOT graph, plus every
/// subject the migration touches.
fn focus_nodes(
    store: &Store,
    proposal: &Proposal,
    planned: &MigrationPlan,
) -> Result<BTreeSet<String>> {
    let mut focus = BTreeSet::new();
    if proposal.kind == ProposalKind::Shape {
        for query in target_queries(&proposal.diff)? {
            let result = crate::sparql::query(store, &query)?;
            for row in result.rows() {
                if let Some(Value::Ref(id)) = row.get("n") {
                    focus.insert(store.resolve(*id)?);
                }
            }
        }
    }
    for p in planned.retract.iter().chain(&planned.assert) {
        focus.insert(p.subject.clone());
    }
    Ok(focus)
}

/// One SELECT of `?n` per SHACL target declared in `shapes_turtle`.
pub(crate) fn target_queries(shapes_turtle: &str) -> Result<Vec<String>> {
    const SH: &str = "http://www.w3.org/ns/shacl#";
    let parser = oxrdfio::RdfParser::from_format(oxrdfio::RdfFormat::Turtle);
    let mut queries = Vec::new();
    for quad in parser.for_reader(shapes_turtle.as_bytes()) {
        let quad = quad.map_err(|e| Error::InvalidValue(format!("proposed shapes: {e}")))?;
        let oxrdf::Term::NamedNode(object) = &quad.object else {
            continue;
        };
        let o = object.as_str();
        match quad.predicate.as_str().strip_prefix(SH) {
            Some("targetClass") => {
                queries.push(format!("SELECT DISTINCT ?n WHERE {{ ?n a <{o}> }}"));
            }
            Some("targetSubjectsOf") => {
                queries.push(format!("SELECT DISTINCT ?n WHERE {{ ?n <{o}> ?x }}"));
            }
            Some("targetObjectsOf") => queries.push(format!(
                "SELECT DISTINCT ?n WHERE {{ ?x <{o}> ?n FILTER(isIRI(?n)) }}"
            )),
            Some("targetNode") => queries.push(format!("SELECT ?n WHERE {{ BIND(<{o}> AS ?n) }}")),
            _ => {}
        }
    }
    Ok(queries)
}

/// The current ROOT facts of `focus`, with `planned` applied when given.
#[cfg(feature = "shacl")]
fn focus_triples(
    store: &Store,
    focus: &BTreeSet<String>,
    planned: Option<&MigrationPlan>,
) -> Result<String> {
    let mut out = String::new();
    let retract: BTreeSet<String> = match planned {
        Some(p) => p
            .retract
            .iter()
            .map(|f| f.n_triple(store))
            .collect::<Result<_>>()?,
        None => BTreeSet::new(),
    };
    for subject in focus {
        let Some(entity) = store.lookup(subject)? else {
            continue;
        };
        for fact in store.entity_facts_in_graph(entity, crate::schema::ROOT_GRAPH)? {
            let object = match fact.value {
                Value::Ref(id) => super::plan::Obj::Iri(store.resolve(id)?),
                other => super::plan::Obj::Lit(other),
            };
            let line = Planned {
                subject: subject.clone(),
                predicate: store.resolve(fact.attribute)?,
                object,
            }
            .n_triple(store)?;
            if !retract.contains(&line) {
                out.push_str(&line);
            }
        }
    }
    if let Some(p) = planned {
        for f in &p.assert {
            out.push_str(&f.n_triple(store)?);
        }
    }
    Ok(out)
}

#[cfg(feature = "shacl")]
pub(crate) type Issue = crate::shacl::ValidationIssue;

/// A violation-free stand-in when SHACL is compiled out.
#[cfg(not(feature = "shacl"))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct Issue {
    /// The focus node.
    pub focus_node: String,
    /// The property path, if any.
    pub path: Option<String>,
    /// The SHACL component.
    pub component: String,
}

pub(crate) fn issue_key(issue: &Issue) -> (String, Option<String>, String) {
    (
        issue.focus_node.clone(),
        issue.path.clone(),
        issue.component.clone(),
    )
}

/// Every violation the shapes find among `focus`, before or after `planned`.
///
/// Judged on the focus nodes' whole facts with the types of the nodes they
/// reference, and WITHOUT write-time debt admission: this answers "what does
/// the new schema refuse", not "would this write be admitted".
#[cfg(feature = "shacl")]
pub(crate) fn refusals(
    store: &Store,
    shapes: Option<&str>,
    focus: &BTreeSet<String>,
    planned: Option<&MigrationPlan>,
) -> Result<Vec<Issue>> {
    let data = focus_triples(store, focus, planned)?;
    let (Some(shapes), false) = (shapes, data.is_empty()) else {
        return Ok(Vec::new());
    };
    let referenced: BTreeSet<String> = data
        .lines()
        .filter_map(|line| {
            let object = line.trim_end_matches(" .").rsplit(' ').next()?;
            object
                .strip_prefix('<')
                .and_then(|o| o.strip_suffix('>'))
                .map(str::to_owned)
        })
        .filter(|iri| !focus.contains(iri))
        .collect();
    let context = crate::shacl_context::store_type_context_in_graph(
        store,
        &referenced,
        crate::schema::ROOT_GRAPH,
    )?;
    let reject = crate::shacl::split_shapes_by_policy(shapes).reject;
    let feedback = crate::shacl::validate_shapes(&reject, &format!("{data}{context}"))?;
    Ok(feedback
        .results
        .into_iter()
        .filter(|r| r.severity.eq_ignore_ascii_case("violation"))
        .collect())
}

#[cfg(not(feature = "shacl"))]
pub(crate) fn refusals(
    _store: &Store,
    _shapes: Option<&str>,
    _focus: &BTreeSet<String>,
    _planned: Option<&MigrationPlan>,
) -> Result<Vec<Issue>> {
    Ok(Vec::new())
}
