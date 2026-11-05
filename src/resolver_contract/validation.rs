//! Structural/incompleteness checks only, never a SHACL or admission substitute.

use super::*;
use crate::changeset::Statement;
use crate::{Error, Result};

fn refuse(why: &str) -> Error {
    Error::InvalidValue(format!("resolver contract: {why}"))
}

fn triple(value: &str) -> Result<()> {
    let statement = Statement::new(value, None);
    let quad = statement.to_quad()?;
    if statement.has_blank_node() || Statement::from_quad(&quad).triple != value {
        return Err(refuse("triple must be canonical and skolemized"));
    }
    Ok(())
}

fn time(value: &ProspectiveTime) -> Result<()> {
    if let ProspectiveTime::RetainedUtc(value) | ProspectiveTime::AdmissionResolved(value) = value
        && crate::time::normalize_rfc3339_utc(value).as_deref() != Some(value.as_str())
    {
        return Err(refuse("retained time must be canonical UTC seconds"));
    }
    Ok(())
}

fn known(value: &ProspectiveTime) -> Option<&str> {
    match value {
        ProspectiveTime::RetainedUtc(v) | ProspectiveTime::AdmissionResolved(v) => Some(v),
        ProspectiveTime::AdmissionAssigned => None,
    }
}

fn row(value: &ProspectiveRow) -> Result<()> {
    triple(&value.triple)?;
    time(&value.valid_from)?;
    if let Some(end) = &value.valid_to {
        time(end)?;
        if let (Some(start), Some(end)) = (known(&value.valid_from), known(end))
            && end <= start
        {
            return Err(refuse(
                "data row interval must be nonempty; use Received for history",
            ));
        }
    }
    Ok(())
}

fn slot(value: &Slot) -> Result<()> {
    // Reuse the existing canonical RDF terms parser, with a fixed named object.
    triple(&format!(
        "{} {} <urn:resolver:slot-check>",
        value.subject, value.predicate
    ))
}

pub(super) fn candidate(value: &ProspectiveCandidate) -> Result<()> {
    let source = &value.source;
    if source.store_id.is_empty() || source.log_epoch.is_empty() || source.graph.is_empty() {
        return Err(refuse("store, epoch and graph must be explicit"));
    }
    let mut names = BTreeSet::new();
    for input in &source.inputs {
        if input.name.is_empty() || input.version.is_empty() || !names.insert(input.name.as_str()) {
            return Err(refuse(
                "input bindings need unique names and explicit versions",
            ));
        }
    }
    if names != BTreeSet::from(["base", "ours", "theirs", "decisions"]) {
        return Err(refuse(
            "bind exactly base, ours, theirs and decisions inputs",
        ));
    }
    if let Some(scope) = &source.scope {
        for value in scope {
            slot(value)?;
        }
    }
    for value in &value.rows {
        row(value)?;
    }
    for decision in &value.decisions {
        slot(&decision.slot)?;
        for op in &decision.operations {
            match op {
                ProspectiveOperation::Assert(value) => row(value)?,
                ProspectiveOperation::Close { triple: value, at }
                | ProspectiveOperation::Received { triple: value, at } => {
                    triple(value)?;
                    time(at)?;
                }
            }
        }
    }
    Ok(())
}

fn join(values: &[NamedContent], separator: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut previous: Option<&str> = None;
    for value in values {
        if value.name.is_empty()
            || value.version.is_empty()
            || previous.is_some_and(|name| name >= value.name.as_str())
        {
            return Err(refuse(
                "registry snapshot needs named versions in strict name order",
            ));
        }
        if previous.is_some() {
            out.extend_from_slice(separator);
        }
        out.extend_from_slice(&value.content);
        previous = Some(&value.name);
    }
    Ok(out)
}

pub(super) fn manifest(value: &ValidatorManifest) -> Result<()> {
    if join(&value.loaded_shapes, b"\n\n")? != value.combined_shapes
        || join(&value.loaded_ontologies, b"\n")? != value.combined_ontologies
    {
        return Err(refuse(
            "combined registry bytes do not match the exact ordered join",
        ));
    }
    if value.incremental.selection_policy.is_empty()
        || value.incremental.fallback_contract.is_empty()
    {
        return Err(refuse(
            "incremental selection and fallback contracts must be explicit",
        ));
    }
    for option in [
        "engine",
        "parser",
        "shape_base",
        "data_base",
        "validation_mode",
    ] {
        if !value
            .implementation
            .options
            .get(option)
            .is_some_and(|v| !v.is_empty())
        {
            return Err(refuse(
                "implementation options must bind engine/parser/base IRIs/mode",
            ));
        }
    }
    // Selection is a separately bound registry subset, not an inferred union.
    join(&value.incremental.selected_shapes, b"\n\n")?;
    if value
        .incremental
        .selected_shapes
        .iter()
        .any(|v| !value.loaded_shapes.contains(v))
    {
        return Err(refuse(
            "incremental selection is not from the bound loaded snapshot",
        ));
    }
    Ok(())
}

fn assigned(value: &ProspectiveTime) -> bool {
    matches!(value, ProspectiveTime::AdmissionAssigned)
}

fn row_assigned(value: &ProspectiveRow) -> bool {
    assigned(&value.valid_from) || value.valid_to.as_ref().is_some_and(assigned)
}

pub(super) fn evidence(
    value: &ProspectiveCandidate,
    manifest: &ValidatorManifest,
    observed: ValidationOutcome,
) -> Result<ValidationEvidence> {
    let validator = manifest.identity()?;
    if value.validator != validator {
        return Err(refuse(
            "candidate is bound to a different validator manifest",
        ));
    }
    let features = &manifest.implementation.compiled_features;
    let incomplete = !features.contains("shacl")
        || manifest.incremental.coverage != ContextCoverage::Complete
        || u64::try_from(manifest.incremental.facts.len())
            .ok()
            .is_none_or(|n| n > manifest.incremental.fact_budget)
        || (features.contains("owl")
            && manifest.ontology_cache_source.as_ref()
                != Some(&encoding::digest(&manifest.combined_ontologies)));
    let time_pending = value.rows.iter().any(row_assigned)
        || value
            .decisions
            .iter()
            .flat_map(|v| &v.operations)
            .any(|op| match op {
                ProspectiveOperation::Assert(row) => row_assigned(row),
                ProspectiveOperation::Close { at, .. }
                | ProspectiveOperation::Received { at, .. } => assigned(at),
            });
    let outcome = if observed == ValidationOutcome::Conforms && incomplete {
        ValidationOutcome::Unknown
    } else if observed == ValidationOutcome::Conforms && time_pending {
        ValidationOutcome::RequiresAdmissionTime
    } else {
        observed
    };
    Ok(ValidationEvidence {
        candidate: value.identity()?,
        validator,
        outcome,
    })
}
