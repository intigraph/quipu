//! Snapshot promotion must not undo a later local decision to retract a fact.
//!
//! A foreign `tx_anchor` is not comparable with the receiving store's transaction
//! IDs. Conservatively retain ROOT's exact-fact retractions until the operator
//! explicitly reasserts that fact locally. Source graph membership remains the
//! unmodified evidence of what the pack supplied.

use std::collections::HashSet;

use crate::Error;
use crate::error::Result;
use crate::share_import::{
    PromoteImportRequest, PromoteImportResult, provenance_source, staging_graph,
};
use crate::store::{Datum, Store};
use crate::types::{Fact, Op};

pub(crate) fn promote_snapshot(
    store: &mut Store,
    graph: i64,
    timestamp: &str,
    actor: Option<&str>,
    source: &str,
) -> Result<(i64, usize, usize)> {
    store.ensure_ownership_writable()?;
    store
        .conn
        .execute_batch("SAVEPOINT quipu_snapshot_promotion")?;
    let result = (|| {
        let facts = store.current_facts_in_graph(graph)?;
        let (datums, suppressed) = without_retracted_root_facts(store, &facts, timestamp)?;
        let tx = store.transact(&datums, timestamp, actor, Some(source))?;
        Ok((tx, datums.len(), suppressed))
    })();
    match result {
        Ok(value) => {
            crate::store::release::release(&store.conn, "quipu_snapshot_promotion")?;
            Ok(value)
        }
        Err(error) => {
            crate::store::release::rollback_release(&store.conn, "quipu_snapshot_promotion")?;
            store.read_model.borrow_mut().clear();
            Err(error)
        }
    }
}

/// Explicitly copy an eligible staging graph into ROOT, or move its subjects
/// into a target named graph (aegis-27l).
pub(crate) fn promote_inner(
    store: &mut Store,
    request: &PromoteImportRequest,
    timestamp: &str,
    authenticated_actor: Option<&str>,
) -> Result<PromoteImportResult> {
    let graph_iri = staging_graph(&request.share_id, false)?;
    let graph = store.lookup(&graph_iri)?.ok_or_else(|| {
        Error::InvalidValue(format!(
            "no eligible staged import for {}",
            request.share_id
        ))
    })?;
    if store.graph_class(graph)?.as_deref() != Some("committed") {
        return Err(Error::InvalidValue(format!(
            "staging graph is not committed: {graph_iri}"
        )));
    }
    crate::time::check_request_deadline()?;
    let source = provenance_source(
        "share-promotion",
        &request.share_id,
        request.actor.as_deref(),
    );
    let (tx_id, count, suppressed, moved) = match request.target_graph.as_deref() {
        None if request.dry_run => {
            return Err(Error::InvalidValue(
                "dry_run requires target_graph: it previews a subject move".into(),
            ));
        }
        None if request.retire_from.is_some() => {
            return Err(Error::InvalidValue(
                "retire_from requires target_graph: a ROOT promotion moves nothing".into(),
            ));
        }
        None => {
            let (tx, count, suppressed) = crate::share_promotion::promote_snapshot(
                store,
                graph,
                timestamp,
                authenticated_actor,
                &source,
            )?;
            (tx, count, suppressed, None)
        }
        Some(target) => {
            let moved = crate::share_promotion::promote_subjects_into_graph(
                store,
                graph,
                target,
                request,
                timestamp,
                authenticated_actor,
                &source,
            )?;
            (
                moved.tx_id,
                moved.asserted,
                0,
                Some((target.to_string(), moved)),
            )
        }
    };
    // AFTER the data: if installing fails, the hold survives and promoting
    // again retries it.
    let queries = if request.dry_run {
        None
    } else {
        crate::share_queries::release(store, &request.share_id, timestamp)?
    };
    let unchanged = moved.as_ref().is_some_and(|(_, m)| m.tx_id == 0);
    let warning = moved
        .as_ref()
        .filter(|(_, m)| request.dry_run && m.replaced > 0)
        .map(|(target, m)| {
            format!(
                "would retract {} facts of the moved subjects in {target} that the share does not carry",
                m.replaced
            )
        });
    let outcome = match (request.dry_run, unchanged) {
        (true, _) => "dry_run",
        (false, true) => "unchanged",
        (false, false) => "promoted",
    };
    Ok(PromoteImportResult {
        outcome: outcome.into(),
        share_id: request.share_id.clone(),
        staging_graph: graph_iri,
        tx_id,
        triples: count,
        suppressed_retractions: suppressed,
        queries,
        retired: moved
            .as_ref()
            .and_then(|(_, m)| m.retire_from.as_ref().map(|_| m.retired)),
        replaced: moved.as_ref().map(|(_, m)| m.replaced),
        target_graph: moved.map(|(target, _)| target),
        warning,
    })
}

/// What a subject-scoped promotion changed.
#[derive(Debug, Clone, Default)]
pub(crate) struct SubjectMove {
    /// 0 when nothing needed to change (an idempotent repeat).
    pub tx_id: i64,
    pub asserted: usize,
    pub replaced: usize,
    pub retired: usize,
    pub retire_from: Option<String>,
}

