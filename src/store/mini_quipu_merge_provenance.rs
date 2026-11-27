//! Slice M4: the mini-Quipu writer's provenance, carried through the fork
//! merge's carriage reader (`merge_carriage::carry_source`). It reads the
//! mini-Quipu file before the home writes, never a request header, and
//! writes nothing.
use crate::{Result, Store};

/// One written slot of a mini-Quipu merge and its origin transactions.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MergeRow {
    /// Member graph IRI.
    pub graph: String,
    /// `(<subject>, predicate)`, as in the partial-merge receipt.
    pub slot: (String, String),
    /// Mini-Quipu `local_tx` values that wrote this slot's incoming values.
    pub origin_txs: Vec<i64>,
}

/// Slice M4: the origins and carried provenance of every slot the plans
/// write, read from the mini-Quipu file before the home writes. Nothing here
/// reads the merge request's headers or writes a row.
pub(super) fn carriage(
    home: &Store,
    mini: &Store,
    plans: &[(i64, i64, String, crate::store::forks_merge::ForkMerge)],
) -> Result<(Vec<MergeRow>, crate::merge_carriage::Carriage)> {
    let mut rows = Vec::new();
    let mut all = crate::merge_carriage::Carriage {
        state: crate::merge_carriage::OBSERVED.into(),
        transactions: std::collections::BTreeMap::new(),
    };
    for (_, mg, graph, plan) in plans {
        let mut written: std::collections::BTreeMap<_, std::collections::BTreeSet<String>> =
            std::collections::BTreeMap::new();
        for d in plan.receive.iter().chain(&plan.decide) {
            let slot = (
                format!("<{}>", home.resolve(d.entity)?),
                home.resolve(d.attribute)?,
            );
            let term = crate::rdf::value_to_term(home, &d.value)?.to_string();
            written.entry(slot).or_default().insert(term);
        }
        let (origins, carried) = crate::merge_carriage::carry_source(mini, *mg, &written)?;
        all.transactions.extend(carried.transactions);
        rows.extend(origins.into_iter().map(|(slot, origin_txs)| MergeRow {
            graph: graph.clone(),
            slot,
            origin_txs,
        }));
    }
    Ok((rows, all))
}
