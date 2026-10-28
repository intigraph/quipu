//! The shape-aware three-way slot merge, shared by share reconnect
//! (`share_merge`, `git_merge`) and fork promotion (`store::forks`).
//!
//! A *slot* is a `(subject, predicate)` pair and its value set. A slot only
//! one side changed relative to base takes that side's values; a slot both
//! sides changed identically takes that value set. Only a slot BOTH sides
//! changed differently is merged value by value: each side's removals from base
//! are removed and each side's additions are added. Such a slot conflicts when
//! an `sh:maxCount` bound that applies to it (see [`ShapeBounds`]) would be
//! exceeded, or — for a bound of 1 — when one side deleted the base value while
//! the other replaced it. A conflicting slot is held at its base value and
//! reported, never resolved by guessing. This is the operator described in
//! `docs/paper-merge/`.
//!
//! The core is generic over the slot and value types so each caller keeps its
//! own term representation (RDF terms for shares, dictionary ids for forks)
//! while the merge semantics live in exactly one place.

use std::collections::{BTreeMap, BTreeSet};

use oxrdf::{Term, Triple};
use oxrdfio::{RdfFormat, RdfParser};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

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

/// The merged slots and the conflicts held at base.
pub(crate) type SlotMerge<'a, S, V> = (Slots<S, V>, Vec<SlotConflict<'a, S, V>>);

/// Three-way merge `ours` and `theirs` against `base`, slot by slot.
/// `bound(slot)` is the `sh:maxCount` that applies to the slot, if any; it is
/// only consulted for slots both sides changed differently. Returns the merged
/// slots (conflicting slots held at base) and the conflicts in slot order.
pub(crate) fn merge_slots<'a, S: Ord + Clone, V: Ord + Clone>(
    base: &'a Slots<S, V>,
    ours: &'a Slots<S, V>,
    theirs: &'a Slots<S, V>,
    mut bound: impl FnMut(&S) -> Result<Option<usize>>,
    empty: &'a BTreeSet<V>,
) -> Result<SlotMerge<'a, S, V>> {
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
        // Standard three-way rule: an unchanged side defers to the other, and
        // agreeing sides need no merge. A pre-existing value set is never a
        // conflict just because it is over a bound neither side touched.
        let chosen = if ov == bv || ov == tv {
            tv.clone()
        } else if tv == bv {
            ov.clone()
        } else {
            let mut values = bv.clone();
            for removed in bv.difference(ov).chain(bv.difference(tv)) {
                values.remove(removed);
            }
            values.extend(ov.difference(bv).cloned());
            values.extend(tv.difference(bv).cloned());
            let bound = bound(slot)?;
            let delete_replace =
                bound == Some(1) && !bv.is_empty() && (ov.is_empty() || tv.is_empty());
            let conflict = bound.is_some_and(|n| values.len() > n) || delete_replace;
            if conflict {
                conflicts.push(SlotConflict {
                    slot,
                    max_count: bound.unwrap_or(1),
                    base: bv,
                    ours: ov,
                    theirs: tv,
                });
                bv.clone()
            } else {
                values
            }
        };
        if !chosen.is_empty() {
            merged.insert(slot.clone(), chosen);
        }
    }
    Ok((merged, conflicts))
}

const SH: &str = "http://www.w3.org/ns/shacl#";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const RDFS_CLASS: &str = "http://www.w3.org/2000/01/rdf-schema#Class";
/// The emit-policy predicate, under both IRIs the admission path accepts.
const ON_VIOLATION: [&str; 2] = [
    "http://quipu.dev/ontology/onViolation",
    "http://quipu.dev/ns#onViolation",
];

/// What a data graph has to answer for target resolution. `N` is the caller's
/// node representation; IRIs are passed bare (no angle brackets).
pub(crate) trait FocusView {
    /// The caller's node type.
    type Node;
    /// Whether `node` is the RDF term `term` (N-Triples rendering).
    fn is_term(&self, node: &Self::Node, term: &str) -> Result<bool>;
    /// Whether `node` is a SHACL instance of `class` (`rdf:type` followed by
    /// zero or more `rdfs:subClassOf`).
    fn instance_of(&self, node: &Self::Node, class: &str) -> Result<bool>;
    /// Whether `node` is the subject of some `predicate` triple.
    fn has_out(&self, node: &Self::Node, predicate: &str) -> Result<bool>;
    /// Whether `node` is the object of some `predicate` triple.
    fn has_in(&self, node: &Self::Node, predicate: &str) -> Result<bool>;
}

#[derive(Debug, Default, Clone)]
struct Targets {
    classes: BTreeSet<String>,
    nodes: BTreeSet<String>,
    subjects_of: BTreeSet<String>,
    objects_of: BTreeSet<String>,
}

