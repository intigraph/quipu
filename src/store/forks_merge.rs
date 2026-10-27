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
//! The slot semantics (and `sh:maxCount` conflict rules) are the share
//! merge's, via [`crate::merge3::merge_slots`]; only the term representation
//! differs (dictionary ids and `term_key` bytes rather than RDF terms).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rusqlite::params;

use crate::error::{Error, Result};
use crate::merge3::{DecisionRecord, Slots, max_counts, merge_slots};
use crate::types::{Fact, Value};

use super::Store;

type Key = (i64, i64, Vec<u8>);

/// What promotion would do to ROOT, or why it cannot.
pub(super) struct ForkMerge {
    /// Fork facts to assert into ROOT.
    pub(super) added: Vec<Fact>,
    /// ROOT facts to retract.
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

    /// Three-way merge of `theirs` (the fork) into `ours` (ROOT) over `base`,
    /// returned as the delta to apply to ROOT.
    pub(super) fn fork_merge(
        &self,
        base: Vec<Fact>,
        ours: Vec<Fact>,
        theirs: Vec<Fact>,
    ) -> Result<ForkMerge> {
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
        let (o, mut ours_facts) = slots(ours, true);
        let (t, mut theirs_facts) = slots(theirs, true);

        let bounds = match self.get_combined_shapes()? {
            Some(shapes) => max_counts(&shapes)?,
            None => BTreeMap::new(),
        };
        let mut attr_bound: HashMap<i64, Option<usize>> = HashMap::new();
        if !bounds.is_empty() {
            for &(_, a) in b.keys().chain(o.keys()).chain(t.keys()) {
                if let std::collections::hash_map::Entry::Vacant(slot) = attr_bound.entry(a) {
                    slot.insert(bounds.get(&self.resolve(a)?).copied());
                }
            }
        }
        let empty = BTreeSet::new();
        let (merged, held) = merge_slots(
            &b,
            &o,
            &t,
            |slot| attr_bound.get(&slot.1).copied().flatten(),
            &empty,
        );

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
        for c in &held {
            let (e, a) = *c.slot;
            conflicts.push(DecisionRecord {
                subject: crate::rdf::value_to_term(self, &Value::Ref(e))?.to_string(),
                predicate: self.resolve(a)?,
                max_count: c.max_count,
                base: render(e, a, c.base)?,
                ours: render(e, a, c.ours)?,
                theirs: render(e, a, c.theirs)?,
            });
        }
        if !conflicts.is_empty() {
            return Ok(ForkMerge {
                added: Vec::new(),
                removed: Vec::new(),
                conflicts,
            });
        }

        // With no conflict, every merged value ROOT lacks came from the fork,
        // and every ROOT value the merge dropped is a ROOT fact.
        let mut added = Vec::new();
        let mut removed = Vec::new();
        let slot_keys: BTreeSet<&(i64, i64)> = o.keys().chain(merged.keys()).collect();
        for &(e, a) in slot_keys {
            let mv = merged.get(&(e, a)).unwrap_or(&empty);
            let ov = o.get(&(e, a)).unwrap_or(&empty);
            for k in mv.difference(ov) {
                added.push(theirs_facts.remove(&(e, a, k.clone())).ok_or_else(|| {
                    Error::Store(format!(
                        "fork merge: added value on ({e}, {a}) is not a fork fact"
                    ))
                })?);
            }
            for k in ov.difference(mv) {
                removed.push(ours_facts.remove(&(e, a, k.clone())).ok_or_else(|| {
                    Error::Store(format!(
                        "fork merge: removed value on ({e}, {a}) is not a ROOT fact"
                    ))
                })?);
            }
        }
        Ok(ForkMerge {
            added,
            removed,
            conflicts,
        })
    }
}
