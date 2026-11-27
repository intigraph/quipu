//! SHACL-derived type dependencies for participant batches (aegis-n0wc.5
//! Phase 3 slice B).
//!
//! Every member with active write shapes is judged at PREPARE, under the
//! fences and before any intent, by a dry run of its exact batch in a
//! rolled-back savepoint. Before slice B a member violation surfaced only at
//! APPLY, after the COMMIT decision, leaving every participant fenced with a
//! replay that fails the same way.
//!
//! When the dry run refuses and the batch enlists type sources, the values the
//! member's `sh:class` constraints (on predicate paths) need typed are looked
//! up ONLY in those sources' owned graphs, under their fences: a direct
//! `rdf:type`, or a type plus the `rdfs:subClassOf` chain up to the required
//! class. Each triple used becomes a derived dependency, pinned in the source
//! (types as [`DependencyPin`]s, axioms as [`SubclassPin`]s) and handed to the
//! member's apply as context, so the local gate sees exactly what was read and
//! pinned. Unenlisted files and ROOT are never sources. The context is
//! subset-only: it is never written and creates no focus, so it can satisfy
//! a constraint but never add a violation. Derived and declared checks share
//! [`MAX_DEPENDENCY_CHECKS`]; a duplicate of a declared check is one check.
#[cfg(feature = "shacl")]
use super::deps::{DependencyPin, SUBCLASS_OF, SubclassPin};
use super::*;
use std::collections::BTreeMap;
#[cfg(feature = "shacl")]
use std::collections::BTreeSet;

#[cfg(feature = "shacl")]
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

#[path = "participant_replay.rs"]
mod replay;
#[cfg(feature = "shacl")]
use replay::budget;
pub(super) use replay::{audit, audit_pending, recheck_shapes, replay};
#[cfg(all(test, feature = "shacl"))]
pub use replay::{
    with_before_decision, with_before_decision_on, with_prepare_budget, with_slow_dry_run,
};

/// One derived triple, the source (index in canonical order) and graph it
/// was read from.
#[cfg(feature = "shacl")]
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Found {
    source: usize,
    graph: String,
    triple: [String; 3],
}