impl Targets {
    fn is_empty(&self) -> bool {
        self.classes.is_empty()
            && self.nodes.is_empty()
            && self.subjects_of.is_empty()
            && self.objects_of.is_empty()
    }

    fn extend(&mut self, other: &Targets) {
        self.classes.extend(other.classes.iter().cloned());
        self.nodes.extend(other.nodes.iter().cloned());
        self.subjects_of.extend(other.subjects_of.iter().cloned());
        self.objects_of.extend(other.objects_of.iter().cloned());
    }

    fn selects<V: FocusView + ?Sized>(&self, node: &V::Node, view: &V) -> Result<bool> {
        for t in &self.nodes {
            if view.is_term(node, t)? {
                return Ok(true);
            }
        }
        for c in &self.classes {
            if view.instance_of(node, c)? {
                return Ok(true);
            }
        }
        for p in &self.subjects_of {
            if view.has_out(node, p)? {
                return Ok(true);
            }
        }
        for p in &self.objects_of {
            if view.has_in(node, p)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[derive(Debug, Clone)]
struct PathBound {
    max: usize,
    /// `None`: the shape declares no target anywhere (a bare
    /// `[] sh:path p ; sh:maxCount n` property shape, the legacy share form),
    /// so the bound applies to every slot with the path.
    targets: Option<Targets>,
}

/// The `sh:maxCount` bounds of a shapes document, scoped to their targets.
///
/// A property shape's bound applies to the focus nodes selected by its own
/// targets and by the targets of every node shape that lists it under
/// `sh:property` (`sh:targetClass` with `rdfs:subClassOf`, `sh:targetNode`,
/// `sh:targetSubjectsOf`, `sh:targetObjectsOf`, and implicit class targets).
/// Shapes annotated `quipu:onViolation "emit"` observe and do not gate, so
/// they are ignored. A property shape with no target declarations at all
/// applies to every slot with its path.
#[derive(Debug, Default, Clone)]
pub(crate) struct ShapeBounds {
    by_path: BTreeMap<String, Vec<PathBound>>,
}

impl ShapeBounds {
    /// Parse a Turtle shapes document.
    pub(crate) fn parse(shapes: &str) -> Result<Self> {
        let triples: Vec<Triple> = RdfParser::from_format(RdfFormat::Turtle)
            .for_reader(shapes.as_bytes())
            .map(|q| {
                q.map(Triple::from)
                    .map_err(|e| Error::InvalidValue(format!("shapes.ttl parse: {e}")))
            })
            .collect::<Result<_>>()?;
        let mut props: BTreeMap<String, Vec<(String, Term)>> = BTreeMap::new();
        let mut parents: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for t in &triples {
            let subject = t.subject.to_string();
            if t.predicate.as_str() == format!("{SH}property") {
                parents
                    .entry(t.object.to_string())
                    .or_default()
                    .push(subject.clone());
            }
            props
                .entry(subject)
                .or_default()
                .push((t.predicate.as_str().to_string(), t.object.clone()));
        }
        let get = |node: &str, pred: &str| -> Vec<&Term> {
            props
                .get(node)
                .map(|ps| {
                    ps.iter()
                        .filter(|(p, _)| p == pred)
                        .map(|(_, o)| o)
                        .collect()
                })
                .unwrap_or_default()
        };
        let is_emit = |node: &str| {
            let values: Vec<&Term> = ON_VIOLATION.iter().flat_map(|p| get(node, p)).collect();
            !values.is_empty()
                && values.iter().all(|v| {
                    matches!(v, Term::Literal(l)
                        if l.language().is_none()
                            && l.datatype() == oxrdf::vocab::xsd::STRING
                            && l.value() == "emit")
                })
        };
        let targets_of = |node: &str| {
            let iri = |t: &Term| match t {
                Term::NamedNode(n) => Some(n.as_str().to_string()),
                _ => None,
            };
            let mut out = Targets::default();
            out.classes.extend(
                get(node, &format!("{SH}targetClass"))
                    .into_iter()
                    .filter_map(iri),
            );
            out.nodes.extend(
                get(node, &format!("{SH}targetNode"))
                    .into_iter()
                    .map(ToString::to_string),
            );
            out.subjects_of.extend(
                get(node, &format!("{SH}targetSubjectsOf"))
                    .into_iter()
                    .filter_map(iri),
            );
            out.objects_of.extend(
                get(node, &format!("{SH}targetObjectsOf"))
                    .into_iter()
                    .filter_map(iri),
            );
            let implicit = get(node, RDF_TYPE)
                .iter()
                .any(|t| matches!(t, Term::NamedNode(n) if n.as_str() == RDFS_CLASS));
            if implicit
                && let Some(class) = node.strip_prefix('<').and_then(|n| n.strip_suffix('>'))
            {
                out.classes.insert(class.to_string());
            }
            out
        };

        let mut by_path: BTreeMap<String, Vec<PathBound>> = BTreeMap::new();
        for node in props.keys() {
            let Some(max) = get(node, &format!("{SH}maxCount"))
                .into_iter()
                .find_map(|t| match t {
                    Term::Literal(v) => v.value().parse::<usize>().ok(),
                    _ => None,
                })
            else {
                continue;
            };
            let Some(path) = get(node, &format!("{SH}path"))
                .into_iter()
                .find_map(|t| match t {
                    Term::NamedNode(p) => Some(p.as_str().to_string()),
                    _ => None,
                })
            else {
                continue;
            };
            if is_emit(node) {
                continue;
            }
            let owners = parents.get(node).cloned().unwrap_or_default();
            let gating: Vec<&String> = owners.iter().filter(|o| !is_emit(o)).collect();
            if !owners.is_empty() && gating.is_empty() {
                continue;
            }
            let mut targets = targets_of(node);
            for owner in gating {
                targets.extend(&targets_of(owner));
            }
            by_path.entry(path).or_default().push(PathBound {
                max,
                targets: (!targets.is_empty()).then_some(targets),
            });
        }
        Ok(Self { by_path })
    }

    /// Whether the document has no gating bound at all.
    pub(crate) fn is_empty(&self) -> bool {
        self.by_path.is_empty()
    }

    /// Whether any gating bound names `path`.
    pub(crate) fn constrains(&self, path: &str) -> bool {
        self.by_path.contains_key(path)
    }

    /// The tightest `sh:maxCount` on `path` that applies to `node`, if any.
    pub(crate) fn bound<V: FocusView + ?Sized>(
        &self,
        node: &V::Node,
        path: &str,
        view: &V,
    ) -> Result<Option<usize>> {
        let mut best: Option<usize> = None;
        for b in self.by_path.get(path).into_iter().flatten() {
            let applies = match &b.targets {
                None => true,
                Some(t) => t.selects(node, view)?,
            };
            if applies {
                best = Some(best.map_or(b.max, |m| m.min(b.max)));
            }
        }
        Ok(best)
    }
}

const RDFS_SUBCLASS_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";

/// Walk `rdf:type` then `rdfs:subClassOf` edges from `start` looking for
/// `goal`. Shared by both views; `next` yields a node's superclasses.
pub(crate) fn reaches<K: Ord + Clone>(
    start: impl IntoIterator<Item = K>,
    goal: &K,
    mut next: impl FnMut(&K) -> Vec<K>,
) -> bool {
    let mut seen = BTreeSet::new();
    let mut stack: Vec<K> = start.into_iter().collect();
    while let Some(k) = stack.pop() {
        if &k == goal {
            return true;
        }
        if seen.insert(k.clone()) {
            stack.extend(next(&k));
        }
    }
    false
}

/// A [`FocusView`] over RDF triples (the union of a merge's three sides).
/// Nodes are N-Triples renderings, as slot subjects are.
#[derive(Default)]
pub(crate) struct TripleView {
    types: BTreeMap<String, BTreeSet<String>>,
    supers: BTreeMap<String, BTreeSet<String>>,
    out: BTreeSet<(String, String)>,
    inn: BTreeSet<(String, String)>,
}

impl TripleView {
    pub(crate) fn new<'a>(triples: impl IntoIterator<Item = &'a Triple>) -> Self {
        let mut view = Self::default();
        for t in triples {
            let s = t.subject.to_string();
            let p = t.predicate.as_str();
            if let Term::NamedNode(o) = &t.object {
                if p == RDF_TYPE {
                    view.types
                        .entry(s.clone())
                        .or_default()
                        .insert(o.as_str().into());
                } else if p == RDFS_SUBCLASS_OF
                    && let Some(class) = s.strip_prefix('<').and_then(|c| c.strip_suffix('>'))
                {
                    view.supers
                        .entry(class.to_string())
                        .or_default()
                        .insert(o.as_str().into());
                }
            }
            view.inn.insert((t.object.to_string(), p.to_string()));
            view.out.insert((s, p.to_string()));
        }
        view
    }
}

impl FocusView for TripleView {
    type Node = String;

    fn is_term(&self, node: &String, term: &str) -> Result<bool> {
        Ok(node == term)
    }

    fn instance_of(&self, node: &String, class: &str) -> Result<bool> {
        let start = self.types.get(node).cloned().unwrap_or_default();
        Ok(reaches(start, &class.to_string(), |c| {
            self.supers
                .get(c)
                .map(|s| s.iter().cloned().collect())
                .unwrap_or_default()
        }))
    }

    fn has_out(&self, node: &String, predicate: &str) -> Result<bool> {
        Ok(self.out.contains(&(node.clone(), predicate.to_string())))
    }

    fn has_in(&self, node: &String, predicate: &str) -> Result<bool> {
        Ok(self.inn.contains(&(node.clone(), predicate.to_string())))
    }
}
