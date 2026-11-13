//! Immutable observations and deterministic current-value projections.
//!
//! This module never changes ordinary merge slots. Observation identity is
//! independent of its host clock; current order is transaction, writer, id.
use std::collections::{BTreeMap, BTreeSet};

use crate::{Error, Result};
use serde::{Deserialize, Serialize};

mod native_registry;
mod observable;
#[allow(dead_code)]
pub(crate) mod pending_config;
mod portable;
#[cfg(feature = "shacl")]
mod projection;
pub(crate) mod registration;
mod shapes;
// Source-only protocol core; no runtime witness grant loader or import adapter.
#[allow(dead_code)]
pub(crate) mod source_witness;
mod targets;
pub mod transport;
mod valid_time;
pub(crate) mod writer;
pub use observable::{Observable, Registry};
pub use shapes::{Declaration, declarations};
pub use targets::{Binding, bindings};
pub use valid_time::DomainInterval;

/// Aggregation declared by a fluent property shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Policy {
    Latest,
    Max,
    Sum,
    Set,
}

/// A portable value: RDF term text for latest/set, exact integers for max/sum.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Scalar {
    Integer(i64),
    Term(String),
}

/// Source ordering, retained across import; never replaced by import time.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Order {
    pub tx: u64,
    pub writer: String,
    pub event: String,
}

/// One immutable event. Sum values are increments, not absolute snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub fluent: String,
    pub order: Order,
    pub as_of: String,
    pub recorded_on: String,
    pub value: Scalar,
}

/// An append-only event set. A reused event id with different content is invalid
/// input, rather than a merge conflict that could discard an observation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<Observation>", into = "Vec<Observation>")]
pub struct Log {
    events: BTreeMap<String, Observation>,
}

/// Current value and the producing events (the projection's reifier links).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Current {
    pub values: BTreeSet<Scalar>,
    pub produced_by: BTreeSet<String>,
}

/// A half-open interval on the authoritative transaction order. Source wall
/// times remain on the events and do not determine the winner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Interval {
    pub from: Order,
    pub until: Option<Order>,
    pub current: Current,
}

impl TryFrom<Vec<Observation>> for Log {
    type Error = Error;
    fn try_from(events: Vec<Observation>) -> Result<Self> {
        let mut log = Self::default();
        for event in events {
            log.append(event)?;
        }
        Ok(log)
    }
}
impl From<Log> for Vec<Observation> {
    fn from(log: Log) -> Self {
        log.events.into_values().collect()
    }
}

fn invalid(message: &str) -> Error {
    Error::InvalidValue(format!("fluent: {message}"))
}

impl Log {
    /// Append or replay exactly the same event; never overwrite history.
    pub fn append(&mut self, observation: Observation) -> Result<bool> {
        let o = &observation;
        portable::validate(o)?;
        if let Some(existing) = self.events.get(&o.order.event) {
            if existing != o {
                return Err(invalid("event identity reused with different content"));
            }
            return Ok(false);
        }
        self.events.insert(o.order.event.clone(), observation);
        Ok(true)
    }

    /// Set-union merge. Validate the complete candidate before changing self.
    pub fn merge(&mut self, other: &Self) -> Result<()> {
        let mut merged = self.clone();
        for event in other.events.values() {
            merged.append(event.clone())?;
        }
        *self = merged;
        Ok(())
    }

    /// Stable event traversal, including source wall-clock declarations.
    pub fn observations(&self, fluent: &str) -> Vec<&Observation> {
        let mut observations: Vec<_> = self
            .events
            .values()
            .filter(|o| o.fluent == fluent)
            .collect();
        observations.sort_by(|a, b| a.order.cmp(&b.order));
        observations
    }

    /// Derive current state without retracting any event.
    pub fn current(&self, fluent: &str, policy: Policy) -> Result<Option<Current>> {
        reduce(&self.observations(fluent), policy)
    }

    /// Derive every half-open order interval and its exact source-event links.
    pub fn intervals(&self, fluent: &str, policy: Policy) -> Result<Vec<Interval>> {
        let observations = self.observations(fluent);
        observations
            .iter()
            .enumerate()
            .map(|(i, observation)| {
                Ok(Interval {
                    from: observation.order.clone(),
                    until: observations.get(i + 1).map(|next| next.order.clone()),
                    current: reduce(&observations[..=i], policy)?.expect("nonempty prefix"),
                })
            })
            .collect()
    }
}

fn integer(o: &Observation) -> Result<i64> {
    match o.value {
        Scalar::Integer(n) => Ok(n),
        Scalar::Term(_) => Err(invalid("max/sum require integer observations")),
    }
}

fn reduce(observations: &[&Observation], policy: Policy) -> Result<Option<Current>> {
    let Some(last) = observations.last() else {
        return Ok(None);
    };
    let mut values = BTreeSet::new();
    let mut produced_by = BTreeSet::new();
    match policy {
        Policy::Latest => {
            values.insert(last.value.clone());
            produced_by.insert(last.order.event.clone());
        }
        Policy::Set => {
            for observation in observations {
                values.insert(observation.value.clone());
                produced_by.insert(observation.order.event.clone());
            }
        }
        Policy::Max => {
            let maximum = observations
                .iter()
                .map(|o| integer(o))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .max()
                .expect("nonempty observations");
            values.insert(Scalar::Integer(maximum));
            for observation in observations {
                if integer(observation)? == maximum {
                    produced_by.insert(observation.order.event.clone());
                }
            }
        }
        Policy::Sum => {
            let mut sum = 0_i128;
            for observation in observations {
                sum = sum
                    .checked_add(i128::from(integer(observation)?))
                    .ok_or_else(|| invalid("integer sum overflow"))?;
                produced_by.insert(observation.order.event.clone());
            }
            values.insert(Scalar::Integer(
                i64::try_from(sum).map_err(|_| invalid("integer sum overflow"))?,
            ));
        }
    }
    Ok(Some(Current {
        values,
        produced_by,
    }))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod target_tests;