/// Promote a staged share into the named graph `target`, as a MOVE of exactly
/// the share's subjects (aegis-27l: rb3 B3, tg5 F5):
///
/// - in `target`, each staged subject ends up holding exactly its staged facts;
///   facts of every other subject in `target` are untouched (this is not
///   [`crate::project_graph::promote_into_graph`]'s whole-graph diff, which
///   retracts whatever the share does not mention);
/// - with `retire_from`, the same subjects' current facts are retracted from that
///   graph; its other subjects are untouched.
///
/// One transaction per graph, both inside the caller's savepoint, so a failure
/// or an expired request deadline rolls back the move whole. The writer is
/// bounded the way /import is (aegis-oa5): `check_request_deadline` between the
/// steps, never a partial state to resume. A share too big for one budget is
/// split by the caller into smaller shares.
pub(crate) fn promote_subjects_into_graph(
    store: &mut Store,
    staging: i64,
    target: &str,
    request: &PromoteImportRequest,
    timestamp: &str,
    actor: Option<&str>,
    source: &str,
) -> Result<SubjectMove> {
    let (retire_from, dry_run) = (request.retire_from.as_deref(), request.dry_run);
    if retire_from == Some(target) {
        return Err(crate::Error::InvalidValue(
            "retire_from must differ from target_graph: a graph cannot be moved into itself".into(),
        ));
    }
    let wanted = store.current_facts_in_graph(staging)?;
    let subjects: HashSet<i64> = wanted.iter().map(|f| f.entity).collect();
    let key = |f: &Fact| (f.entity, f.attribute, f.value.to_bytes());
    let datum = |f: &Fact, op| Datum {
        entity: f.entity,
        attribute: f.attribute,
        value: f.value.clone(),
        valid_from: timestamp.to_string(),
        valid_to: None,
        op,
    };
    crate::time::check_request_deadline()?;
    // A dry run only looks: it never creates the target graph. It still
    // refuses a target the real create refuses (a graph of another class).
    let g = if dry_run {
        let g = store.lookup(target)?;
        if let Some(g) = g {
            store.committed_graph_registered(g)?;
        }
        g
    } else {
        Some(store.graph_create(target)?)
    };
    let have: Vec<Fact> = match g {
        Some(g) => store
            .current_facts_in_graph(g)?
            .into_iter()
            .filter(|f| subjects.contains(&f.entity))
            .collect(),
        None => Vec::new(),
    };
    let wanted_keys: HashSet<_> = wanted.iter().map(key).collect();
    let have_keys: HashSet<_> = have.iter().map(key).collect();
    let mut changes: Vec<Datum> = have
        .iter()
        .filter(|f| !wanted_keys.contains(&key(f)))
        .map(|f| datum(f, Op::Retract))
        .collect();
    let replaced = changes.len();
    changes.extend(
        wanted
            .iter()
            .filter(|f| !have_keys.contains(&key(f)))
            .map(|f| datum(f, Op::Assert)),
    );
    let asserted = changes.len() - replaced;
    let mut moved = SubjectMove {
        asserted,
        replaced,
        retire_from: retire_from.map(String::from),
        ..SubjectMove::default()
    };
    if dry_run {
        if let Some(b) = retire_from
            .map(|from| store.lookup(from))
            .transpose()?
            .flatten()
        {
            moved.retired = store
                .current_facts_in_graph(b)?
                .iter()
                .filter(|f| subjects.contains(&f.entity))
                .count();
        }
        return Ok(moved);
    }
    let Some(g) = g else {
        return Err(crate::Error::InvalidValue(format!(
            "target graph missing: {target}"
        )));
    };
    crate::time::check_request_deadline()?;
    if !changes.is_empty() {
        moved.tx_id = store.transact_to_graph(&changes, timestamp, actor, Some(source), g)?;
    }
    #[cfg(test)]
    crate::share_import::test_hooks::at(crate::share_import::test_hooks::Boundary::TargetWritten);
    if let Some(from) = retire_from {
        // An unknown source graph holds nothing to retire; never create one.
        if let Some(b) = store.lookup(from)? {
            crate::time::check_request_deadline()?;
            let retire: Vec<Datum> = store
                .current_facts_in_graph(b)?
                .iter()
                .filter(|f| subjects.contains(&f.entity))
                .map(|f| datum(f, Op::Retract))
                .collect();
            moved.retired = retire.len();
            if !retire.is_empty() {
                let tx = store.transact_to_graph(&retire, timestamp, actor, Some(source), b)?;
                moved.tx_id = moved.tx_id.max(tx);
            }
        }
    }
    // No check here: the caller's final check, before the savepoint RELEASE,
    // covers this retirement, the query release and the review record.
    #[cfg(test)]
    crate::share_import::test_hooks::at(crate::share_import::test_hooks::Boundary::Retired);
    Ok(moved)
}

fn without_retracted_root_facts(
    store: &Store,
    facts: &[Fact],
    timestamp: &str,
) -> Result<(Vec<Datum>, usize)> {
    let mut statement = store.conn.prepare(
        "SELECT DISTINCT e, a, v FROM facts WHERE g=0 \
         AND (op=0 OR retracted_tx IS NOT NULL)",
    )?;
    let mut retracted: HashSet<(i64, i64, Vec<u8>)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<std::result::Result<_, _>>()?;
    // A deliberate local reassertion supersedes a historical retraction. An
    // unrelated predicate, value, or graph never suppresses this fact.
    for fact in store.current_facts()? {
        retracted.remove(&(fact.entity, fact.attribute, fact.value.to_bytes()));
    }
    let datums: Vec<_> = facts
        .iter()
        .filter(|fact| !retracted.contains(&(fact.entity, fact.attribute, fact.value.to_bytes())))
        .map(|fact| Datum {
            entity: fact.entity,
            attribute: fact.attribute,
            value: fact.value.clone(),
            valid_from: timestamp.to_string(),
            valid_to: None,
            op: Op::Assert,
        })
        .collect();
    let suppressed = facts.len() - datums.len();
    Ok((datums, suppressed))
}

#[cfg(test)]
#[path = "share_promotion_tests.rs"]
mod tests;
