//! Typed RDF-list values. Cells are representation, never mergeable set members.
//! Callers supply admitted list slots/modes explicitly; no inference from labels
//! or sh:group. A missing property differs from the empty rdf:nil sequence.
use crate::error::{Error, Result};
use crate::share_merge::{Graph, parse_graph};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const MAX_CELLS: usize = 4096;
const MAX_DIFF: usize = 1_000_000;
pub(crate) type Slot = (String, String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Mode {
    Whole,
    Sequence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Value {
    pub(crate) elements: Vec<String>,
    pub(crate) cells: BTreeSet<String>,
}

fn invalid(message: &str) -> Error {
    Error::InvalidValue(format!("list merge: {message}"))
}

/// Read every declared slot and reject orphan, shared, malformed or cyclic cells.
/// Non-list blank nodes are not admitted by this reader.
pub(crate) fn read(
    graph: &Graph,
    slots: &BTreeMap<Slot, Mode>,
) -> Result<BTreeMap<Slot, Option<Value>>> {
    let mut cells: BTreeMap<String, (Vec<String>, Vec<String>)> = BTreeMap::new();
    for triple in graph {
        let side = match triple.predicate.as_str() {
            p if p == format!("{RDF}first") => 0,
            p if p == format!("{RDF}rest") => 1,
            _ => continue,
        };
        let entry = cells.entry(triple.subject.to_string()).or_default();
        if side == 0 {
            entry.0.push(triple.object.to_string());
        } else {
            entry.1.push(triple.object.to_string());
        }
        if entry.0.len() > 1 || entry.1.len() > 1 {
            return Err(invalid("duplicate first/rest edge"));
        }
        if cells.len() > MAX_CELLS {
            return Err(invalid("cell budget exceeded"));
        }
    }
    let nil = format!("<{RDF}nil>");
    let mut claimed = BTreeSet::new();
    let mut out = BTreeMap::new();
    for slot in slots.keys() {
        let heads: Vec<_> = graph
            .iter()
            .filter(|t| t.subject.to_string() == slot.0 && t.predicate.as_str() == slot.1)
            .map(|t| t.object.to_string())
            .collect();
        if heads.len() > 1 {
            return Err(invalid("list property has multiple heads"));
        }
        let Some(mut head) = heads.into_iter().next() else {
            out.insert(slot.clone(), None);
            continue;
        };
        let mut value = Value {
            elements: Vec::new(),
            cells: BTreeSet::new(),
        };
        while head != nil {
            if !value.cells.insert(head.clone()) {
                return Err(invalid("cycle"));
            }
            if !claimed.insert(head.clone()) {
                return Err(invalid("cell shared by list slots"));
            }
            let (first, rest) = cells
                .get(&head)
                .ok_or_else(|| invalid("unterminated list"))?;
            if first.len() != 1 || rest.len() != 1 {
                return Err(invalid("exactly one first and rest required"));
            }
            if first[0].starts_with("_:") {
                return Err(invalid(
                    "blank element requires explicit nested-value identity",
                ));
            }
            value.elements.push(first[0].clone());
            head = rest[0].clone();
        }
        out.insert(slot.clone(), Some(value));
    }
    if cells.keys().any(|key| !claimed.contains(key)) {
        return Err(invalid("orphan list cell"));
    }
    // Do not silently discard annotations or incoming references when lowering.
    for triple in graph {
        let subject = triple.subject.to_string();
        let object = triple.object.to_string();
        if claimed.contains(&subject)
            && ![format!("{RDF}first"), format!("{RDF}rest")]
                .contains(&triple.predicate.as_str().to_owned())
        {
            return Err(invalid(
                "annotated cell needs an explicit structured-value profile",
            ));
        }
        if claimed.contains(&object)
            && triple.predicate.as_str() != format!("{RDF}rest")
            && !slots.contains_key(&(subject, triple.predicate.as_str().into()))
        {
            return Err(invalid("external reference to a list cell"));
        }
    }
    Ok(out)
}

/// Cell-label-independent identity, also distinguishing absence from rdf:nil.
pub(crate) fn identity(elements: &Option<Vec<String>>) -> Result<String> {
    Ok(crate::share::sha256(
        &serde_json::to_vec(&("quipu-list-value/v1", elements))
            .map_err(|e| Error::Serialization(e.to_string()))?,
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Edit {
    start: usize,
    end: usize,
    values: Vec<String>,
}

fn edits(base: &[String], side: &[String]) -> Result<Vec<Edit>> {
    let n = base.len();
    let m = side.len();
    if n > MAX_CELLS || m > MAX_CELLS || (n + 1).saturating_mul(m + 1) > MAX_DIFF {
        return Err(invalid("sequence diff budget exceeded"));
    }
    let mut lcs = vec![0usize; (n + 1) * (m + 1)];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i * (m + 1) + j] = if base[i] == side[j] {
                1 + lcs[(i + 1) * (m + 1) + j + 1]
            } else {
                lcs[(i + 1) * (m + 1) + j].max(lcs[i * (m + 1) + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut result = Vec::new();
    while i < n || j < m {
        if i < n && j < m && base[i] == side[j] {
            i += 1;
            j += 1;
            continue;
        }
        let start = i;
        let mut values = Vec::new();
        while i < n || j < m {
            if i < n && j < m && base[i] == side[j] {
                break;
            }
            if j < m && (i == n || lcs[i * (m + 1) + j + 1] > lcs[(i + 1) * (m + 1) + j]) {
                values.push(side[j].clone());
                j += 1;
            } else {
                i += 1;
            }
        }
        result.push(Edit {
            start,
            end: i,
            values,
        });
    }
    Ok(result)
}

fn overlap(a: &Edit, b: &Edit) -> bool {
    if a.start == a.end {
        return a.start >= b.start && a.start <= b.end;
    }
    if b.start == b.end {
        return b.start >= a.start && b.start <= a.end;
    }
    a.start < b.end && b.start < a.end
}

/// None means a real list-divergent conflict; absence is represented inside Some.
pub(crate) fn merge(
    base: &Option<Vec<String>>,
    ours: &Option<Vec<String>>,
    theirs: &Option<Vec<String>>,
    mode: Mode,
) -> Result<Option<Option<Vec<String>>>> {
    if ours == base || ours == theirs {
        return Ok(Some(theirs.clone()));
    }
    if theirs == base {
        return Ok(Some(ours.clone()));
    }
    let (Some(base), Some(ours), Some(theirs)) = (base, ours, theirs) else {
        return Ok(None);
    };
    if mode == Mode::Whole {
        return Ok(None);
    }
    let mut combined = edits(base, ours)?;
    for incoming in edits(base, theirs)? {
        if combined.contains(&incoming) {
            continue;
        }
        if combined.iter().any(|old| overlap(old, &incoming)) {
            return Ok(None);
        }
        combined.push(incoming);
    }
    combined.sort_by_key(|edit| (edit.start, edit.end));
    let mut result = Vec::new();
    let mut cursor = 0;
    for edit in combined {
        result.extend_from_slice(&base[cursor..edit.start]);
        result.extend(edit.values);
        cursor = edit.end;
    }
    result.extend_from_slice(&base[cursor..]);
    Ok(Some(Some(result)))
}

/// Lower a complete list value at once, using content+slot-bound stable cell IRIs.
/// The caller removes the previous head and ALL old cells as one merge unit.
pub(crate) fn lower(slot: &Slot, value: &Option<Vec<String>>) -> Result<Graph> {
    let Some(elements) = value else {
        return Ok(Graph::new());
    };
    if elements.len() > MAX_CELLS {
        return Err(invalid("cell budget exceeded"));
    }
    let digest = crate::share::sha256(
        &serde_json::to_vec(&("quipu-list-cells/v1", slot, elements))
            .map_err(|e| Error::Serialization(e.to_string()))?,
    );
    let cell = |i| format!("<urn:quipu:list:{digest}:{i}>");
    let nil = format!("<{RDF}nil>");
    let head = if elements.is_empty() {
        nil.clone()
    } else {
        cell(0)
    };
    let mut graph = parse_graph(&format!("{} <{}> {head} .", slot.0, slot.1), "list head")?;
    if graph.len() != 1 {
        return Err(invalid("invalid list slot"));
    }
    for (i, value) in elements.iter().enumerate() {
        crate::fork_merge_decisions::parse_term(value)?;
        let rest = if i + 1 == elements.len() {
            nil.clone()
        } else {
            cell(i + 1)
        };
        let first = parse_graph(
            &format!("{} <{RDF}first> {value} .", cell(i)),
            "list element",
        )?;
        if first.len() != 1 {
            return Err(invalid("element is not one RDF term"));
        }
        graph.extend(first);
        graph.extend(parse_graph(
            &format!("{} <{RDF}rest> {rest} .", cell(i)),
            "list rest",
        )?);
    }
    Ok(graph)
}

/// One whole-list plan: authored and decision clocks are supplied by the caller.
/// Sequence synthesis is a derived decision at Td; its incoming sequence is
/// retained as an empty-interval receipt, never spliced link by link.
pub(crate) fn plan(
    slot: &Slot,
    base: &Option<Vec<String>>,
    ours: &Option<Vec<String>>,
    theirs: &Option<Vec<String>>,
    mode: Mode,
    decision: Option<&crate::merge_core::Decision<Vec<String>>>,
    ours_from: &str,
    incoming_from: &str,
    td: &str,
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
        ours: rows(ours, ours_from),
        theirs: rows(theirs, incoming_from),
        theirs_closed: base
            .iter()
            .cloned()
            .map(|v| (v, incoming_from.to_owned()))
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
    plan_slot(&input, decision.or(synthesized.as_ref()), td)
}

/// Expand a logical list operation as a whole head+cell operation group.
/// Closing ROOT uses ROOT's original cell identities, not newly generated ones.
pub(crate) fn lower_ops(
    slot: &Slot,
    original_ours: &Graph,
    ops: &[crate::merge_core::FactOp<Vec<String>>],
) -> Result<Vec<crate::merge_core::FactOp<oxrdf::Triple>>> {
    use crate::merge_core::FactOp;
    let mut result = Vec::new();
    for op in ops {
        let graph = match op {
            FactOp::Close { .. } => original_ours.clone(),
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

#[cfg(test)]
#[path = "merge_lists_tests.rs"]
mod tests;
