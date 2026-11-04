//! Fork promotion as a three-way merge (gap 1 of `docs/design/terminusdb-gap-analysis.md`).
//!
//! Promotion used to diff the fork against CURRENT ROOT and apply the
//! difference, so a fact written to ROOT after the fork's base looked like
//! something the fork had removed and was silently retracted. The merge here
//! takes three sides instead:
//!
//! - **base**: ROOT live as of the fork's `fork_tx` — the same #83 as-of
//!   predicate `fork_create` materialized the fork from, so base is exactly
//!   the state the fork started at;
//! - **ours**: ROOT now;
//! - **theirs**: the fork now.
//!
//! The slot semantics are the share merge's, via
//! [`crate::merge3::merge_slots`]: a slot only one side changed takes that
//! side, and only a slot both sides changed differently can conflict, on an
//! `sh:maxCount` bound that targets its subject ([`crate::merge3::ShapeBounds`];
//! emit-mode shapes do not gate). Only the term representation differs
//! (dictionary ids and `term_key` bytes rather than RDF terms).
//!
//! Cost: promotion holds base, ROOT and the fork in memory at once (three
//! present-state graphs plus their slot maps), one more graph than the old
//! two-way diff.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use rusqlite::params;

use crate::error::{Error, Result};
use crate::merge_core::{Card, Decision, FactOp, SlotInput, plan_merge};
use crate::merge3::{DecisionRecord, FocusView, ShapeBounds, Slots, merge_slots, reaches};
use crate::types::{Fact, Op, Value};

use super::{Datum, Store};

type Key = (i64, i64, Vec<u8>);

/// Decided values per conflicting slot `(entity, attribute)`, as the
/// reconcile flow resolved them (`crate::fork_merge_decisions`). Every key
/// must be a slot the merge holds in conflict.
pub type ForkResolutions = HashMap<(i64, i64), Vec<Value>>;

/// What promotion would do to ROOT, or why it cannot.
pub(super) struct ForkMerge {
    /// The two ordered steps for ROOT (spec R3, ruling B): the clean part plus
    /// a receipt `[Td, Td)` per incoming value of a decided slot, then the
    /// decisions.
    pub(super) receive: Vec<Datum>,
    pub(super) decide: Vec<Datum>,
    /// Open assertions across both steps, for the SHACL pre-check. Receipts
    /// are not here: they are current at no valid time.
    pub(super) added: Vec<Fact>,
    /// ROOT facts closed across both steps, for the deletion gate.
    pub(super) removed: Vec<Fact>,
    /// Bounded slots both sides changed incompatibly. Non-empty means the
    /// caller must write nothing.
    pub(super) conflicts: Vec<DecisionRecord>,
}

impl Store {
    /// ROOT's live triples as of transaction `tx`, with the predicate
    /// `fork_create` materializes a fork with.
    pub(super) fn root_facts_as_of(&self, tx: i64) -> Result<Vec<Fact>> {
        let mut stmt = self.conn.prepare(
            "SELECT e, a, v, tx, valid_from, valid_to, op FROM facts \
             WHERE g = ?1 AND op = 1 AND tx <= ?2 \
               AND (valid_to IS NULL OR retracted_tx > ?2) \
             ORDER BY e, a, tx",
        )?;
        Self::collect_visible_facts(&mut stmt, params![crate::schema::ROOT_GRAPH, tx])
    }

