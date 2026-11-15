//! Wire values for the admitted fluent adapter, over the native writer only.
use super::{Current, Policy, Scalar, invalid, native_registry, writer};
use crate::{Result, Store};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// An append contains domain data, never caller-selected writer/tx/recording time.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppendRequest {
    pub graph: String,
    pub fluent: String,
    pub event: String,
    pub subject: String,
    pub predicate: String,
    pub property_shape: String,
    pub as_of: String,
    pub value: Scalar,
}

/// A derived value and its immutable producing transactions, at a store snapshot.
#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub graph: String,
    pub fluent: String,
    pub policy: Policy,
    pub current: Current,
    pub producing_txs: BTreeSet<u64>,
    pub snapshot_tx: i64,
}

fn graph_id(store: &Store, graph: &str) -> Result<i64> {
    writer::iri(graph)?;
    let id = if graph == crate::schema::ROOT_GRAPH_IRI {
        0
    } else {
        store
            .lookup(graph)?
            .ok_or_else(|| invalid("unknown graph"))?
    };
    if store.graph_class(id)?.is_none() {
        return Err(invalid("unknown graph"));
    }
    Ok(id)
}

impl Store {
    /// Append through native reservation, ownership, SHACL and immutable-log checks.
    /// The caller must already have scoped verified transaction identity.
    pub fn append_fluent_observation(&mut self, request: &AppendRequest) -> Result<i64> {
        let graph = graph_id(self, &request.graph)?;
        self.append_fluent_native(
            &writer::Append {
                fluent: request.fluent.clone(),
                event: request.event.clone(),
                subject: request.subject.clone(),
                predicate: request.predicate.clone(),
                property_shape: request.property_shape.clone(),
                as_of: request.as_of.clone(),
                value: request.value.clone(),
            },
            graph,
        )
    }

    /// Read the existing native graph; an absent declaration is an error, not zero.
    pub fn fluent_snapshot(&self, graph: &str, fluent: &str) -> Result<Snapshot> {
        let id = graph_id(self, graph)?;
        writer::iri(fluent)?;
        let (registry, log) = native_registry::load(self, id)?;
        let policy = registry.policy(fluent)?;
        let current = registry.current(fluent, &log)?;
        let producing_txs = log
            .events
            .values()
            .filter(|event| current.produced_by.contains(&event.order.event))
            .map(|event| event.order.tx)
            .collect();
        Ok(Snapshot {
            graph: graph.into(),
            fluent: fluent.into(),
            policy,
            current,
            producing_txs,
            snapshot_tx: self.latest_tx_id()?,
        })
    }
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
