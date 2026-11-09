//! Ordinary fact adapters cannot manufacture or edit native fluent records.
use super::{Datum, Store};
use crate::{Error, Result, Value};
use rusqlite::params;
use std::collections::BTreeSet;

/// Source/actor strings are never writer grants. Only the private materializer
/// can select native staging; transport admission must supply its own proof.
pub(super) fn check(store: &Store, graph: i64, datums: &[Datum]) -> Result<()> {
    let Some(type_id) = store.lookup(crate::namespace::RDF_TYPE)? else {
        return Ok(());
    };
    let types: BTreeSet<_> = ["Fluent", "FluentObservation", "Observable"]
        .into_iter()
        .map(|name| store.lookup(&format!("{}{name}", crate::namespace::QUIPU)))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();
    if types.is_empty() {
        return Ok(());
    }
    let refused =
        || Error::InvalidValue("fluent: records require an admitted append-only writer".into());
    if datums
        .iter()
        .any(|d| d.attribute == type_id && matches!(d.value, Value::Ref(id) if types.contains(&id)))
    {
        return Err(refused());
    }
    // Historical typing protects a record even if its current type was damaged.
    // Probe the indexed slot, not the entity's arbitrarily large event history.
    let mut query = store
        .conn
        .prepare_cached("SELECT 1 FROM facts WHERE g=?1 AND e=?2 AND a=?3 AND v=?4 LIMIT 1")?;
    for entity in datums.iter().map(|d| d.entity).collect::<BTreeSet<_>>() {
        for kind in &types {
            if query.exists(params![
                graph,
                entity,
                type_id,
                Value::Ref(*kind).to_bytes()
            ])? {
                return Err(refused());
            }
        }
    }
    Ok(())
}

/// Inference can repeat a prepared triple, or add ordinary facts elsewhere,
/// but it cannot expand the immutable native record or mint another one.
pub(super) fn check_native_inferences(
    store: &Store,
    graph: i64,
    prepared: &[Datum],
    inferred: &[Datum],
) -> Result<()> {
    let records: BTreeSet<_> = prepared.iter().map(|d| d.entity).collect();
    let additional: Vec<_> = inferred
        .iter()
        .filter(|d| {
            !prepared.iter().any(|p| {
                p.entity == d.entity
                    && p.attribute == d.attribute
                    && p.value == d.value
                    && p.op == d.op
                    && p.valid_from == d.valid_from
                    && p.valid_to == d.valid_to
            })
        })
        .cloned()
        .collect();
    if additional.iter().any(|d| records.contains(&d.entity)) {
        return Err(Error::InvalidValue(
            "fluent: inference changes prepared immutable record content".into(),
        ));
    }
    check(store, graph, &additional)
}

impl Store {
    /// Event identity is global within this local Store, even when the value
    /// reader is graph-scoped. Do not manufacture a second origin elsewhere.
    pub(crate) fn check_fluent_event_graph(&self, event: &str, graph: i64) -> Result<()> {
        let Some(event) = self.lookup(event)? else {
            return Ok(());
        };
        let mut query = self
            .conn
            .prepare_cached("SELECT 1 FROM facts WHERE e=?1 AND g<>?2 LIMIT 1")?;
        if query.exists(params![event, graph])? {
            return Err(Error::InvalidValue(
                "fluent: event identity already belongs to another graph".into(),
            ));
        }
        Ok(())
    }

    /// An identity already bound in any local graph cannot be rebound by a
    /// native append. Retained history, not just live typing, fixes that graph.
    pub(crate) fn check_fluent_graph_binding(&self, fluent: &str, graph: &str) -> Result<()> {
        let (Some(entity), Some(attribute)) = (
            self.lookup(fluent)?,
            self.lookup(&format!("{}inGraph", crate::namespace::QUIPU))?,
        ) else {
            return Ok(());
        };
        let graph = self.lookup(graph)?.map(|id| Value::Ref(id).to_bytes());
        let mut query = self.conn.prepare_cached(
            "SELECT 1 FROM facts WHERE e=?1 AND a=?2 AND (?3 IS NULL OR v<>?3) LIMIT 1",
        )?;
        if query.exists(params![entity, attribute, graph])? {
            return Err(Error::InvalidValue(
                "fluent: native identity cannot change graph".into(),
            ));
        }
        Ok(())
    }
}
