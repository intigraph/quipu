//! Deterministic import identifiers and claimed provenance formatting.

use crate::error::{Error, Result};

fn hash_suffix(value: &str) -> Result<&str> {
    value
        .strip_prefix("sha256:")
        .filter(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| Error::InvalidValue(format!("invalid sha256 identifier: {value}")))
}

pub(super) fn staging_graph(share_id: &str, quarantine: bool) -> Result<String> {
    let hash = hash_suffix(share_id)?;
    let kind = if quarantine { "quarantine" } else { "staging" };
    Ok(format!("urn:quipu:import:{kind}:{hash}"))
}

pub(super) fn import_id(share_id: &str) -> Result<String> {
    Ok(format!("urn:quipu:import:event:{}", hash_suffix(share_id)?))
}

pub(super) fn provenance_source(prefix: &str, id: &str, claim: Option<&str>) -> String {
    claim.map_or_else(
        || format!("{prefix}:{id}"),
        |actor| format!("{prefix}:{id}:claimed-actor={actor}"),
    )
}
