//! Resolve a fork promotion that stopped on conflicts (aegis-k7l).
//!
//! `quipu fork promote` holds every conflicting slot and writes nothing
//! (`ForkPromotion::Conflicts`). This module finishes such a promotion with
//! the same emit / propose / decide / apply split the share merge uses
//! (`crate::share_merge_decisions`), and the same row: a [`DecisionRecord`]
//! with each side's provenance, its `kind` and `rule`, a mechanical
//! `proposal` and the operator's `decision`.
//!
//! 1. [`emit`] writes a fork decisions file, bound to the fork, its base
//!    transaction, and each reviewed slot on BOTH sides (`root_slot_hash`,
//!    `fork_slot_hash`). Nothing is written to the store.
//! 2. [`propose`] fills each row's `proposal`; it never decides.
//! 3. [`apply`] promotes with every decided slot, through the same SHACL and
//!    write gates as a clean promote, in one transaction whose source names the
//!    reviewer and the file's hash. It refuses, writing nothing, on an
//!    undecided row, a stale file, too many values, or a value that is not an
//!    RDF term. Applying the same file again reports `already-applied`.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::error::{Error, Result};
use crate::merge3::DecisionRecord;
use crate::share::sha256;
use crate::share_merge_decisions::{AppliedRow, Decision, LocalFact, Proposal, Side, union};
use crate::share_merge_decisions_view::{ConflictKind, kind, rule};
use crate::store::Store;
use crate::store::forks::{ForkPromotion, ForkResolutions};
use crate::types::Value;

/// Schema of a fork decisions file.
pub const FORK_DECISIONS_SCHEMA: &str = "https://github.com/scbrown/quipu/merge-decisions/fork-v1";

const RDFS_LABEL: &str = "http://www.w3.org/2000/01/rdf-schema#label";

/// A fork decisions file: the conflicts of promoting one fork.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForkDecisionFile {
    pub schema: String,
    /// The fork's registry name.
    pub fork: String,
    /// The ROOT transaction the fork was taken at (the merge base).
    pub fork_tx: i64,
    /// `rdfs:label` per IRI the rows mention, for display.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    pub rows: Vec<ForkDecisionRow>,
}

/// One conflicting slot of a fork promotion and, once decided, its resolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForkDecisionRow {
    pub id: String,
    #[serde(flatten)]
    pub record: DecisionRecord,
    pub provenance: ForkProvenance,
    /// ROOT's current values and their provenance for this slot.
    pub root_slot_hash: String,
    /// The fork's current values and their provenance for this slot.
    pub fork_slot_hash: String,
    /// Filled by [`propose`]; advisory only, never applied.
    pub proposal: Option<Proposal>,
    /// Filled by the operator; [`apply`] refuses while any row lacks one.
    pub decision: Option<Decision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ConflictKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    /// Who decided, as the editing tool claims it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<String>,
}

/// Where each side's values came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForkProvenance {
    /// ROOT's current facts for the slot.
    pub ours: Vec<LocalFact>,
    /// The fork's current facts for the slot.
    pub theirs: Vec<LocalFact>,
    /// The base is ROOT as of this transaction.
    pub base_tx: i64,
}

/// Result of [`apply`].
#[derive(Debug, Clone, Serialize)]
pub struct ForkDecidedPromotion {
    /// `promoted`, `dry-run` or `already-applied`.
    pub outcome: String,
    pub fork: String,
    /// The promotion's transaction (`0` for an empty delta or a dry run).
    pub tx_id: Option<i64>,
    pub asserted: usize,
    pub retracted: usize,
    pub reviewer: String,
    pub decisions_sha256: String,
    pub applied: Vec<AppliedRow>,
}

/// The term id of a rendered subject or predicate, without interning.
fn term_id(store: &Store, term: &str) -> Result<Option<i64>> {
    let iri = term
        .strip_prefix('<')
        .and_then(|t| t.strip_suffix('>'))
        .unwrap_or(term);
    store.lookup(iri)
}

fn slot_ids(store: &Store, record: &DecisionRecord) -> Result<Option<(i64, i64)>> {
    Ok(
        match (
            term_id(store, &record.subject)?,
            store.lookup(&record.predicate)?,
        ) {
            (Some(e), Some(a)) => Some((e, a)),
            _ => None,
        },
    )
}