    /// Three-way merge of `theirs` (the fork) into `ours` (ROOT) over `base`.
    ///
    /// Conflict detection is `merge3`'s; with `timestamp` (the promotion
    /// instant `Td`) the write is planned by [`crate::merge_core`]: incoming
    /// one-side changes keep the fork's own valid times, closes take the
    /// fork's close time clamped to the ROOT row (aegis-0s2a), and a decided
    /// slot (from `resolved`) is written as a receipt then a live assertion
    /// from `Td`, so a winner never overlaps the value it replaces. Without
    /// `timestamp` only the conflicts are computed.
    pub(super) fn fork_merge(
        &self,
        base: Vec<Fact>,
        ours: Vec<Fact>,
        theirs: Vec<Fact>,
        resolved: Option<&ForkResolutions>,
        fork_g: i64,
        timestamp: Option<&str>,
    ) -> Result<ForkMerge> {
        let bounds = match self.get_combined_shapes()? {
            Some(shapes) => ShapeBounds::parse(&shapes)?,
            None => ShapeBounds::default(),
        };
        let view = if bounds.is_empty() {
            IdView::empty(self)
        } else {
            IdView::new(self, base.iter().chain(&ours).chain(&theirs))?
        };

        let mut values: HashMap<Key, Value> = HashMap::new();
        let mut slots = |facts: Vec<Fact>, keep: bool| {
            let mut out: Slots<(i64, i64), Vec<u8>> = BTreeMap::new();
            let mut kept: HashMap<Key, Fact> = HashMap::new();
            for f in facts {
                let k = (f.entity, f.attribute, f.value.term_key());
                out.entry((k.0, k.1)).or_default().insert(k.2.clone());
                values.entry(k.clone()).or_insert_with(|| f.value.clone());
                if keep {
                    kept.entry(k).or_insert(f);
                }
            }
            (out, kept)
        };
        let (b, _) = slots(base, false);
        let (o, ours_facts) = slots(ours, true);
        let (t, theirs_facts) = slots(theirs, true);

        let mut paths: HashMap<i64, Option<String>> = HashMap::new();
        let mut bound_of = |&(e, a): &(i64, i64)| -> Result<Option<usize>> {
            if bounds.is_empty() {
                return Ok(None);
            }
            let path = match paths.get(&a) {
                Some(p) => p.clone(),
                None => {
                    let iri = self.resolve(a)?;
                    let p = bounds.constrains(&iri).then_some(iri);
                    paths.insert(a, p.clone());
                    p
                }
            };
            match path {
                Some(p) => bounds.bound(&e, &p, &view),
                None => Ok(None),
            }
        };
        let empty = BTreeSet::new();
        let (_, held) = merge_slots(&b, &o, &t, &mut bound_of, &empty)?;

        let render = |e: i64, a: i64, set: &BTreeSet<Vec<u8>>| -> Result<Vec<String>> {
            let mut out = set
                .iter()
                .map(|k| {
                    let v = &values[&(e, a, k.clone())];
                    crate::rdf::value_to_term(self, v).map(|t| t.to_string())
                })
                .collect::<Result<Vec<_>>>()?;
            out.sort();
            Ok(out)
        };
        let mut conflicts = Vec::with_capacity(held.len());
        let mut used = 0;
        for c in &held {
            let (e, a) = *c.slot;
            if resolved.is_some_and(|r| r.contains_key(&(e, a))) {
                used += 1;
                continue;
            }
            conflicts.push(DecisionRecord {
                subject: crate::rdf::value_to_term(self, &Value::Ref(e))?.to_string(),
                predicate: self.resolve(a)?,
                max_count: c.max_count,
                base: render(e, a, c.base)?,
                ours: render(e, a, c.ours)?,
                theirs: render(e, a, c.theirs)?,
            });
        }
        if resolved.is_some_and(|r| r.len() != used) {
            return Err(Error::InvalidValue(
                "fork merge: a decision names a slot that is not in conflict; nothing was written"
                    .into(),
            ));
        }
        let mut out = ForkMerge {
            receive: Vec::new(),
            decide: Vec::new(),
            added: Vec::new(),
            removed: Vec::new(),
            conflicts,
        };
        let Some(td) = timestamp.filter(|_| out.conflicts.is_empty()) else {
            return Ok(out);
        };

        // Plan only the slots the fork changed or a decision names: an
        // unchanged or ROOT-only slot writes nothing.
        let mut inputs = BTreeMap::new();
        let mut decisions = BTreeMap::new();
        let keys: BTreeSet<(i64, i64)> =
            b.keys().chain(o.keys()).chain(t.keys()).copied().collect();
        for (e, a) in keys {
            let (bv, ov, tv) = (
                b.get(&(e, a)).unwrap_or(&empty),
                o.get(&(e, a)).unwrap_or(&empty),
                t.get(&(e, a)).unwrap_or(&empty),
            );
            let decided = resolved.and_then(|r| r.get(&(e, a)));
            if tv == bv && decided.is_none() {
                continue;
            }
            let rows = |set: &BTreeSet<Vec<u8>>, facts: &HashMap<Key, Fact>| {
                set.iter()
                    .map(|k| (k.clone(), facts[&(e, a, k.clone())].valid_from.clone()))
                    .collect::<BTreeMap<_, _>>()
            };
            let theirs_closed = if bv.difference(tv).next().is_some() {
                self.fork_closes(fork_g, e, a)?
            } else {
                BTreeMap::new()
            };
            inputs.insert(
                (e, a),
                SlotInput {
                    base: bv.clone(),
                    ours: rows(ov, &ours_facts),
                    theirs: rows(tv, &theirs_facts),
                    theirs_closed,
                    card: Card::Many(bound_of(&(e, a))?),
                },
            );
            if let Some(chosen) = decided {
                let set = chosen
                    .iter()
                    .map(|v| {
                        let k = v.term_key();
                        values.entry((e, a, k.clone())).or_insert_with(|| v.clone());
                        k
                    })
                    .collect();
                decisions.insert((e, a), Decision::Values(set));
            }
        }
        let plan = plan_merge(&inputs, &decisions, td)?;
        let fact = |e: i64, a: i64, k: &Vec<u8>, from: &str, to: Option<&String>, op| Fact {
            entity: e,
            attribute: a,
            value: values[&(e, a, k.clone())].clone(),
            tx: 0,
            valid_from: from.to_string(),
            valid_to: to.cloned(),
            op,
        };
        for (step, ops) in [
            (&mut out.receive, &plan.receive),
            (&mut out.decide, &plan.decide),
        ] {
            for ((e, a), op) in ops {
                let (e, a) = (*e, *a);
                let f = match op {
                    FactOp::Assert { value, from } => {
                        let f = fact(e, a, value, from, None, Op::Assert);
                        out.added.push(f.clone());
                        f
                    }
                    FactOp::Receipt { value, at } => fact(e, a, value, at, Some(at), Op::Assert),
                    FactOp::Close { value, at } => {
                        let row = ours_facts.get(&(e, a, value.clone())).ok_or_else(|| {
                            Error::Store(format!(
                                "fork merge: closed value on ({e}, {a}) is not a ROOT fact"
                            ))
                        })?;
                        out.removed.push(row.clone());
                        fact(e, a, value, &row.valid_from, Some(at), Op::Retract)
                    }
                };
                step.push(Datum {
                    entity: f.entity,
                    attribute: f.attribute,
                    value: f.value,
                    valid_from: f.valid_from,
                    valid_to: f.valid_to,
                    op: f.op,
                });
            }
        }
        Ok(out)
    }
}

