//! Independent original list prototype, retained as test support only.
use super::*;
#[path = "merge_lists_admission.rs"]
mod admission;

/// A ROOT list component extracted only after the complete profile was read.
/// The private fields prevent an adapter from passing a full graph to closing.
pub(crate) struct Original {
    slot: Slot,
    graph: Graph,
    elements: Option<Vec<String>>,
}

fn originals(graph: &Graph, values: &BTreeMap<Slot, Option<Value>>) -> BTreeMap<Slot, Original> {
    let owners: BTreeMap<_, _> = values
        .iter()
        .flat_map(|(slot, value)| {
            value
                .iter()
                .flat_map(move |value| value.cells.iter().map(move |cell| (cell.as_str(), slot)))
        })
        .collect();
    let mut out: BTreeMap<_, _> = values
        .iter()
        .map(|(slot, value)| {
            (
                slot.clone(),
                Original {
                    slot: slot.clone(),
                    graph: Graph::new(),
                    elements: value.as_ref().map(|value| value.elements.clone()),
                },
            )
        })
        .collect();
    // One pass, including all slots: no per-slot scan of the full context.
    for triple in graph {
        let subject = triple.subject.to_string();
        let head = (subject.clone(), triple.predicate.as_str().to_owned());
        if let Some(original) = out.get_mut(&head) {
            original.graph.insert(triple.clone());
        } else if let Some(slot) = owners.get(subject.as_str())
            && let Some(original) = out.get_mut(*slot)
        {
            original.graph.insert(triple.clone());
        }
    }
    out
}

/// Cell-label-independent identity, also distinguishing absence from rdf:nil.
pub(crate) fn identity(elements: &Option<Vec<String>>) -> Result<String> {
    Ok(crate::share::sha256(
        &serde_json::to_vec(&("quipu-list-value/v1", elements))
            .map_err(|e| Error::Serialization(e.to_string()))?,
    ))
}

/// One whole-list plan: authored and decision clocks are supplied by the caller.
/// Sequence synthesis is a derived decision at Td; its incoming sequence is
/// retained as an empty-interval receipt, never spliced link by link.
pub(crate) struct Clocks<'a> {
    pub ours: &'a str,
    pub incoming: &'a str,
    pub decision: &'a str,
}
pub(crate) fn plan(
    slot: &Slot,
    base: &Option<Vec<String>>,
    ours: &Option<Vec<String>>,
    theirs: &Option<Vec<String>>,
    mode: Mode,
    decision: Option<&crate::merge_core::Decision<Vec<String>>>,
    clocks: Clocks<'_>,
) -> Result<crate::merge_core::SlotPlan<Vec<String>>> {
    use crate::merge_core::{Card, Decision, SlotInput, plan_slot};
    let values = |v: &Option<Vec<String>>| v.iter().cloned().collect();
    let rows = |v: &Option<Vec<String>>, time: &str| {
        v.iter().cloned().map(|v| (v, time.to_owned())).collect()
    };
    // Validate terms and bounds even for a convergent/no-op plan.
    for value in [base, ours, theirs] {
        lower(slot, value)?;
    }
    let input = SlotInput {
        base: values(base),
        ours: rows(ours, clocks.ours),
        theirs: rows(theirs, clocks.incoming),
        theirs_closed: base
            .iter()
            .cloned()
            .map(|v| (v, clocks.incoming.to_owned()))
            .collect(),
        card: Card::ONE,
    };
    let synthesized = if decision.is_none() && mode == Mode::Sequence {
        match merge(base, ours, theirs, mode)? {
            Some(chosen) if ours != base && theirs != base && ours != theirs => {
                Some(Decision::Values(values(&chosen)))
            }
            _ => None,
        }
    } else {
        None
    };
    plan_slot(&input, decision.or(synthesized.as_ref()), clocks.decision)
}

/// Expand a logical list operation as a whole head+cell operation group.
/// Closing ROOT uses ROOT's original cell identities, not newly generated ones.
pub(crate) fn lower_ops(
    slot: &Slot,
    original_ours: &Original,
    ops: &[crate::merge_core::FactOp<Vec<String>>],
) -> Result<Vec<crate::merge_core::FactOp<oxrdf::Triple>>> {
    use crate::merge_core::FactOp;
    if &original_ours.slot != slot {
        return Err(invalid("original list belongs to a different slot"));
    }
    let mut result = Vec::new();
    for op in ops {
        let graph = match op {
            FactOp::Close { value, .. } => {
                if original_ours.elements.as_ref() != Some(value) {
                    return Err(invalid("close does not match the original list value"));
                }
                original_ours.graph.clone()
            }
            FactOp::Assert { value, .. } | FactOp::Receipt { value, .. } => {
                lower(slot, &Some(value.clone()))?
            }
        };
        let mut triples: Vec<_> = graph.into_iter().collect();
        triples.sort_by_key(ToString::to_string);
        for value in triples {
            result.push(match op {
                FactOp::Close { at, .. } => FactOp::Close {
                    value,
                    at: at.clone(),
                },
                FactOp::Assert { from, .. } => FactOp::Assert {
                    value,
                    from: from.clone(),
                },
                FactOp::Receipt { at, .. } => FactOp::Receipt {
                    value,
                    at: at.clone(),
                },
            });
        }
    }
    Ok(result)
}
