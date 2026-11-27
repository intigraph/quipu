//! Merge carriage of write provenance (write-provenance-persistence.md §5.1,
//! slice P6).
//!
//! A merge's own `receive` and `decide` transactions take their declaration
//! and authentication rows from the APPLIER's request, through the normal
//! write pipeline. The incoming writer's provenance is different data: it is
//! about the origin, never identity on the home transaction. It is read here
//! from the source — for a fork, the fork's own transactions in this store —
//! and recorded in the merge receipt: each decision row names the origin
//! transactions of its incoming facts, and the receipt's `carried` object maps
//! every origin transaction to its `actor`, `source` and stored declarations.
//!
//! Nothing here reads the merge request's headers, so a merge call with forged
//! `X-Quipu-*` headers changes no carried field. Nothing here writes a home
//! `transaction_declarations` or `transaction_auth` row, and the origin's
//! `transaction_auth` is never carried: it is local credential evidence.
//!
//! A share has no transactions, so its carriage is `unknown-no-transactions`,
//! never an empty "nothing was declared".

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::merge_core::FactOp;
use crate::merge_lists::Slot;
use crate::merge_session::{Path, Prepared, Value};
use crate::store::Store;
use crate::write_provenance_read::ReadState;

/// The carriage state of a whole merge.
pub(crate) const OBSERVED: &str = "observed";
/// The incoming side has no transactions to read (a share).
pub(crate) const UNKNOWN_NO_TRANSACTIONS: &str = "unknown-no-transactions";

/// One origin transaction's carried provenance, verbatim from the source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Carried {
    pub actor: Option<String>,
    pub source: Option<String>,
    /// The origin's stored declarations; `None` when `state` is unknown.
    pub declarations: Option<BTreeMap<String, String>>,
    /// `observed`, `unknown-no-row` or `unknown-no-table`, as on read (P3).
    pub state: String,
}

/// The receipt's `carried` object: per origin transaction, keyed by its id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Carriage {
    pub state: String,
    pub transactions: BTreeMap<String, Carried>,
}

fn term_of(op: &FactOp<Value>) -> Option<&str> {
    let value = match op {
        FactOp::Assert { value, .. }
        | FactOp::Close { value, .. }
        | FactOp::Receipt { value, .. } => value,
    };
    match value {
        Value::Term(term) => Some(term),
        // A list slot's cells are separate subjects; its origins are not
        // traced per fact here, so the row names none.
        Value::List(_) => None,
    }
}

/// The fork transactions that wrote (asserted or closed) one of the values
/// this slot's plan writes, from the fork graph's own history.
fn slot_origins(
    store: &Store,
    fork_graph: i64,
    slot: &Slot,
    terms: &BTreeSet<String>,
) -> Result<Vec<i64>> {
    let subject = slot.0.strip_prefix('<').and_then(|s| s.strip_suffix('>'));
    let (Some(Some(e)), Some(a)) = (
        subject.map(|s| store.lookup(s)).transpose()?,
        store.lookup(&slot.1)?,
    ) else {
        return Ok(Vec::new());
    };
    let mut origins = BTreeSet::new();
    for fact in store.entity_facts_in_graph(e, fork_graph)? {
        if fact.attribute == a
            && terms.contains(
                crate::rdf::value_to_term(store, &fact.value)?
                    .to_string()
                    .as_str(),
            )
        {
            origins.insert(fact.tx);
        }
    }
    Ok(origins.into_iter().collect())
}

/// Per-slot origin transactions and the carried provenance of each, read from
/// the source. Called before the merge writes anything, so the home receive
/// and decide transactions can never be their own origin.
pub(crate) fn carry(
    store: &Store,
    prepared: &Prepared,
) -> Result<(BTreeMap<Slot, Vec<i64>>, Carriage)> {
    if prepared.request.path != Path::Fork {
        let carriage = Carriage {
            state: UNKNOWN_NO_TRANSACTIONS.into(),
            transactions: BTreeMap::new(),
        };
        return Ok((BTreeMap::new(), carriage));
    }
    let fork_graph = store.fork_graph_for_read(&prepared.request.input_id)?;
    let written = prepared
        .plan
        .slots
        .iter()
        .map(|(slot, plan)| {
            let terms = plan
                .receive
                .iter()
                .chain(&plan.decide)
                .filter_map(term_of)
                .map(str::to_owned)
                .collect();
            (slot.clone(), terms)
        })
        .collect();
    carry_source(store, fork_graph, &written)
}

/// The one carriage reader for every merge path: for each slot, the source
/// transactions in `source_graph` that wrote one of the `terms` (rendered
/// RDF terms) the plan writes, and each origin's carried provenance read from
/// `source` verbatim. A fork reads its fork graph in the home store; a
/// mini-Quipu (slice M4) reads its member graph in the mini-Quipu file. It
/// writes nothing and never reads a request header.
pub(crate) fn carry_source(
    source: &Store,
    source_graph: i64,
    written: &BTreeMap<Slot, BTreeSet<String>>,
) -> Result<(BTreeMap<Slot, Vec<i64>>, Carriage)> {
    let mut origins = BTreeMap::new();
    let mut transactions = BTreeMap::new();
    for (slot, terms) in written {
        if terms.is_empty() {
            continue;
        }
        let txs = slot_origins(source, source_graph, slot, terms)?;
        for &tx in &txs {
            if let std::collections::btree_map::Entry::Vacant(entry) =
                transactions.entry(tx.to_string())
            {
                entry.insert(carried(source, tx)?);
            }
        }
        if !txs.is_empty() {
            origins.insert(slot.clone(), txs);
        }
    }
    let carriage = Carriage {
        state: OBSERVED.into(),
        transactions,
    };
    Ok((origins, carriage))
}

fn carried(store: &Store, tx: i64) -> Result<Carried> {
    let transaction = store.get_transaction(tx)?;
    let (declarations, state) = store.declaration_row(tx)?;
    Ok(Carried {
        actor: transaction.as_ref().and_then(|t| t.actor.clone()),
        source: transaction.and_then(|t| t.source),
        declarations,
        state: ReadState::as_str(state).into(),
    })
}

#[cfg(test)]
#[path = "merge_carriage_tests.rs"]
mod tests;
