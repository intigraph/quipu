//! Explicit deployment admission for the data merge risk gate.
//!
//! No policy is installed by source publication. An absent environment policy
//! leaves the existing adapters inactive; enabling it is a separate deployment.
use crate::change_risk::Verdict;
use crate::error::{Error, Result};
use crate::merge_plan::{Policy, plan_touched};
use crate::store::{Datum, Store};
use crate::types::Op;
use std::collections::BTreeSet;

/// Receiver-owned configuration, never a field of a retained merge request.
pub const POLICY_ENV: &str = "QUIPU_MERGE_RISK_POLICY";

pub(crate) fn policy() -> Result<Option<Policy>> {
    match std::env::var(POLICY_ENV) {
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(Error::InvalidValue(format!("merge policy: {error}"))),
        Ok(value) => serde_json::from_str(&value)
            .map(Some)
            .map_err(|error| Error::InvalidValue(format!("merge policy: {error}"))),
    }
}

/// All operation paths use the actual lowered datums, including zero-width
/// receipts, instead of trusting the caller's prospective graph annotation.
pub(crate) fn datums(store: &Store, steps: &[&[Datum]], ledger: Option<&str>) -> Result<()> {
    let Some(policy) = policy()? else {
        return Ok(());
    };
    let before = crate::share_merge::root_graph(store)?.0;
    let mut after = before.clone();
    let mut touched = BTreeSet::new();
    let mut exposure = before.clone();
    for datum in steps.iter().flat_map(|s| s.iter()) {
        if matches!(datum.op, Op::Tombstone) {
            return Err(Error::InvalidValue(
                "automatic merge needs decisions: overlay Tombstone operation is unproved".into(),
            ));
        }
        let subject = store.resolve(datum.entity)?;
        let predicate = store.resolve(datum.attribute)?;
        let object = crate::rdf::value_to_term(store, &datum.value)?;
        touched.insert((format!("<{subject}>"), predicate.clone()));
        let triples = crate::share_merge::parse_graph(
            &format!("<{subject}> <{predicate}> {object} ."),
            "risk gate operation",
        )?;
        for triple in triples {
            if matches!(datum.op, Op::Assert) {
                exposure.insert(triple.clone());
            }
            match datum.op {
                Op::Assert if datum.valid_to.is_none() => {
                    after.insert(triple);
                }
                Op::Retract => {
                    after.remove(&triple);
                }
                Op::Assert => {}
                Op::Tombstone => unreachable!("refused before graph projection"),
            }
        }
    }
    let mut report = plan_touched(
        store,
        &crate::merge_session::render(&before),
        &crate::merge_session::render(&after),
        &policy,
        &touched,
    )?;
    exposure.extend(after.iter().cloned());
    report.protected.extend(crate::merge_plan::protected(
        &before, &exposure, &touched, &policy,
    ));
    if !report.protected.is_empty() {
        report.risk.verdict = Verdict::Fail;
    }
    let operations: Vec<_> = steps
        .iter()
        .map(|step| {
            step.iter()
                .map(|d| {
                    (
                        d.entity,
                        d.attribute,
                        d.value.term_key(),
                        &d.valid_from,
                        &d.valid_to,
                        format!("{:?}", d.op),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect();
    report.operation_hash = Some(crate::share::sha256(
        &serde_json::to_vec(&(operations, ledger))
            .map_err(|e| Error::Serialization(e.to_string()))?,
    ));
    if let Some(ledger) = ledger {
        report.write_graphs.insert(ledger.into());
        report
            .undetermined
            .push("retained ledger post-state query coverage is not qualified".into());
        if report.risk.verdict == Verdict::Pass {
            report.risk.verdict = Verdict::Undetermined;
        }
    }
    if report.risk.verdict != Verdict::Pass {
        return Err(Error::InvalidValue(format!(
            "automatic merge needs decisions; plan={}",
            serde_json::to_string(&report).map_err(|e| Error::Serialization(e.to_string()))?
        )));
    }
    Ok(())
}

#[cfg(test)]
#[path = "merge_plan_gate_tests.rs"]
mod tests;
