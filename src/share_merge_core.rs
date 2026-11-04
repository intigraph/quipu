//! Lower verified share graphs to the common temporal merge plan.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::{Error, Result};
use crate::merge_core::{Card, Decision, FactOp, MergePlan, SlotInput, plan_merge};
use crate::store::{Datum, Store};
use crate::types::Op;

use super::{Graph, by_slot, parse_graph};

pub(crate) type Slot = (String, String);
pub(crate) type Plan = MergePlan<Slot, String>;

/// All three complete graphs; the incoming share supplies authored time.
pub(crate) struct Inputs<'a> {
    pub base: &'a Graph,
    pub ours: &'a Graph,
    pub theirs: &'a Graph,
    pub shapes: &'a str,
    pub authored_at: &'a str,
}

fn row_start(store: &Store, slot: &Slot, value: &str) -> Result<String> {
    let subject = slot
        .0
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(&slot.0);
    let missing = || {
        Error::Store(format!(
            "share merge: ROOT provenance missing for {slot:?} {value}"
        ))
    };
    let e = store.lookup(subject)?.ok_or_else(missing)?;
    let a = store.lookup(&slot.1)?.ok_or_else(missing)?;
    let mut starts = Vec::new();
    for f in store.entity_facts(e)? {
        if f.attribute == a && crate::rdf::value_to_term(store, &f.value)?.to_string() == value {
            starts.push(f.valid_from);
        }
    }
    starts.into_iter().max().ok_or_else(missing)
}

pub(crate) fn plan(
    store: &Store,
    input: &Inputs<'_>,
    decisions: &BTreeMap<Slot, Decision<String>>,
    td: &str,
) -> Result<Plan> {
    let (b, o, t) = (
        by_slot(input.base),
        by_slot(input.ours),
        by_slot(input.theirs),
    );
    let bounds = crate::merge3::ShapeBounds::parse(input.shapes)?;
    let view =
        crate::merge3::TripleView::new(input.base.iter().chain(input.ours).chain(input.theirs));
    let keys: BTreeSet<_> = b.keys().chain(o.keys()).chain(t.keys()).cloned().collect();
    let empty = BTreeSet::new();
    let mut slots = BTreeMap::new();
    for slot in keys {
        let bv = b.get(&slot).unwrap_or(&empty);
        let ov = o.get(&slot).unwrap_or(&empty);
        let tv = t.get(&slot).unwrap_or(&empty);
        slots.insert(
            slot.clone(),
            SlotInput {
                base: bv.clone(),
                ours: ov
                    .iter()
                    .map(|v| Ok((v.clone(), row_start(store, &slot, v)?)))
                    .collect::<Result<_>>()?,
                theirs: tv
                    .iter()
                    .map(|v| (v.clone(), input.authored_at.into()))
                    .collect(),
                // The share anchor supplies removal time too (spec §3.1/N1).
                // The core clamps it against the ROOT row before closing.
                theirs_closed: bv
                    .difference(tv)
                    .map(|v| (v.clone(), input.authored_at.to_string()))
                    .collect(),
                card: Card::Many(bounds.bound(&slot.0, &slot.1, &view)?),
            },
        );
    }
    plan_merge(&slots, decisions, td)
}

fn datums(store: &Store, ops: &[(Slot, FactOp<String>)]) -> Result<Vec<Datum>> {
    let mut out = Vec::new();
    for (slot, op) in ops {
        let (value, from, to, op) = match op {
            FactOp::Assert { value, from } => (value, from.clone(), None, Op::Assert),
            FactOp::Receipt { value, at } => (value, at.clone(), Some(at.clone()), Op::Assert),
            FactOp::Close { value, at } => (
                value,
                row_start(store, slot, value)?,
                Some(at.clone()),
                Op::Retract,
            ),
        };
        let graph = parse_graph(
            &format!("{} <{}> {} .", slot.0, slot.1, value),
            "merge operation",
        )?;
        if graph.len() != 1 {
            return Err(Error::InvalidValue(
                "share merge: operation is not one triple".into(),
            ));
        }
        let triple = graph.into_iter().next().expect("one triple");
        out.push(Datum {
            entity: crate::rdf::intern_subject(store, &triple.subject)?,
            attribute: store.intern(triple.predicate.as_str())?,
            value: crate::rdf::term_to_value(store, &triple.object)?,
            valid_from: from,
            valid_to: to,
            op,
        });
    }
    Ok(out)
}

/// Preserve normal transaction gates on both ordered steps. The caller holds
/// the outer writer reservation, including graph/staleness reads and prechecks.
pub(crate) fn persist(
    store: &mut Store,
    plan: &Plan,
    td: &str,
    actor: Option<&str>,
    source: &str,
) -> Result<i64> {
    if plan.held != 0 {
        return Err(Error::InvalidValue(
            "share merge: unresolved slots; nothing was written".into(),
        ));
    }
    // Lower both steps before receive changes the ROOT provenance view.
    let receive = datums(store, &plan.receive)?;
    let decide = datums(store, &plan.decide)?;
    let mut tx = 0;
    for (step, data) in [("receive", receive), ("decide", decide)] {
        if !data.is_empty() {
            tx = store.transact(&data, td, actor, Some(&format!("{source};step={step}")))?;
        }
    }
    // Keep the existing successful no-op transaction/receipt contract.
    if tx == 0 {
        tx = store.transact(&[], td, actor, Some(source))?;
    }
    Ok(tx)
}

/// Reserve the writer before reading ROOT or checking decision bindings.
pub(crate) fn atomic<T>(
    store: &mut Store,
    body: impl FnOnce(&mut Store) -> Result<T>,
) -> Result<T> {
    store.ensure_ownership_writable()?;
    store.conn.execute_batch("SAVEPOINT quipu_share_merge")?;
    let result = (|| {
        store
            .conn
            .execute("UPDATE transactions SET id=id WHERE 0", [])?;
        body(store)
    })();
    match result {
        Ok(value) => {
            if let Err(e) = crate::store::release::release(&store.conn, "quipu_share_merge") {
                let _ = crate::store::release::rollback_release(&store.conn, "quipu_share_merge");
                store.term_cache.borrow_mut().clear_persistent();
                store.invalidate_read_model();
                return Err(e.into());
            }
            Ok(value)
        }
        Err(e) => {
            let _ = crate::store::release::rollback_release(&store.conn, "quipu_share_merge");
            store.term_cache.borrow_mut().clear_persistent();
            store.invalidate_read_model();
            Err(e)
        }
    }
}

#[cfg(test)]
#[path = "share_merge_core_tests.rs"]
mod tests;
