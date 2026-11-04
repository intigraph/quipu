//! Portable, non-authoritative composition over a caught-up canonical graph.
#[cfg(test)]
#[path = "preview_tests.rs"]
mod preview_tests;

use std::collections::BTreeMap;

use super::*;
use crate::merge3::{DecisionRecord, ShapeBounds, TripleView, merge_slots};

/// A preview is never an admission or SHACL-validation decision. Conflicts do
/// not expose a partially resolved graph as a successful composed read.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum OverlayPreview {
    /// Canonical catch-up composed with local proposals, without changing either.
    Composed {
        /// Canonical, skolemized N-Triples terms, without trailing dots.
        triples: BTreeSet<String>,
    },
    /// Explicit unresolved slots. The frozen base and private proposal survive.
    Conflicts {
        /// The shared merge operator's decisions: ours is canonical, theirs local.
        records: Vec<DecisionRecord>,
    },
}

type Slot = (String, String);
type Slots = BTreeMap<Slot, BTreeSet<String>>;

fn parse(rows: impl IntoIterator<Item = String>) -> Result<Vec<oxrdf::Triple>> {
    rows.into_iter()
        .map(|triple| {
            let statement = Statement::new(triple, None);
            let quad = statement.to_quad()?;
            if statement.has_blank_node() || Statement::from_quad(&quad) != statement {
                return Err(refuse("preview requires canonical skolemized terms"));
            }
            Ok(oxrdf::Triple::from(quad))
        })
        .collect()
}

fn slots(triples: &[oxrdf::Triple]) -> Slots {
    let mut result = Slots::new();
    for triple in triples {
        result
            .entry((triple.subject.to_string(), triple.predicate.as_str().into()))
            .or_default()
            .insert(triple.object.to_string());
    }
    result
}

impl ClientOverlay {
    /// Compose a complete canonical catch-up with the frozen local delta using
    /// the same pure slot operator as master admission. This does not rebase,
    /// sign, validate SHACL or authorize anything. Both journals must have the
    /// same lineage, graph and shapes, with open intervals; unsupported temporal
    /// scopes refuse instead of being flattened into a successful result.
    pub fn preview(&self, canonical: &BaseJournal, shapes: &str) -> Result<OverlayPreview> {
        self.base().validate()?;
        canonical.validate()?;
        let base = self.base();
        if canonical.store_id != base.store_id
            || canonical.log_epoch != base.log_epoch
            || canonical.graph != base.graph
            || canonical.shapes_hash != base.shapes_hash
            || digest(shapes.as_bytes()) != base.shapes_hash
            || canonical.base_tx < base.base_tx
        {
            return Err(refuse("preview lineage, scope, shapes or cursor mismatch"));
        }
        if canonical.base_tx == base.base_tx && canonical != base {
            return Err(refuse("preview changed content at the frozen cursor"));
        }
        if base
            .rows()
            .iter()
            .chain(canonical.rows())
            .any(|r| r.valid_to.is_some())
        {
            return Err(refuse("preview bounded intervals are unsupported"));
        }
        let base_graph = parse(base.rows().iter().map(|r| r.triple.clone()))?;
        let current_graph = parse(canonical.rows().iter().map(|r| r.triple.clone()))?;
        let local_graph = parse(self.read().iter().cloned())?;
        let bounds = ShapeBounds::parse(shapes)?;
        let view = TripleView::new(base_graph.iter().chain(&current_graph).chain(&local_graph));
        let (b, o, t) = (
            slots(&base_graph),
            slots(&current_graph),
            slots(&local_graph),
        );
        let empty = BTreeSet::new();
        let (merged, conflicts) = merge_slots(
            &b,
            &o,
            &t,
            |slot| bounds.bound(&slot.0, &slot.1, &view),
            &empty,
        )?;
        if !conflicts.is_empty() {
            return Ok(OverlayPreview::Conflicts {
                records: conflicts
                    .into_iter()
                    .map(|c| DecisionRecord {
                        subject: c.slot.0.clone(),
                        predicate: c.slot.1.clone(),
                        max_count: c.max_count,
                        base: c.base.iter().cloned().collect(),
                        ours: c.ours.iter().cloned().collect(),
                        theirs: c.theirs.iter().cloned().collect(),
                    })
                    .collect(),
            });
        }
        let triples = merged
            .into_iter()
            .flat_map(|((s, p), values)| values.into_iter().map(move |v| format!("{s} <{p}> {v}")))
            .collect();
        Ok(OverlayPreview::Composed { triples })
    }
}