/// Every participant owner: members, then validation-only owners, then type
/// sources not already present. A member may neither validate a type
/// dependency nor be a type source.
pub(super) fn participants<'a>(
    members: &[Member<'a>],
    validations: &[Validation<'a>],
    sources: &[&'a GraphSetOwner],
) -> Result<Vec<&'a GraphSetOwner>> {
    let is_member = |owner: &GraphSetOwner| members.iter().any(|m| std::ptr::eq(m.owner, owner));
    if validations
        .iter()
        .any(|v| is_member(v.owner) && !v.checks.is_empty())
    {
        return Err(refuse(
            "duplicate participant: a member cannot validate a type dependency",
        ));
    }
    if sources.iter().any(|s| is_member(s)) {
        return Err(refuse(
            "duplicate participant: a member cannot be a type source",
        ));
    }
    let mut owners: Vec<&'a GraphSetOwner> = members.iter().map(|m| m.owner).collect();
    for owner in validations
        .iter()
        .map(|v| v.owner)
        .chain(sources.iter().copied())
    {
        if !owners.iter().any(|o| std::ptr::eq(*o, owner)) {
            owners.push(owner);
        }
    }
    Ok(owners)
}

/// One participant's exact intent payload: its member delta (or nothing)
/// with its pin changes.
pub(super) fn payload(owner: &GraphSetOwner, members: &[Member<'_>], pins: &PinChanges) -> Vec<u8> {
    match members.iter().find(|m| std::ptr::eq(m.owner, owner)) {
        Some(member) => encode_payload_with(&member.batch, pins),
        None => super::deps::empty_payload(owner, pins),
    }
}

/// Bind the enlisted type sources (sorted file ids) into the batch digest;
/// absent when none, so earlier digests are unchanged.
pub(super) fn bind_sources(bound: &mut Value, sources: &[&GraphSetOwner]) {
    if !sources.is_empty() {
        let mut ids: Vec<_> = sources
            .iter()
            .map(|s| s.owner.catalog_route().file_id.clone())
            .collect();
        ids.sort();
        bound["sources"] = serde_json::json!(ids);
    }
}

/// Judge members at PREPARE and fold any derived dependencies into `plan`
/// (pins per source, context per member).
#[cfg_attr(not(feature = "shacl"), allow(unused_variables))]
pub(super) fn prepare_members(
    order: &[&GraphSetOwner],
    guards: &mut [Guard<'_>],
    members: &[Member<'_>],
    validations: &[Validation<'_>],
    sources: &[&GraphSetOwner],
    plan: &mut BTreeMap<String, PinChanges>,
) -> Result<()> {
    #[cfg(not(feature = "shacl"))]
    {
        if sources.is_empty() {
            return Ok(());
        }
        Err(refuse("type sources require the shacl feature"))
    }
    #[cfg(feature = "shacl")]
    {
        let position = |owner: &GraphSetOwner| {
            order
                .iter()
                .position(|o| std::ptr::eq(*o, owner))
                .ok_or_else(|| refuse("participant lookup"))
        };
        let source_at: Vec<usize> = sources.iter().map(|s| position(s)).collect::<Result<_>>()?;
        let declared: BTreeSet<[String; 3]> = validations
            .iter()
            .flat_map(|v| &v.checks)
            .map(|c| [c.graph.clone(), c.entity.clone(), c.type_iri.clone()])
            .collect();
        let declared_count: usize = validations
            .iter()
            .map(|v| v.checks.len() + v.absences.len())
            .sum();
        let mut derived: BTreeSet<(String, [String; 3])> = BTreeSet::new();
        let started = std::time::Instant::now();
        let over_budget = || {
            (started.elapsed() > budget()).then(|| {
                refuse(format!(
                    "prepare validation exceeded {:?} under the fences; nothing prepared",
                    budget()
                ))
            })
        };
        for member in members {
            let at = position(member.owner)?;
            let file = member.owner.owner.catalog_route().file_id.clone();
            let Some(shapes) = guards[at].write_reject_shapes()? else {
                continue;
            };
            plan.entry(file.clone()).or_default().shapes =
                Some((sha256(shapes.as_bytes()), shapes.clone()));
            let outcome = dry_run(&mut guards[at], &Delta::of(&member.batch), &[]);
            if let Some(error) = over_budget() {
                return Err(error);
            }
            let first = match outcome {
                Ok(()) => continue,
                Err(error @ Error::ValidationFailed { .. }) => error,
                Err(error) => {
                    return Err(refuse(format!(
                        "member {file} refused at prepare: {error}; nothing prepared"
                    )));
                }
            };
            let mut found = Vec::new();
            let mut unresolved = Vec::new();
            for (value, class) in candidates(&guards[at], &shapes, &member.batch)? {
                match find(order, guards, &source_at, &value, &class)? {
                    Some(triples) => {
                        for f in &triples {
                            let [s, p, o] = &f.triple;
                            let is_declared = p == RDF_TYPE
                                && declared.contains(&[f.graph.clone(), s.clone(), o.clone()]);
                            if !is_declared {
                                derived.insert((f.graph.clone(), f.triple.clone()));
                            }
                        }
                        // The bound counts distinct checks (source reads), deduped
                        // across members. Pins are per dependent file, so N members
                        // deriving one triple write N pins for 1 check
                        // (eph-review-0dd73c #3): each pin guards its own dependent.
                        let total = declared_count + derived.len();
                        if total > MAX_DEPENDENCY_CHECKS {
                            return Err(refuse(format!(
                                "dependency bound exceeded: {total} checks over \
                                 {MAX_DEPENDENCY_CHECKS} (derived for sh:class {class} on \
                                 {value}); nothing prepared"
                            )));
                        }
                        found.extend(triples.into_iter().map(|f| (value.clone(), f)));
                    }
                    None => unresolved.push(format!("{value} (sh:class {class})")),
                }
            }
            let context: Vec<[String; 3]> = found
                .iter()
                .map(|(_, f)| f.triple.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let unresolved = if unresolved.is_empty() {
                "none".to_owned()
            } else {
                unresolved.join(", ")
            };
            if context.is_empty() {
                return Err(refuse(format!(
                    "member {file} refused at prepare: {first}; values no enlisted type \
                     source types: {unresolved}; nothing prepared"
                )));
            }
            let outcome = dry_run(&mut guards[at], &Delta::of(&member.batch), &context);
            if let Some(error) = over_budget() {
                return Err(error);
            }
            if let Err(error) = outcome {
                return Err(refuse(format!(
                    "member {file} refused at prepare with derived types: {error}; values no \
                     enlisted type source types: {unresolved}; nothing prepared"
                )));
            }
            for (via, f) in found {
                let source = order[f.source].owner.catalog_route().file_id.clone();
                let [s, p, o] = f.triple;
                let changes = plan.entry(source).or_default();
                if p == RDF_TYPE {
                    changes.add.push(DependencyPin {
                        graph: f.graph,
                        entity: s,
                        type_iri: o,
                        dependent_file_id: file.clone(),
                    });
                } else {
                    changes.axioms.push(SubclassPin {
                        graph: f.graph,
                        class: s,
                        parent: o,
                        via,
                        dependent_file_id: file.clone(),
                    });
                }
            }
            plan.entry(file).or_default().context = context;
        }
        for changes in plan.values_mut() {
            changes.add.sort();
            changes.add.dedup();
            changes.axioms.sort();
            changes.axioms.dedup();
        }
        Ok(())
    }
}

/// The parts of a member delta a dry run replays: from a caller batch at
/// PREPARE, or from a decoded intent in the post-commit audit.
#[cfg_attr(not(feature = "shacl"), allow(dead_code))]
pub(super) struct Delta<'a> {
    pub(super) graphs: &'a [(String, Vec<crate::store::Datum>)],
    pub(super) timestamp: &'a str,
    pub(super) actor: Option<&'a str>,
    pub(super) source: Option<&'a str>,
}

#[cfg_attr(not(feature = "shacl"), allow(dead_code))]
impl<'a> Delta<'a> {
    fn of(batch: &'a GraphSetBatch) -> Self {
        Delta {
            graphs: &batch.graphs,
            timestamp: &batch.timestamp,
            actor: batch.actor.as_deref(),
            source: batch.source.as_deref(),
        }
    }
}

/// Apply `batch` to `store` inside a savepoint that is always rolled back:
/// the exact local gate a later apply runs, with nothing kept.
#[cfg(feature = "shacl")]
pub(super) fn dry_run(store: &mut Store, batch: &Delta<'_>, context: &[[String; 3]]) -> Result<()> {
    let native = batch
        .graphs
        .iter()
        .map(|(graph, datums)| {
            let id = store
                .committed_graph_id(graph)?
                .ok_or_else(|| refuse("member graph is absent or not committed"))?;
            Ok((id, datums.clone()))
        })
        .collect::<Result<Vec<_>>>()?;
    #[cfg(test)]
    replay::test_delay();
    store.ownership_operation(|store| {
        store
            .conn
            .execute_batch("SAVEPOINT quipu_participant_dry_run")?;
        let outcome = store.with_derived_type_context(context, |store| {
            store
                .transact_graph_batches_tx(&native, batch.timestamp, batch.actor, batch.source)
                .map(|_| ())
        });
        crate::store::release::rollback_release(&store.conn, "quipu_participant_dry_run")?;
        store.read_model.borrow_mut().clear();
        // The dry run interned terms (the derived context's foreign IRIs, any
        // new datum term) that the rollback just freed; their rowids are
        // reused, so cached ids would resolve a later term to the wrong IRI
        // (eph-review-873d46). Same convention as WriteScope's rollback.
        store.term_cache.borrow_mut().clear_persistent();
        outcome
    })
}

/// (value, class) for every current asserted reference the member writes on
/// a predicate some `sh:class` constrains.
///
/// Deliberately conservative (eph-review-d2b162 #3, eph-review-0dd73c #2):
/// this does not narrow to the (focus, value) pairs the refusing report
/// names, nor skip values the member's own post-state already types. Every
/// candidate a source can type is pinned, which can over-pin the source (a
/// later retraction of that type is refused until the pin clears) and can
/// reach [`MAX_DEPENDENCY_CHECKS`] sooner than strictly needed. Both failure
/// modes refuse; neither admits an unverified write. Narrowing needs the
/// validation report parsed per focus and is follow-up work.
#[cfg(feature = "shacl")]
fn candidates(
    store: &Store,
    shapes: &str,
    batch: &GraphSetBatch,
) -> Result<BTreeSet<(String, String)>> {
    let plan = crate::shacl::cached_validator(shapes)?.cardinality_plan()?;
    let mut out = BTreeSet::new();
    if plan.class_requirements.is_empty() {
        return Ok(out);
    }
    for datum in batch.graphs.iter().flat_map(|(_, datums)| datums) {
        let crate::types::Value::Ref(value) = datum.value else {
            continue;
        };
        if datum.op != crate::types::Op::Assert || datum.valid_to.is_some() {
            continue;
        }
        let predicate = store.resolve(datum.attribute)?;
        for (path, class) in &plan.class_requirements {
            if *path == predicate {
                out.insert((store.resolve(value)?, class.clone()));
            }
        }
    }
    Ok(out)
}

/// Current asserted `subject predicate ?o` objects in one graph, sorted.
#[cfg(feature = "shacl")]
fn objects(store: &Store, graph: &str, subject: &str, predicate: &str) -> Result<Vec<String>> {
    let Some(g) = store.committed_graph_id(graph)? else {
        return Ok(Vec::new());
    };
    let (Some(entity), Some(attribute)) = (store.lookup(subject)?, store.lookup(predicate)?) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for fact in store.current_facts_for_entities_in_graphs(&[entity], &[g])? {
        if fact.attribute == attribute
            && fact.op == crate::types::Op::Assert
            && let crate::types::Value::Ref(id) = fact.value
        {
            out.push(store.resolve(id)?);
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// The triples, from enlisted sources only, by which `value` is a `class`:
/// a direct type, or a type and its subclass chain. First match in canonical
/// source order, owner graph order and sorted type order.
#[cfg(feature = "shacl")]
fn find(
    order: &[&GraphSetOwner],
    guards: &[Guard<'_>],
    sources: &[usize],
    value: &str,
    class: &str,
) -> Result<Option<Vec<Found>>> {
    for &s in sources {
        let route = order[s].owner.catalog_route();
        for graph in order[s].catalog.owner_graphs(&route.graph)? {
            for ty in objects(&guards[s], graph, value, RDF_TYPE)? {
                let typed = Found {
                    source: s,
                    graph: graph.to_owned(),
                    triple: [value.to_owned(), RDF_TYPE.to_owned(), ty.clone()],
                };
                if ty == class {
                    return Ok(Some(vec![typed]));
                }
                if let Some(mut chain) = subclass_chain(order, guards, sources, &ty, class)? {
                    chain.insert(0, typed);
                    return Ok(Some(chain));
                }
            }
        }
    }
    Ok(None)
}

/// Breadth-first `from rdfs:subClassOf+ to` over the sources' owned graphs,
/// bounded like the local post-state hierarchy walk.
#[cfg(feature = "shacl")]
fn subclass_chain(
    order: &[&GraphSetOwner],
    guards: &[Guard<'_>],
    sources: &[usize],
    from: &str,
    to: &str,
) -> Result<Option<Vec<Found>>> {
    let mut parent_of: BTreeMap<String, Found> = BTreeMap::new();
    let mut queue = std::collections::VecDeque::from([from.to_owned()]);
    let mut seen = BTreeSet::from([from.to_owned()]);
    while let Some(class) = queue.pop_front() {
        for &s in sources {
            let route = order[s].owner.catalog_route();
            for graph in order[s].catalog.owner_graphs(&route.graph)? {
                for parent in objects(&guards[s], graph, &class, SUBCLASS_OF)? {
                    if !seen.insert(parent.clone()) {
                        continue;
                    }
                    if seen.len() > crate::store::shacl_poststate::CONTEXT_NODE_BUDGET {
                        return Err(refuse(format!(
                            "type hierarchy above {from} exceeds the derivation budget"
                        )));
                    }
                    parent_of.insert(
                        parent.clone(),
                        Found {
                            source: s,
                            graph: graph.to_owned(),
                            triple: [class.clone(), SUBCLASS_OF.to_owned(), parent.clone()],
                        },
                    );
                    if parent == to {
                        let mut chain = Vec::new();
                        let mut at = parent;
                        while let Some(step) = parent_of.get(&at) {
                            chain.push(step.clone());
                            at = step.triple[0].clone();
                        }
                        chain.reverse();
                        return Ok(Some(chain));
                    }
                    queue.push_back(parent);
                }
            }
        }
    }
    Ok(None)
}
