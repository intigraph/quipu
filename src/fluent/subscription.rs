//! Transport-neutral change detection for completed transaction-cut projections.
//!
//! The caller must obtain a graph-authorized projection at `tx`, not read the
//! latest head and label it with an older transaction. This component neither
//! opens a feed nor grants a writer, shape, producer or graph-read capability.
use super::{Current, Scalar, invalid};
use crate::Result;
use oxrdf::NamedNode;
use serde::Serialize;
use std::collections::BTreeSet;

const MAX_VALUES: usize = 256;
const MAX_FRAME_BYTES: usize = 64 * 1024;

/// One value transition, coalesced by its completed source transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Frame {
    pub fluent: String,
    pub value: BTreeSet<Scalar>,
    pub tx: i64,
}

/// A graph-scoped subscription's delivered/scanned transaction and value.
/// Provenance-only changes advance the cursor without emitting a value frame.
pub struct Subscription {
    graph: String,
    fluent: String,
    scanned: i64,
    value: Option<BTreeSet<Scalar>>,
}

impl Subscription {
    /// Establish a baseline without inventing an initial change notification.
    pub fn new(graph: &str, fluent: &str, tx: i64, current: Option<&Current>) -> Result<Self> {
        for name in [graph, fluent] {
            if name.len() > 2048 || NamedNode::new(name).is_err() {
                return Err(invalid("subscription requires bounded graph/fluent IRIs"));
            }
        }
        if tx < 0 {
            return Err(invalid("subscription transaction must be nonnegative"));
        }
        validate(fluent, tx, current)?;
        Ok(Self {
            graph: graph.into(),
            fluent: fluent.into(),
            scanned: tx,
            value: current.map(|c| c.values.clone()),
        })
    }

    pub fn scanned(&self) -> i64 {
        self.scanned
    }

    /// Accept one completed projection for this exact scope. An unrelated
    /// scope does not consume its transaction: another projection for our
    /// scope may still occur in that same transaction. Conflicts and invalid
    /// projections leave both the cursor and prior value unchanged.
    pub fn advance(
        &mut self,
        graph: &str,
        fluent: &str,
        tx: i64,
        current: &Current,
    ) -> Result<Option<Frame>> {
        if graph != self.graph || fluent != self.fluent {
            return Ok(None);
        }
        if tx < self.scanned {
            return Err(invalid("subscription projection moved backwards"));
        }
        validate(fluent, tx, Some(current))?;
        if tx == self.scanned {
            return if self.value.as_ref() == Some(&current.values) {
                Ok(None)
            } else {
                Err(invalid(
                    "subscription transaction has conflicting projections",
                ))
            };
        }
        let changed = self.value.as_ref() != Some(&current.values);
        self.scanned = tx;
        self.value = Some(current.values.clone());
        Ok(changed.then(|| Frame {
            fluent: self.fluent.clone(),
            value: current.values.clone(),
            tx,
        }))
    }
}

fn validate(fluent: &str, tx: i64, current: Option<&Current>) -> Result<()> {
    let Some(current) = current else {
        return Ok(());
    };
    if current.values.is_empty() || current.values.len() > MAX_VALUES {
        return Err(invalid("subscription projection value count is invalid"));
    }
    let mut text_bytes = 0usize;
    for value in &current.values {
        if let Scalar::Term(text) = value {
            text_bytes = text_bytes
                .checked_add(text.len())
                .ok_or_else(|| invalid("subscription projection byte count overflow"))?;
            if text_bytes > MAX_FRAME_BYTES {
                return Err(invalid("subscription projection exceeds frame budget"));
            }
        }
    }
    // Account for the actual wire bytes, including escaping, before retaining
    // a new value or advancing a cursor. Unknown reads are errors at the caller,
    // never an empty/zero projection here.
    let bytes = serde_json::to_vec(&Frame {
        fluent: fluent.into(),
        value: current.values.clone(),
        tx,
    })
    .map_err(|_| invalid("subscription projection cannot be encoded"))?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(invalid("subscription projection exceeds frame budget"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "subscription_tests.rs"]
mod tests;
