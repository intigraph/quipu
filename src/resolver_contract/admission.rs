//! Coordinate binding only; normal master admission remains the authority.

use super::*;
use crate::{Error, Result};

fn replace(time: &mut ProspectiveTime, actual: &str) {
    if matches!(time, ProspectiveTime::AdmissionAssigned) {
        *time = ProspectiveTime::AdmissionResolved(actual.to_owned());
    }
}

fn row(value: &mut ProspectiveRow, actual: &str) {
    replace(&mut value.valid_from, actual);
    if let Some(end) = &mut value.valid_to {
        replace(end, actual);
    }
}

pub(super) fn assign(value: &ProspectiveCandidate, actual: &str) -> Result<ProspectiveCandidate> {
    if crate::time::normalize_rfc3339_utc(actual).as_deref() != Some(actual) {
        return Err(Error::InvalidValue(
            "admission time must be canonical UTC seconds".into(),
        ));
    }
    let mut assigned = value.clone();
    for value in &mut assigned.rows {
        row(value, actual);
    }
    for decision in &mut assigned.decisions {
        for operation in &mut decision.operations {
            match operation {
                ProspectiveOperation::Assert(value) => row(value, actual),
                ProspectiveOperation::Close { at, .. }
                | ProspectiveOperation::Received { at, .. } => replace(at, actual),
            }
        }
    }
    validation::candidate(&assigned)?;
    Ok(assigned)
}

pub(super) fn bind(
    value: &ProspectiveCandidate,
    manifest: &ValidatorManifest,
    observed: ValidationOutcome,
    actual: AdmissionCoordinates,
) -> Result<AdmissionValidation> {
    if crate::time::normalize_rfc3339_utc(&actual.accepted_at).as_deref()
        != Some(actual.accepted_at.as_str())
        || actual.transaction <= value.source.base_tx
    {
        return Err(Error::InvalidValue(
            "invalid actual admission coordinates".into(),
        ));
    }
    let agrees = |time: &ProspectiveTime| !matches!(time, ProspectiveTime::AdmissionResolved(v) if v != &actual.accepted_at);
    let row_agrees =
        |row: &ProspectiveRow| agrees(&row.valid_from) && row.valid_to.as_ref().is_none_or(agrees);
    if value.rows.iter().any(|row| !row_agrees(row))
        || value
            .decisions
            .iter()
            .flat_map(|v| &v.operations)
            .any(|op| match op {
                ProspectiveOperation::Assert(row) => !row_agrees(row),
                ProspectiveOperation::Close { at, .. }
                | ProspectiveOperation::Received { at, .. } => !agrees(at),
            })
    {
        return Err(Error::InvalidValue(
            "resolved time differs from actual admission coordinates".into(),
        ));
    }
    let evidence = value.bind_validation(manifest, observed)?;
    if evidence.outcome != ValidationOutcome::Conforms {
        return Err(Error::InvalidValue(
            "admission evidence requires fresh post-assignment conformance".into(),
        ));
    }
    Ok(AdmissionValidation {
        evidence,
        master: actual,
    })
}
