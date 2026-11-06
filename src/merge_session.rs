//! One prepared plan for share, fork and pendant adapters. Source-stage API.
//! Profiles are explicit, hash-bound input: no sh:group or guessed shape mode.
use crate::error::{Error, Result};
use crate::merge_core::selection::{Mode, select};
use crate::merge_core::{self, Card, Decision, FactOp, SlotInput};
use crate::merge_lists::{self, Slot};
use crate::share_merge::{Graph, by_slot};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Path {
    Share,
    Fork,
    Pendant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ListBinding {
    pub slot: Slot,
    pub mode: merge_lists::Mode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Profile {
    #[serde(default = "profile_schema")]
    pub schema: String,
    #[serde(default)]
    pub shapes_hash: Option<String>,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub lists: Vec<ListBinding>,
    /// Explicit slot groups; never interpreted as SHACL presentation groups.
    #[serde(default)]
    pub units: Vec<BTreeSet<Slot>>,
}

fn profile_schema() -> String {
    "quipu-merge-profile/v1".into()
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            schema: profile_schema(),
            shapes_hash: None,
            mode: Mode::Atomic,
            lists: Vec::new(),
            units: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "kebab-case")]
pub(crate) enum Value {
    Term(String),
    List(Vec<String>),
}

/// Fully retained request. Input graphs and the profile travel with pending rows;
/// after resume ROOT/provenance is re-read, not trusted from this payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    pub store_id: String,
    pub root_tx: i64,
    pub path: Path,
    pub input_id: String,
    pub base_id: String,
    pub base: String,
    pub ours: String,
    pub theirs: String,
    pub shapes: String,
    pub profile: Profile,
    pub parent: Option<String>,
    /// Resume only these slots; already applied slots must never be replayed.
    pub active_slots: Option<BTreeSet<Slot>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Choice {
    pub slot: Slot,
    pub decision: Decision<Value>,
}

#[derive(Clone)]
pub(crate) struct Prepared {
    pub request: Request,
    pub id: String,
    /// Exact clocks used to lower this operation sequence. Persistence cannot
    /// silently substitute another transaction or incoming-authorship time.
    pub authored_at: String,
    pub decision_at: String,
    pub plan: merge_core::MergePlan<Slot, Value>,
    pub prospective: Graph,
    pub receive: Vec<FactOp<oxrdf::Triple>>,
    pub decide: Vec<FactOp<oxrdf::Triple>>,
    /// Connected held units, one pending row per unit (not one per list cell).
    pub held_units: Vec<BTreeSet<Slot>>,
    pub choices: Vec<Choice>,
    pub reviewed_choices: Vec<Choice>,
}

fn invalid(message: &str) -> Error {
    Error::InvalidValue(format!("merge session: {message}"))
}
fn graph(text: &str) -> Result<Graph> {
    crate::share_merge::parse_graph(text, "session snapshot")
}
pub(crate) fn render(graph: &Graph) -> String {
    let mut lines: Vec<_> = graph.iter().map(|t| format!("{t} .\n")).collect();
    lines.sort();
    lines.concat()
}

fn list_graph(graph: &Graph, slot: &Slot, cells: &BTreeSet<String>) -> Graph {
    graph
        .iter()
        .filter(|t| {
            cells.contains(&t.subject.to_string())
                || (t.subject.to_string() == slot.0 && t.predicate.as_str() == slot.1)
        })
        .cloned()
        .collect()
}

fn scalar_graph(graph: &Graph, lists: &BTreeMap<Slot, Option<merge_lists::Value>>) -> Graph {
    graph
        .iter()
        .filter(|t| {
            !lists.iter().any(|(slot, value)| {
                (t.subject.to_string() == slot.0 && t.predicate.as_str() == slot.1)
                    || value
                        .as_ref()
                        .is_some_and(|v| v.cells.contains(&t.subject.to_string()))
            })
        })
        .cloned()
        .collect()
}

/// Prepare all modes with the same scalar/list/selection operators. `start`
/// returns the real ROOT `valid_from` for a head/scalar claim, never a fake clock.
pub(crate) fn prepare(
    mut request: Request,
    choices: &[Choice],
    authored_at: &str,
    td: &str,
    mut start: impl FnMut(bool, &Slot, &str) -> Result<String>,
    mut closes: impl FnMut(&Slot) -> Result<BTreeMap<String, String>>,
) -> Result<Prepared> {
    let (base, ours, theirs) = (
        graph(&request.base)?,
        graph(&request.ours)?,
        graph(&request.theirs)?,
    );
    request.profile = crate::merge_profile::bind(
        request.profile.clone(),
        &request.shapes,
        [&base, &ours, &theirs],
    )?;
    if let Some(active) = &request.active_slots {
        request
            .profile
            .units
            .retain(|unit| !unit.is_disjoint(active));
        if request
            .profile
            .units
            .iter()
            .any(|unit| !unit.is_subset(active))
        {
            return Err(invalid("resume would split a shape-declared unit"));
        }
    }
    let mut lists = BTreeMap::new();
    for binding in &request.profile.lists {
        if lists.insert(binding.slot.clone(), binding.mode).is_some() {
            return Err(invalid("duplicate list slot"));
        }
    }
    let (bl, ol, tl) = (
        merge_lists::read(&base, &lists)?,
        merge_lists::read(&ours, &lists)?,
        merge_lists::read(&theirs, &lists)?,
    );
    let (b, o, t) = (
        by_slot(&scalar_graph(&base, &bl)),
        by_slot(&scalar_graph(&ours, &ol)),
        by_slot(&scalar_graph(&theirs, &tl)),
    );
    let bounds = crate::merge3::ShapeBounds::parse(&request.shapes)?;
    let view = crate::merge3::TripleView::new(base.iter().chain(&ours).chain(&theirs));
    let empty = BTreeSet::new();
    let mut inputs = BTreeMap::new();
    let keys: BTreeSet<_> = b.keys().chain(o.keys()).chain(t.keys()).cloned().collect();
    for slot in keys {
        let bv = b.get(&slot).unwrap_or(&empty);
        let ov = o.get(&slot).unwrap_or(&empty);
        let tv = t.get(&slot).unwrap_or(&empty);
        let rows = ov
            .iter()
            .map(|v| Ok((Value::Term(v.clone()), start(false, &slot, v)?)))
            .collect::<Result<_>>()?;
        inputs.insert(
            slot.clone(),
            SlotInput {
                base: bv.iter().cloned().map(Value::Term).collect(),
                ours: rows,
                theirs: tv
                    .iter()
                    .map(|v| Ok((Value::Term(v.clone()), start(true, &slot, v)?)))
                    .collect::<Result<_>>()?,
                theirs_closed: if request.path == Path::Fork {
                    closes(&slot)?
                        .into_iter()
                        .map(|(v, t)| (Value::Term(v), t))
                        .collect()
                } else {
                    bv.difference(tv)
                        .cloned()
                        .map(|v| (Value::Term(v), authored_at.to_owned()))
                        .collect()
                },
                card: Card::Many(bounds.bound(&slot.0, &slot.1, &view)?),
            },
        );
    }
    let mut decisions = BTreeMap::new();
    for choice in choices {
        if decisions
            .insert(choice.slot.clone(), choice.decision.clone())
            .is_some()
        {
            return Err(invalid("duplicate decision slot"));
        }
    }
    for (slot, mode) in &lists {
        let sequence = |v: &Option<merge_lists::Value>| v.as_ref().map(|v| v.elements.clone());
        let (bv, ov, tv) = (
            sequence(&bl[slot]),
            sequence(&ol[slot]),
            sequence(&tl[slot]),
        );
        let set = |v: &Option<Vec<String>>| v.iter().cloned().map(Value::List).collect();
        let head = ours
            .iter()
            .find(|tr| tr.subject.to_string() == slot.0 && tr.predicate.as_str() == slot.1);
        let ours_start = match head {
            Some(head) => start(false, slot, &head.object.to_string())?,
            None => td.to_owned(),
        };
        let incoming_head = theirs
            .iter()
            .find(|tr| tr.subject.to_string() == slot.0 && tr.predicate.as_str() == slot.1);
        let incoming_start = match incoming_head {
            Some(head) => start(true, slot, &head.object.to_string())?,
            None => authored_at.to_owned(),
        };
        let closed = if request.path == Path::Fork {
            let original = base
                .iter()
                .find(|tr| tr.subject.to_string() == slot.0 && tr.predicate.as_str() == slot.1);
            let by_value = closes(slot)?;
            original.and_then(|head| by_value.get(&head.object.to_string()).cloned())
        } else {
            Some(authored_at.to_owned())
        };
        inputs.insert(
            slot.clone(),
            SlotInput {
                base: set(&bv),
                ours: ov
                    .iter()
                    .cloned()
                    .map(|v| (Value::List(v), ours_start.clone()))
                    .collect(),
                theirs: tv
                    .iter()
                    .cloned()
                    .map(|v| (Value::List(v), incoming_start.clone()))
                    .collect(),
                theirs_closed: bv
                    .iter()
                    .filter_map(|v| closed.as_ref().map(|t| (Value::List(v.clone()), t.clone())))
                    .collect(),
                card: Card::ONE,
            },
        );
        if !decisions.contains_key(slot)
            && *mode == merge_lists::Mode::Sequence
            && ov != bv
            && tv != bv
            && ov != tv
            && let Some(chosen) = merge_lists::merge(&bv, &ov, &tv, *mode)?
        {
            decisions.insert(slot.clone(), Decision::Values(set(&chosen)));
        }
    }
    // A decision cannot change a scalar into a list, or inject multiple lists.
    for (slot, decision) in &decisions {
        if let Decision::Values(values) = decision
            && values
                .iter()
                .any(|value| matches!(value, Value::List(_)) != lists.contains_key(slot))
        {
            return Err(invalid(
                "decision value type differs from the declared slot profile",
            ));
        }
    }
    for unit in &request.profile.units {
        for slot in unit {
            inputs.entry(slot.clone()).or_insert_with(|| SlotInput {
                base: BTreeSet::new(),
                ours: BTreeMap::new(),
                theirs: BTreeMap::new(),
                theirs_closed: BTreeMap::new(),
                card: Card::Many(None),
            });
        }
    }
    if let Some(active) = &request.active_slots {
        if active.iter().any(|slot| !inputs.contains_key(slot)) {
            return Err(invalid("resume slot disappeared"));
        }
        inputs.retain(|slot, _| active.contains(slot));
    }
    let entities = inputs
        .keys()
        .map(|slot| (slot.clone(), slot.0.clone()))
        .collect();
    let plan = select(
        &inputs,
        &decisions,
        &entities,
        &request.profile.units,
        request.profile.mode,
        td,
    )?;
    let lower = |step: &[(Slot, FactOp<Value>)]| -> Result<Vec<FactOp<oxrdf::Triple>>> {
        let mut out = Vec::new();
        for (slot, op) in step {
            let value = match op {
                FactOp::Assert { value, .. }
                | FactOp::Close { value, .. }
                | FactOp::Receipt { value, .. } => value,
            };
            let triples = match value {
                Value::Term(value) => {
                    crate::fork_merge_decisions::parse_term(value)?;
                    crate::share_merge::parse_graph(
                        &format!("{} <{}> {value} .", slot.0, slot.1),
                        "scalar operation",
                    )?
                }
                Value::List(value) => {
                    if matches!(op, FactOp::Close { .. }) {
                        list_graph(
                            &ours,
                            slot,
                            &ol[slot]
                                .as_ref()
                                .ok_or_else(|| invalid("missing ROOT list"))?
                                .cells,
                        )
                    } else {
                        merge_lists::lower(slot, &Some(value.clone()))?
                    }
                }
            };
            let mut triples: Vec<_> = triples.into_iter().collect();
            triples.sort_by_key(ToString::to_string);
            for value in triples {
                out.push(match op {
                    FactOp::Assert { from, .. } => FactOp::Assert {
                        value,
                        from: from.clone(),
                    },
                    FactOp::Close { at, .. } => FactOp::Close {
                        value,
                        at: at.clone(),
                    },
                    FactOp::Receipt { at, .. } => FactOp::Receipt {
                        value,
                        at: at.clone(),
                    },
                });
            }
        }
        Ok(out)
    };
    let (receive, decide) = (lower(&plan.receive)?, lower(&plan.decide)?);
    let mut prospective = ours.clone();
    for op in receive.iter().chain(&decide) {
        match op {
            FactOp::Assert { value, .. } => {
                prospective.insert(value.clone());
            }
            FactOp::Close { value, .. } => {
                prospective.remove(value);
            }
            FactOp::Receipt { .. } => {}
        }
    }
    let mut held_units: Vec<BTreeSet<Slot>> = plan
        .slots
        .iter()
        .filter(|(_, p)| p.held)
        .map(|(s, _)| BTreeSet::from([s.clone()]))
        .collect();
    let mut groups = request.profile.units.clone();
    if request.profile.mode == Mode::PartialEntity {
        let mut by_entity: BTreeMap<String, BTreeSet<Slot>> = BTreeMap::new();
        for key in plan.slots.keys() {
            by_entity
                .entry(key.0.clone())
                .or_default()
                .insert(key.clone());
        }
        groups.extend(by_entity.into_values());
    }
    for unit in &groups {
        let mut merged = BTreeSet::new();
        held_units.retain(|old| {
            if !old.is_disjoint(unit) {
                merged.extend(old.iter().cloned());
                false
            } else {
                true
            }
        });
        if !merged.is_empty() {
            held_units.push(merged);
        }
    }
    held_units.sort();
    let mut ordered = choices.to_vec();
    ordered.sort_by(|a, b| a.slot.cmp(&b.slot));
    let id = crate::share::sha256(
        &serde_json::to_vec(&("quipu-merge-session/v1", &request, &ordered, authored_at))
            .map_err(|e| Error::Serialization(e.to_string()))?,
    );
    let recorded_choices = plan
        .slots
        .iter()
        .filter_map(|(slot, p)| {
            decisions
                .get(slot)
                .cloned()
                .or_else(|| {
                    (p.change == merge_core::Change::Convergent).then_some(Decision::Continue)
                })
                .map(|decision| Choice {
                    slot: slot.clone(),
                    decision,
                })
        })
        .collect();
    Ok(Prepared {
        request,
        id,
        authored_at: authored_at.to_owned(),
        decision_at: td.to_owned(),
        plan,
        prospective,
        receive,
        decide,
        held_units,
        choices: recorded_choices,
        reviewed_choices: ordered,
    })
}

#[cfg(test)]
#[path = "merge_session_tests.rs"]
mod tests;