/// Current facts in graph `g` for the record's slot, with their transactions.
fn slot_facts(store: &Store, g: i64, record: &DecisionRecord) -> Result<Vec<LocalFact>> {
    let Some((e, a)) = slot_ids(store, record)? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for fact in store.entity_facts_in_graph(e, g)? {
        if fact.attribute != a {
            continue;
        }
        let tx = store.get_transaction(fact.tx)?;
        out.push(LocalFact {
            value: crate::rdf::value_to_term(store, &fact.value)?.to_string(),
            valid_from: fact.valid_from,
            tx: fact.tx,
            actor: tx.as_ref().and_then(|t| t.actor.clone()),
            source: tx.and_then(|t| t.source),
        });
    }
    out.sort_by(|x, y| x.value.cmp(&y.value).then(x.tx.cmp(&y.tx)));
    Ok(out)
}

/// Bind one side of a reviewed slot: its values, their provenance, and the
/// slot's latest mutation in that graph (so a change-and-restore is stale).
fn slot_hash(store: &Store, g: i64, record: &DecisionRecord, values: &[String]) -> Result<String> {
    let last_tx = match slot_ids(store, record)? {
        Some((e, a)) => store
            .attribute_history_in_graph(e, a, g)?
            .iter()
            .map(|f| f.tx)
            .max(),
        None => None,
    };
    let bytes = serde_json::to_vec(&(
        "quipu-fork-merge-slot/v1",
        g,
        &record.subject,
        &record.predicate,
        values,
        slot_facts(store, g, record)?,
        last_tx,
    ))
    .map_err(|e| Error::Serialization(e.to_string()))?;
    Ok(sha256(&bytes))
}

/// One `rdfs:label` per IRI the rows mention, from ROOT or the fork:
/// untagged first, then `@en`, then the lexically first.
fn labels(
    store: &Store,
    graphs: &[i64],
    records: &[DecisionRecord],
) -> Result<BTreeMap<String, String>> {
    let Some(label) = store.lookup(RDFS_LABEL)? else {
        return Ok(BTreeMap::new());
    };
    let strip = |t: &str| {
        t.strip_prefix('<')
            .and_then(|s| s.strip_suffix('>'))
            .map(str::to_owned)
    };
    let mut iris = BTreeSet::new();
    for r in records {
        iris.insert(r.predicate.clone());
        iris.extend(strip(&r.subject));
        for v in r.base.iter().chain(&r.ours).chain(&r.theirs) {
            iris.extend(strip(v));
        }
    }
    let mut out = BTreeMap::new();
    for iri in iris {
        let Some(id) = store.lookup(&iri)? else {
            continue;
        };
        let mut found: Vec<(u8, String)> = Vec::new();
        for &g in graphs {
            for f in store.entity_facts_in_graph(id, g)? {
                if f.attribute != label {
                    continue;
                }
                match &f.value {
                    Value::Str(s) => found.push((0, s.clone())),
                    Value::Lang { lexical, lang } => {
                        found.push((if lang == "en" { 1 } else { 2 }, lexical.clone()));
                    }
                    _ => {}
                }
            }
        }
        found.sort();
        if let Some((_, l)) = found.into_iter().next() {
            out.insert(iri, l);
        }
    }
    Ok(out)
}

/// Compute the decisions file for promoting fork `name`. Writes nothing.
///
/// # Errors
/// An unknown or closed fork, an unusable base, or a store read error.
pub fn emit(store: &Store, name: &str) -> Result<ForkDecisionFile> {
    let (fork, conflicts) = store.fork_conflicts(name)?;
    let labels = labels(store, &[crate::schema::ROOT_GRAPH, fork.g], &conflicts)?;
    let mut rows = Vec::with_capacity(conflicts.len());
    for (n, record) in conflicts.into_iter().enumerate() {
        rows.push(ForkDecisionRow {
            id: format!("d{}", n + 1),
            provenance: ForkProvenance {
                ours: slot_facts(store, crate::schema::ROOT_GRAPH, &record)?,
                theirs: slot_facts(store, fork.g, &record)?,
                base_tx: fork.fork_tx,
            },
            root_slot_hash: slot_hash(store, crate::schema::ROOT_GRAPH, &record, &record.ours)?,
            fork_slot_hash: slot_hash(store, fork.g, &record, &record.theirs)?,
            kind: Some(kind(&record)),
            rule: Some(rule(&record, &labels)),
            record,
            proposal: None,
            decision: None,
            decided_by: None,
            decided_at: None,
        });
    }
    Ok(ForkDecisionFile {
        schema: FORK_DECISIONS_SCHEMA.into(),
        fork: fork.name,
        fork_tx: fork.fork_tx,
        labels,
        rows,
    })
}

/// Fill each row's `proposal` from mechanical evidence. Never decides.
#[must_use]
pub fn propose(mut file: ForkDecisionFile) -> ForkDecisionFile {
    for row in &mut file.rows {
        row.proposal = Some(proposal_for(row));
    }
    file
}

