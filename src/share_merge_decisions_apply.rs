//! Validate a decision file and persist the common core plan.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn plan_pair(
    store: &mut Store,
    pair: SharePair,
    file: &DecisionFile,
    file_bytes: &[u8],
    reviewer: &str,
    timestamp: &str,
    actor: Option<&str>,
    dry_run: bool,
) -> Result<DecidedMerge> {
    if dry_run {
        plan_pair_inner(
            store, pair, file, file_bytes, reviewer, timestamp, actor, true,
        )
    } else {
        crate::share_merge::core::atomic(store, |store| {
            plan_pair_inner(
                store, pair, file, file_bytes, reviewer, timestamp, actor, false,
            )
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn plan_pair_inner(
    store: &mut Store,
    pair: SharePair,
    file: &DecisionFile,
    file_bytes: &[u8],
    reviewer: &str,
    timestamp: &str,
    actor: Option<&str>,
    dry_run: bool,
) -> Result<DecidedMerge> {
    if reviewer.trim().is_empty() {
        return Err(Error::InvalidValue("a reviewer is required".into()));
    }
    let scoped = file.schema == DECISIONS_SCHEMA;
    if !scoped && file.schema != DECISIONS_SCHEMA_V1 {
        return Err(Error::InvalidValue(format!(
            "not a merge decisions file: schema {:?}",
            file.schema
        )));
    }
    check_fields(file_bytes)?;
    let decisions_sha256 = sha256(file_bytes);
    if let Some(done) = binding::already_applied(store, file, &decisions_sha256, reviewer)? {
        return Ok(done);
    }
    let i = inputs(store, pair)?;
    let stale = |what: &str| {
        Error::InvalidValue(format!(
            "stale decisions: {what} changed since they were emitted (--emit-decisions / quipu_merge_decisions); emit again and re-decide. Nothing was written"
        ))
    };
    if file.incoming_share != i.incoming.manifest.share_id {
        return Err(stale("the incoming share"));
    }
    if file.base_share != i.base.manifest.share_id {
        return Err(stale("the base share"));
    }
    if file.incoming_graph_hash != i.incoming.manifest.graph_hash {
        return Err(stale("the incoming graph"));
    }
    if !scoped && file.local_graph_hash != i.ours_hash {
        return Err(stale("ROOT"));
    }
    let (mut merged, conflicts) = merge_graphs(
        &i.base.graph,
        &i.ours,
        &i.incoming.graph,
        &i.incoming.shapes,
    )?;
    // Same conflicts, in any row order: an operator may sort the file.
    let same = conflicts.len() == file.rows.len()
        && conflicts
            .iter()
            .all(|c| file.rows.iter().any(|row| &row.record == c));
    if !same {
        return Err(stale("the set of conflicts"));
    }
    if scoped {
        for row in &file.rows {
            if !row.record.subject.starts_with('<') {
                return Err(stale("a slot without verifiable local provenance"));
            }
            if row.local_slot_hash.as_deref() != Some(slot_hash(store, &row.record)?.as_str()) {
                return Err(stale("a reviewed slot's values or provenance"));
            }
        }
    }
    let mut applied = Vec::new();
    for row in &file.rows {
        let r = &row.record;
        let values = chosen(row)?;
        if values.len() > r.max_count {
            return Err(Error::InvalidValue(format!(
                "row {} decides {} values for {} {}, above sh:maxCount {}; nothing was written",
                row.id,
                values.len(),
                r.subject,
                r.predicate,
                r.max_count
            )));
        }
        merged.retain(|t| {
            !(t.subject.to_string() == r.subject && t.predicate.as_str() == r.predicate)
        });
        for value in &values {
            let line = format!("{} <{}> {} .", r.subject, r.predicate, value);
            let parsed = parse_graph(&line, &format!("row {} value {value}", row.id))
                .map_err(|e| Error::InvalidValue(format!("{e}; nothing was written")))?;
            // The value is spliced into one N-Triples line, so a newline in it
            // could add triples outside this slot or past sh:maxCount. Exactly
            // one triple, on this row's subject and predicate, or refuse.
            let mut triples = parsed.into_iter();
            let one = match (triples.next(), triples.next()) {
                (Some(t), None)
                    if t.subject.to_string() == r.subject
                        && t.predicate.as_str() == r.predicate =>
                {
                    t
                }
                _ => {
                    return Err(Error::InvalidValue(format!(
                        "row {} value {value:?} is not one RDF term for {} {}; nothing was written",
                        row.id, r.subject, r.predicate
                    )));
                }
            };
            merged.insert(one);
        }
        applied.push(AppliedRow {
            id: row.id.clone(),
            subject: r.subject.clone(),
            predicate: r.predicate.clone(),
            values,
            decided_by: row.decided_by.clone(),
            decided_at: row.decided_at.clone(),
        });
    }
    let parents = [i.ours_hash, i.incoming.manifest.share_id.clone()];
    let source = format!(
        "share-merge:parents={},{};decisions={decisions_sha256};reviewer={reviewer}",
        parents[0], parents[1]
    );
    let merge = if dry_run {
        MergeResult {
            outcome: "dry-run".into(),
            tx_id: None,
            asserted: merged.difference(&i.ours).count(),
            retracted: i.ours.difference(&merged).count(),
            provenance_parents: parents,
            conflicts: Vec::new(),
        }
    } else {
        use crate::merge_core::Decision as CoreDecision;
        let choices = file
            .rows
            .iter()
            .map(|row| {
                let decision = match &row.decision {
                    Some(Decision::Choose { choose: Side::Ours }) => CoreDecision::Ours,
                    Some(Decision::Choose {
                        choose: Side::Theirs,
                    }) => CoreDecision::Theirs,
                    Some(Decision::Choose { choose: Side::Base }) => CoreDecision::Base,
                    Some(Decision::Choose { choose: Side::Both }) => CoreDecision::Both,
                    Some(Decision::Values { values }) => {
                        CoreDecision::Values(values.iter().cloned().collect())
                    }
                    None => CoreDecision::Defer,
                };
                (
                    (row.record.subject.clone(), row.record.predicate.clone()),
                    decision,
                )
            })
            .collect();
        let plan = crate::share_merge::core::plan(
            store,
            &crate::share_merge::core::Inputs {
                base: &i.base.graph,
                ours: &i.ours,
                theirs: &i.incoming.graph,
                shapes: &i.incoming.shapes,
                authored_at: &i.incoming.manifest.created_at,
            },
            &choices,
            timestamp,
        )?;
        let id = crate::merge_core::merge_id(
            "share",
            &i.incoming.manifest.share_id,
            &i.base.manifest.share_id,
            &decisions_sha256,
        );
        let source = format!("{source};merge_id={id}");
        commit(
            store, &i.ours, &merged, &plan, parents, &source, timestamp, actor,
        )?
    };
    Ok(DecidedMerge {
        merge,
        reviewer: reviewer.into(),
        decisions_sha256,
        applied,
    })
}