/// A [`FocusView`] over dictionary ids: the union of base, ROOT and the fork,
/// for resolving which subjects a shape targets.
struct IdView<'s> {
    store: &'s Store,
    types: HashMap<i64, BTreeSet<i64>>,
    supers: HashMap<i64, BTreeSet<i64>>,
    out: HashSet<(i64, i64)>,
    inn: HashSet<(i64, i64)>,
}

impl<'s> IdView<'s> {
    fn empty(store: &'s Store) -> Self {
        Self {
            store,
            types: HashMap::new(),
            supers: HashMap::new(),
            out: HashSet::new(),
            inn: HashSet::new(),
        }
    }

    fn new<'f>(store: &'s Store, facts: impl Iterator<Item = &'f Fact>) -> Result<Self> {
        let rdf_type = store.lookup(crate::namespace::RDF_TYPE)?;
        let sub_of = store.lookup("http://www.w3.org/2000/01/rdf-schema#subClassOf")?;
        let mut view = Self::empty(store);
        for f in facts {
            view.out.insert((f.entity, f.attribute));
            if let Value::Ref(o) = f.value {
                view.inn.insert((o, f.attribute));
                if Some(f.attribute) == rdf_type {
                    view.types.entry(f.entity).or_default().insert(o);
                } else if Some(f.attribute) == sub_of {
                    view.supers.entry(f.entity).or_default().insert(o);
                }
            }
        }
        Ok(view)
    }
}

impl FocusView for IdView<'_> {
    type Node = i64;

    fn is_term(&self, node: &i64, term: &str) -> Result<bool> {
        Ok(crate::rdf::value_to_term(self.store, &Value::Ref(*node))?.to_string() == term)
    }

    fn instance_of(&self, node: &i64, class: &str) -> Result<bool> {
        let Some(class) = self.store.lookup(class)? else {
            return Ok(false);
        };
        let start = self.types.get(node).cloned().unwrap_or_default();
        Ok(reaches(start, &class, |c| {
            self.supers
                .get(c)
                .map(|s| s.iter().copied().collect())
                .unwrap_or_default()
        }))
    }

    fn has_out(&self, node: &i64, predicate: &str) -> Result<bool> {
        Ok(self
            .store
            .lookup(predicate)?
            .is_some_and(|p| self.out.contains(&(*node, p))))
    }

    fn has_in(&self, node: &i64, predicate: &str) -> Result<bool> {
        Ok(self
            .store
            .lookup(predicate)?
            .is_some_and(|p| self.inn.contains(&(*node, p))))
    }
}

impl Store {
    /// The fork's own close time for each value of slot `(e, a)` it closed:
    /// its latest closed row in the fork graph (aegis-jxxd). The merge core
    /// clamps it to the ROOT row it ends.
    fn fork_closes(&self, fork_g: i64, e: i64, a: i64) -> Result<BTreeMap<Vec<u8>, String>> {
        let mut rows = self.conn.prepare_cached(
            "SELECT v, valid_to FROM facts \
             WHERE g = ?1 AND e = ?2 AND a = ?3 AND op = 1 AND valid_to IS NOT NULL \
             ORDER BY retracted_tx DESC, rowid DESC",
        )?;
        let closed: Vec<(Vec<u8>, String)> = rows
            .query_map(params![fork_g, e, a], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut out = BTreeMap::new();
        for (v, valid_to) in closed {
            out.entry(Value::from_bytes(&v)?.term_key())
                .or_insert(valid_to);
        }
        Ok(out)
    }
}