fn proposal_for(row: &ForkDecisionRow) -> Proposal {
    let r = &row.record;
    let newest = |facts: &[LocalFact]| facts.iter().map(|f| f.tx).max();
    let (ours, theirs) = (newest(&row.provenance.ours), newest(&row.provenance.theirs));
    let mut evidence = Vec::new();
    let choose = if r.ours.is_empty() {
        evidence.push(
            "ROOT deleted the slot and the fork replaced it; proposing the replacement".into(),
        );
        Side::Theirs
    } else if r.theirs.is_empty() {
        evidence
            .push("the fork deleted the slot and ROOT replaced it; proposing ROOT's value".into());
        Side::Ours
    } else {
        match (ours, theirs) {
            (Some(o), Some(t)) if o > t => {
                evidence.push(format!("ours is newer: ROOT tx {o} > fork tx {t}"));
                Side::Ours
            }
            (o, t) => {
                evidence.push(format!(
                    "theirs is newer: fork tx {} >= ROOT tx {}",
                    t.map_or("?".into(), |t| t.to_string()),
                    o.map_or("?".into(), |o| o.to_string())
                ));
                Side::Theirs
            }
        }
    };
    evidence.push("mechanical proposal: the operator decides".into());
    Proposal { choose, evidence }
}

const FILE_FIELDS: &[&str] = &["schema", "fork", "fork_tx", "labels", "rows"];
const ROW_FIELDS: &[&str] = &[
    "id",
    "subject",
    "predicate",
    "max_count",
    "base",
    "ours",
    "theirs",
    "provenance",
    "root_slot_hash",
    "fork_slot_hash",
    "proposal",
    "decision",
    "kind",
    "rule",
    "decided_by",
    "decided_at",
];

/// Refuse a field the schema does not define (apply would drop it while the
/// file's hash still covered it).
fn check_fields(bytes: &[u8]) -> Result<()> {
    let value: JsonValue = serde_json::from_slice(bytes)
        .map_err(|e| Error::InvalidValue(format!("decisions file is not JSON: {e}")))?;
    let unknown = |where_: &str, o: &serde_json::Map<String, JsonValue>, allowed: &[&str]| {
        o.keys().find(|k| !allowed.contains(&k.as_str())).map(|k| {
            Error::InvalidValue(format!(
                "unknown field '{k}' {where_}: the fork merge-decisions schema does not define it, and apply would drop it. Nothing was written"
            ))
        })
    };
    let file = value
        .as_object()
        .ok_or_else(|| Error::InvalidValue("decisions file is not an object".into()))?;
    if let Some(e) = unknown("in the file", file, FILE_FIELDS) {
        return Err(e);
    }
    for row in file
        .get("rows")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
    {
        let o = row
            .as_object()
            .ok_or_else(|| Error::InvalidValue("a row is not an object".into()))?;
        let id = o.get("id").and_then(JsonValue::as_str).unwrap_or("?");
        if let Some(e) = unknown(&format!("in row {id}"), o, ROW_FIELDS) {
            return Err(e);
        }
    }
    Ok(())
}

/// The values a decided row resolves to, or why it cannot.
pub(crate) fn decided_values(
    id: &str,
    record: &DecisionRecord,
    decision: Option<&Decision>,
) -> Result<Vec<String>> {
    let values = match decision {
        None => {
            return Err(Error::InvalidValue(format!(
                "row {id} ({} {}) is undecided; nothing was written",
                record.subject, record.predicate
            )));
        }
        Some(Decision::Choose { choose: Side::Ours }) => record.ours.clone(),
        Some(Decision::Choose {
            choose: Side::Theirs,
        }) => record.theirs.clone(),
        Some(Decision::Choose { choose: Side::Base }) => record.base.clone(),
        Some(Decision::Choose { choose: Side::Both }) => union(&record.ours, &record.theirs),
        Some(Decision::Values { values }) => values.clone(),
    };
    if values.len() > record.max_count {
        return Err(Error::InvalidValue(format!(
            "row {id} decides {} values for {} {}, above sh:maxCount {}; nothing was written",
            values.len(),
            record.subject,
            record.predicate,
            record.max_count
        )));
    }
    for v in &values {
        parse_term(v)
            .map_err(|e| Error::InvalidValue(format!("row {id}: {e}; nothing was written")))?;
    }
    Ok(values)
}

/// One N-Triples term, or why it is not.
pub(crate) fn parse_term(value: &str) -> Result<oxrdf::Term> {
    oxrdf::Term::from_str(value.trim())
        .map_err(|e| Error::InvalidValue(format!("value {value:?} is not one RDF term: {e}")))
}

