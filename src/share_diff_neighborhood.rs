//! Bounded, one-hop context around semantically touched subjects.
use std::collections::BTreeSet;

use serde::Serialize;

use super::{Names, PackDiff, Snapshot, times};

const EDGE_LIMIT: usize = 2_000;

/// One displayed edge. Identity keys remain separate from display labels.
#[derive(Debug, Clone, Serialize)]
pub struct Edge {
    pub subject: String,
    pub subject_label: String,
    pub predicate: String,
    pub predicate_label: String,
    pub object: String,
    pub object_label: String,
    pub graph: String,
    pub graph_label: String,
    pub multiplicity: usize,
}

/// A side is independently bounded; counts disclose omitted context.
#[derive(Debug, Clone, Serialize)]
pub struct Side {
    pub edges: Vec<Edge>,
    pub total_edges: usize,
    pub truncated: bool,
}

/// Before/after context, with the same structural blank-node keys as the diff.
#[derive(Debug, Clone, Serialize)]
pub struct Neighborhood {
    pub touched: Vec<String>,
    pub before: Side,
    pub after: Side,
}

/// Include incoming and outgoing edges of touched subjects, without expanding
/// neighbors recursively or changing the semantic diff's identity policy.
pub fn neighborhood(old: &Snapshot, new: &Snapshot, diff: &PackDiff) -> Neighborhood {
    let touched: BTreeSet<_> = diff.entities.iter().map(|e| e.id.clone()).collect();
    let names = Names {
        labels: vec![&new.labels, &old.labels],
    };
    let side = |snap: &Snapshot| {
        let relevant: Vec<_> = snap
            .facts
            .iter()
            .filter(|(f, _)| touched.contains(&f.subject) || touched.contains(&f.object))
            .collect();
        let edges = relevant
            .iter()
            .take(EDGE_LIMIT)
            .map(|(f, n)| Edge {
                subject: f.subject.clone(),
                subject_label: names.entity(&f.subject, &[snap]),
                predicate: f.predicate.clone(),
                predicate_label: names.iri(&f.predicate),
                object: f.object.clone(),
                object_label: names.term(&f.object, &[snap]) + &times(**n),
                graph: f.graph.clone(),
                graph_label: if f.graph.is_empty() {
                    "default graph".into()
                } else {
                    names.term(&f.graph, &[snap])
                },
                multiplicity: **n,
            })
            .collect();
        Side {
            edges,
            total_edges: relevant.len(),
            truncated: relevant.len() > EDGE_LIMIT,
        }
    };
    let (before, after) = (side(old), side(new));
    Neighborhood {
        touched: touched.into_iter().collect(),
        before,
        after,
    }
}
