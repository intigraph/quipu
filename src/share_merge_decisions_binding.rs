//! Current slot values and local provenance used by v2 merge decisions.

use super::{DecisionRecord, LocalFact};
use crate::error::{Error, Result};
use crate::share::sha256;
use crate::store::Store;

pub(super) fn local_facts(store: &Store, record: &DecisionRecord) -> Result<Vec<LocalFact>> {
    let Some(subject) = record
        .subject
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
    else {
        return Ok(Vec::new());
    };
    let (Some(entity), Some(attribute)) =
        (store.lookup(subject)?, store.lookup(&record.predicate)?)
    else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for fact in store.entity_facts(entity)? {
        if fact.attribute != attribute {
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
    out.sort_by(|a, b| a.value.cmp(&b.value));
    Ok(out)
}

/// Bind the reviewed values and their local provenance, not unrelated ROOT facts.
pub(super) fn slot_hash(store: &Store, record: &DecisionRecord) -> Result<String> {
    let bytes = serde_json::to_vec(&(
        &record.subject,
        &record.predicate,
        &record.ours,
        local_facts(store, record)?,
    ))
    .map_err(|e| Error::Serialization(e.to_string()))?;
    Ok(sha256(&bytes))
}
