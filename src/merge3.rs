//! The shape-aware three-way slot merge, shared by share reconnect
//! (`share_merge`, `git_merge`) and fork promotion (`store::forks`).
//!
//! A *slot* is a `(subject, predicate)` pair and its value set. Given a base and
//! two sides, each side's removals from base are removed and each side's
//! additions are added. A slot whose predicate carries an `sh:maxCount` bound
//! conflicts when the merged set would exceed it, or — for a bound of 1 — when
//! one side deleted the base value while the other replaced it. A conflicting
//! slot is held at its base value and reported, never resolved by guessing.
//! This is the operator described in `docs/paper-merge/`.
//!
//! The core is generic over the slot and value types so each caller keeps its
//! own term representation (RDF terms for shares, dictionary ids for forks)
//! while the merge semantics live in exactly one place.

use std::collections::{BTreeMap, BTreeSet};

use oxrdf::{Term, Triple};
use oxrdfio::{RdfFormat, RdfParser};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

const SH_PATH: &str = "http://www.w3.org/ns/shacl#path";
const SH_MAX_COUNT: &str = "http://www.w3.org/ns/shacl#maxCount";

/// One conflicting slot, as reported to an operator. Terms are N-Triples
/// renderings (`<iri>`, `"literal"`, `_:b`); `predicate` is the bare IRI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionRecord {
    /// The slot's subject term.
    pub subject: String,
    /// The slot's predicate IRI.
    pub predicate: String,
    /// The `sh:maxCount` bound the slot conflicted on.
    pub max_count: usize,
    /// Values at the merge base.
    pub base: Vec<String>,
    /// Values on the local side (ROOT).
    pub ours: Vec<String>,
    /// Values on the incoming side (the share, or the fork).
    pub theirs: Vec<String>,
}

/// A slot the merge held at base, with each side's value set.
pub(crate) struct SlotConflict<'a, S, V> {
    pub(crate) slot: &'a S,
    pub(crate) max_count: usize,
    pub(crate) base: &'a BTreeSet<V>,
    pub(crate) ours: &'a BTreeSet<V>,
    pub(crate) theirs: &'a BTreeSet<V>,
}

/// Slot values per slot.
pub(crate) type Slots<S, V> = BTreeMap<S, BTreeSet<V>>;

/// Three-way merge `ours` and `theirs` against `base`, slot by slot.
/// `bound(slot)` is the slot's `sh:maxCount`, if any. Returns the merged slots
/// (conflicting slots held at base) and the conflicts in slot order.
pub(crate) fn merge_slots<'a, S: Ord + Clone, V: Ord + Clone>(
    base: &'a Slots<S, V>,
    ours: &'a Slots<S, V>,
    theirs: &'a Slots<S, V>,
    bound: impl Fn(&S) -> Option<usize>,
    empty: &'a BTreeSet<V>,
) -> (Slots<S, V>, Vec<SlotConflict<'a, S, V>>) {
    let slots: BTreeSet<&'a S> = base
        .keys()
        .chain(ours.keys())
        .chain(theirs.keys())
        .collect();
    let mut merged = BTreeMap::new();
    let mut conflicts = Vec::new();
    for slot in slots {
        let bv = base.get(slot).unwrap_or(empty);
        let ov = ours.get(slot).unwrap_or(empty);
        let tv = theirs.get(slot).unwrap_or(empty);
        let mut values = bv.clone();
        for removed in bv.difference(ov).chain(bv.difference(tv)) {
            values.remove(removed);
        }
        values.extend(ov.difference(bv).cloned());
        values.extend(tv.difference(bv).cloned());
        let bound = bound(slot);
        let delete_replace = bound == Some(1)
            && !bv.is_empty()
            && ((ov.is_empty() && !tv.is_empty() && tv != bv)
                || (tv.is_empty() && !ov.is_empty() && ov != bv));
        let conflict = bound.is_some_and(|n| values.len() > n) || delete_replace;
        let chosen = if conflict { bv.clone() } else { values };
        if conflict {
            conflicts.push(SlotConflict {
                slot,
                max_count: bound.unwrap_or(1),
                base: bv,
                ours: ov,
                theirs: tv,
            });
        }
        if !chosen.is_empty() {
            merged.insert(slot.clone(), chosen);
        }
    }
    (merged, conflicts)
}

/// `sh:path` IRI -> `sh:maxCount` for every property shape in `shapes`
/// (Turtle) that states both. Target declarations are not consulted: a
/// cardinality bound on a path applies to every slot with that predicate.
pub(crate) fn max_counts(shapes: &str) -> Result<BTreeMap<String, usize>> {
    let triples: Vec<Triple> = RdfParser::from_format(RdfFormat::Turtle)
        .for_reader(shapes.as_bytes())
        .map(|q| {
            q.map(Triple::from)
                .map_err(|e| Error::InvalidValue(format!("shapes.ttl parse: {e}")))
        })
        .collect::<Result<_>>()?;
    let mut paths = BTreeMap::new();
    let mut bounds = BTreeMap::new();
    for t in triples {
        let node = t.subject.to_string();
        if t.predicate.as_str() == SH_PATH
            && let Term::NamedNode(path) = &t.object
        {
            paths.insert(node.clone(), path.as_str().to_string());
        }
        if t.predicate.as_str() == SH_MAX_COUNT
            && let Term::Literal(v) = &t.object
            && let Ok(n) = v.value().parse()
        {
            bounds.insert(node, n);
        }
    }
    Ok(bounds
        .into_iter()
        .filter_map(|(node, n)| paths.get(&node).map(|p| (p.clone(), n)))
        .collect())
}