/// Promote fork `name` with every row of `file` decided.
///
/// # Errors
/// [`Error::InvalidValue`] on an undecided row, a stale or foreign file, too
/// many values, a value that is not an RDF term, or a SHACL refusal; in every
/// such case nothing is written. Store failures otherwise.
#[allow(clippy::too_many_arguments)]
pub fn apply(
    store: &mut Store,
    name: &str,
    file: &ForkDecisionFile,
    file_bytes: &[u8],
    reviewer: &str,
    timestamp: &str,
    actor: Option<&str>,
    dry_run: bool,
) -> Result<ForkDecidedPromotion> {
    if reviewer.trim().is_empty() {
        return Err(Error::InvalidValue("a reviewer is required".into()));
    }
    if file.schema != FORK_DECISIONS_SCHEMA {
        return Err(Error::InvalidValue(format!(
            "not a fork merge-decisions file: schema {:?}",
            file.schema
        )));
    }
    check_fields(file_bytes)?;
    if file.fork != name {
        return Err(Error::InvalidValue(format!(
            "these decisions are for fork '{}', not '{name}'; nothing was written",
            file.fork
        )));
    }
    let decisions_sha256 = sha256(file_bytes);
    let applied_rows = || {
        file.rows
            .iter()
            .map(|row| {
                Ok(AppliedRow {
                    id: row.id.clone(),
                    subject: row.record.subject.clone(),
                    predicate: row.record.predicate.clone(),
                    values: decided_values(&row.id, &row.record, row.decision.as_ref())?,
                    decided_by: row.decided_by.clone(),
                    decided_at: row.decided_at.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()
    };
    if let Some(tx) = store.fork_promoted_by(name, &decisions_sha256)? {
        return Ok(ForkDecidedPromotion {
            outcome: "already-applied".into(),
            fork: name.into(),
            tx_id: Some(tx),
            asserted: 0,
            retracted: 0,
            reviewer: reviewer.into(),
            decisions_sha256,
            applied: Vec::new(),
        });
    }
    let stale = |what: &str| {
        Error::InvalidValue(format!(
            "stale decisions: {what} changed since they were emitted (quipu fork promote --emit-decisions); emit again and re-decide. Nothing was written"
        ))
    };
    let (fork, conflicts) = store.fork_conflicts(name)?;
    if fork.fork_tx != file.fork_tx {
        return Err(stale("the fork's base"));
    }
    let same = conflicts.len() == file.rows.len()
        && conflicts
            .iter()
            .all(|c| file.rows.iter().any(|row| &row.record == c));
    if !same {
        return Err(stale("the set of conflicts"));
    }
    for row in &file.rows {
        let r = &row.record;
        if row.root_slot_hash != slot_hash(store, crate::schema::ROOT_GRAPH, r, &r.ours)? {
            return Err(stale(&format!("ROOT's side of row {}", row.id)));
        }
        if row.fork_slot_hash != slot_hash(store, fork.g, r, &r.theirs)? {
            return Err(stale(&format!("the fork's side of row {}", row.id)));
        }
    }
    let applied = applied_rows()?;
    let mut resolutions: ForkResolutions = ForkResolutions::new();
    for row in &applied {
        let record = &file
            .rows
            .iter()
            .find(|r| r.id == row.id)
            .expect("row from file")
            .record;
        let ids =
            slot_ids(store, record)?.ok_or_else(|| stale(&format!("row {}'s slot", row.id)))?;
        let mut values = Vec::with_capacity(row.values.len());
        for v in &row.values {
            values.push(crate::rdf::term_to_value(store, &parse_term(v)?)?);
        }
        resolutions.insert(ids, values);
    }
    match store.fork_promote_resolved(
        name,
        &resolutions,
        &decisions_sha256,
        reviewer,
        timestamp,
        actor,
        dry_run,
    )? {
        ForkPromotion::Promoted {
            tx,
            asserted,
            retracted,
        } => Ok(ForkDecidedPromotion {
            outcome: if dry_run { "dry-run" } else { "promoted" }.into(),
            fork: name.into(),
            tx_id: (!dry_run).then_some(tx),
            asserted,
            retracted,
            reviewer: reviewer.into(),
            decisions_sha256,
            applied,
        }),
        #[cfg(feature = "shacl")]
        ForkPromotion::Refused(feedback) => {
            let first = feedback
                .results
                .first()
                .map(|i| {
                    format!(
                        ": {} on {}",
                        i.message.as_deref().unwrap_or("constraint violated"),
                        i.focus_node
                    )
                })
                .unwrap_or_default();
            Err(Error::InvalidValue(format!(
                "promotion REFUSED by SHACL with these decisions: {} violation(s){first}; nothing was written",
                feedback.violations
            )))
        }
        ForkPromotion::Conflicts(_) => Err(stale("the set of conflicts")),
    }
}

#[cfg(test)]
#[path = "fork_merge_decisions_tests.rs"]
mod tests;
