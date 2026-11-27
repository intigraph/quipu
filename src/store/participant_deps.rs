//! Bounded cross-graph type dependencies for participant batches
//! (aegis-n0wc.5 Phase 3 S2b). A dependency is exactly "entity E has
//! rdf:type T in graph G", where G belongs to a validation-only participant
//! of the same pinned catalog. Its owner is admitted and fenced like any
//! participant (a durable intent with an empty payload), checked under that
//! fence BEFORE any intent, and never written. Anything else refuses before
//! admission.
//!
//! S4 adds the inverse: a committed dependency PINS "E a T in G" in G's file,
//! once per dependent member file. Any later write into G that removes that
//! type (a Retract of it, or an Assert of it with a `valid_to` bound) refuses,
//! unless it is a participant batch that enlists every pinning file with a
//! [`ReferenceAbsence`] check proving that file no longer references E; those
//! pins are then dropped in the same apply. Pins stay until such a release (a
//! dependent dropping its reference does not by itself unpin).
//!
//! Slice B derives dependencies from a member's own sh:class shapes (see
//! `participant_derive.rs`); a subclass axiom it relies on is pinned as a
//! [`SubclassPin`] with the same removal guard (dearing c69: BLOCK, never
//! record-only). Who may clear a pin: whoever can run a participant batch with
//! writer authority over BOTH the pinning (source) file and the dependent
//! file, enlisting the dependent with a reference-absence check. When that
//! needs the dependent file's owner and the owner is gone or stopped, the
//! remedy is an administrator escalation; there is no bypass. Not provided:
//! unbounded or global constraints.
use super::*;

/// The most dependency checks one batch may declare.
pub const MAX_DEPENDENCY_CHECKS: usize = 64;

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

/// "`entity` has rdf:type `type_iri` in `graph`", all exact IRIs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeDependency {
    pub graph: String,
    pub entity: String,
    pub type_iri: String,
}

/// "No current fact in this participant's member graphs has `entity` as its
/// subject or as a reference value": the reverse check that lets a batch
/// change a type that this participant's file pinned (S4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceAbsence {
    pub entity: String,
}

/// A validation-only participant and the checks it must satisfy.
pub struct Validation<'a> {
    pub owner: &'a GraphSetOwner,
    pub checks: Vec<TypeDependency>,
    pub absences: Vec<ReferenceAbsence>,
}

/// A durable record, in the dependency graph's file, that `dependent_file_id`
/// committed a batch relying on "`entity` a `type_iri` in `graph`" (S4).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DependencyPin {
    pub graph: String,
    pub entity: String,
    pub type_iri: String,
    pub dependent_file_id: String,
}

/// A durable record, in a source file, that `dependent_file_id` committed a
/// batch whose sh:class satisfaction for `via` relied on "`class`
/// rdfs:subClassOf `parent`" in `graph` (slice B).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SubclassPin {
    pub graph: String,
    pub class: String,
    pub parent: String,
    pub via: String,
    pub dependent_file_id: String,
}

pub(super) const SUBCLASS_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";

/// Every subclass-axiom pin this owner's file holds, sorted.
pub fn subclass_pins(owner: &GraphSetOwner) -> Result<Vec<SubclassPin>> {
    owner.owner.read(read_subclass_pins)
}

fn read_subclass_pins(store: &Store) -> Result<Vec<SubclassPin>> {
    if !table_exists(store, "subclass_pins")? {
        return Ok(Vec::new());
    }
    let mut stmt = store.conn.prepare(
        "SELECT graph, class, parent, via, dependent_file_id FROM subclass_pins \
         ORDER BY graph, class, parent, via, dependent_file_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(SubclassPin {
            graph: row.get(0)?,
            class: row.get(1)?,
            parent: row.get(2)?,
            via: row.get(3)?,
            dependent_file_id: row.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Every pin this owner's file holds, sorted.
pub fn dependency_pins(owner: &GraphSetOwner) -> Result<Vec<DependencyPin>> {
    owner.owner.read(read_pins)
}

pub(super) fn read_pins(store: &Store) -> Result<Vec<DependencyPin>> {
    if !table_exists(store, "dependency_pins")? {
        return Ok(Vec::new());
    }
    let mut stmt = store.conn.prepare(
        "SELECT graph, entity, type_iri, dependent_file_id FROM dependency_pins \
         ORDER BY graph, entity, type_iri, dependent_file_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(DependencyPin {
            graph: row.get(0)?,
            entity: row.get(1)?,
            type_iri: row.get(2)?,
            dependent_file_id: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn exact(iri: &str) -> bool {
    match iri.split_once(':') {
        Some((scheme, rest)) => {
            !scheme.is_empty()
                && scheme != "_"
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
                && !rest.is_empty()
                && !iri
                    .chars()
                    .any(|c| c.is_whitespace() || "<>\"{}|\\^`".contains(c))
        }
        None => false,
    }
}

/// Shape and bound checks that need no store: refuse before any fence.
pub(super) fn check_bounds(validations: &[Validation<'_>]) -> Result<()> {
    let total: usize = validations
        .iter()
        .map(|v| v.checks.len() + v.absences.len())
        .sum();
    if total > MAX_DEPENDENCY_CHECKS {
        return Err(refuse(format!(
            "dependency bound exceeded: {total} checks over {MAX_DEPENDENCY_CHECKS}"
        )));
    }
    for validation in validations {
        if validation.checks.is_empty() && validation.absences.is_empty() {
            return Err(refuse("a validation participant declares no dependency"));
        }
        if !validation.absences.iter().all(|a| exact(&a.entity)) {
            return Err(refuse("a dependency needs exact IRIs"));
        }
        let route = validation.owner.owner.catalog_route();
        let owned = validation.owner.catalog.owner_graphs(&route.graph)?;
        for check in &validation.checks {
            if ![&check.graph, &check.entity, &check.type_iri]
                .iter()
                .all(|iri| exact(iri))
            {
                return Err(refuse("a dependency needs exact IRIs"));
            }
            if !owned.contains(&check.graph.as_str()) {
                return Err(refuse(format!(
                    "dependency graph {} is not owned by validation participant {}",
                    check.graph, route.file_id
                )));
            }
        }
    }
    Ok(())
}

/// The empty native delta a validation participant prepares and applies.
pub(super) fn empty_payload(owner: &GraphSetOwner, pins: &PinChanges) -> Vec<u8> {
    let route = owner.owner.catalog_route();
    encode_payload_with(
        &GraphSetBatch {
            store_id: route.store_id.clone(),
            log_epoch: route.log_epoch.clone(),
            graphs: Vec::new(),
            timestamp: String::new(),
            actor: None,
            source: None,
        },
        pins,
    )
}

/// Canonical description bound into the batch digest.
pub(super) fn describe(validations: &[Validation<'_>]) -> Value {
    let mut rows: Vec<_> = validations
        .iter()
        .map(|v| {
            let mut checks: Vec<_> = v
                .checks
                .iter()
                .map(|c| [c.graph.clone(), c.entity.clone(), c.type_iri.clone()])
                .collect();
            checks.sort();
            let mut absences: Vec<_> = v.absences.iter().map(|a| a.entity.clone()).collect();
            absences.sort();
            (
                v.owner.owner.catalog_route().file_id.clone(),
                checks,
                absences,
            )
        })
        .collect();
    rows.sort();
    serde_json::json!(rows)
}

/// Run every check on its admitted, fenced validation writer.
pub(super) fn check_satisfied(
    order: &[&GraphSetOwner],
    guards: &[Guard<'_>],
    validations: &[Validation<'_>],
    members: &[Member<'_>],
) -> Result<()> {
    for validation in validations {
        let at = order
            .iter()
            .position(|owner| std::ptr::eq(*owner, validation.owner))
            .ok_or_else(|| refuse("validation participant lookup"))?;
        let store = &guards[at];
        let own = members
            .iter()
            .find(|m| std::ptr::eq(m.owner, validation.owner))
            .map(|m| m.batch.graphs.as_slice());
        for absence in &validation.absences {
            if references(store, validation.owner, &absence.entity, own)? {
                return Err(refuse(format!(
                    "{} still references {}; nothing prepared",
                    validation.owner.owner.catalog_route().file_id,
                    absence.entity
                )));
            }
        }
        for check in &validation.checks {
            if !holds(store, check)? {
                return Err(refuse(format!(
                    "dependency not satisfied: {} a {} in {}; nothing prepared",
                    check.entity, check.type_iri, check.graph
                )));
            }
        }
    }
    Ok(())
}

fn holds(store: &Store, check: &TypeDependency) -> Result<bool> {
    let Some(graph) = store.committed_graph_id(&check.graph)? else {
        return Ok(false);
    };
    let (Some(entity), Some(attribute), Some(ty)) = (
        store.lookup(&check.entity)?,
        store.lookup(RDF_TYPE)?,
        store.lookup(&check.type_iri)?,
    ) else {
        return Ok(false);
    };
    Ok(store
        .current_facts_for_entities_in_graphs(&[entity], &[graph])?
        .iter()
        .any(|fact| {
            fact.attribute == attribute
                && fact.op == crate::types::Op::Assert
                && matches!(fact.value, crate::types::Value::Ref(id) if id == ty)
        }))
}

/// The most current referencing facts a member may retract in the same
/// batch to prove an absence; more refuses rather than scanning unbounded.
pub const MAX_RETRACTED_REFERENCES: usize = 256;

/// Whether any current fact in `owner`'s member graphs has `entity` as its
/// subject or as a reference value (`idx_current_gea` / `idx_active_vge`).
/// For a member (`own` = its batch), judged AFTER that batch: every current
/// reference must be retracted by it, and it may assert no new one.
fn references(
    store: &Store,
    owner: &GraphSetOwner,
    entity: &str,
    own: Option<&[(String, Vec<crate::store::Datum>)]>,
) -> Result<bool> {
    let id = store.lookup(entity)?;
    let touches = |d: &crate::store::Datum, id: i64| {
        d.entity == id || matches!(d.value, crate::types::Value::Ref(v) if v == id)
    };
    if let (Some(own), Some(id)) = (own, id)
        && own
            .iter()
            .flat_map(|(_, datums)| datums)
            .any(|d| touches(d, id) && d.op == crate::types::Op::Assert && d.valid_to.is_none())
    {
        return Ok(true);
    }
    let Some(id) = id else {
        return Ok(false);
    };
    let route = owner.owner.catalog_route();
    let value = crate::types::Value::Ref(id).to_bytes();
    for graph in owner.catalog.owner_graphs(&route.graph)? {
        let Some(g) = store.committed_graph_id(graph)? else {
            continue;
        };
        let mut stmt = store.conn.prepare(
            "SELECT e, a, v FROM facts WHERE op = 1 AND valid_to IS NULL AND g = ?1 AND e = ?2 \
             UNION SELECT e, a, v FROM facts WHERE op = 1 AND valid_to IS NULL \
               AND v = ?3 AND g = ?1 LIMIT ?4",
        )?;
        let found: Vec<(i64, i64, Vec<u8>)> = stmt
            .query_map(
                rusqlite::params![g, id, value, MAX_RETRACTED_REFERENCES as i64 + 1],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?
            .collect::<rusqlite::Result<_>>()?;
        if found.is_empty() {
            continue;
        }
        let Some(own) = own else {
            return Ok(true);
        };
        if found.len() > MAX_RETRACTED_REFERENCES {
            return Err(refuse(format!(
                "more than {MAX_RETRACTED_REFERENCES} references to {entity} to retract in one batch"
            )));
        }
        let retracted = |(e, a, v): &(i64, i64, Vec<u8>)| {
            own.iter()
                .filter(|(name, _)| name == graph)
                .flat_map(|(_, datums)| datums)
                .any(|d| {
                    d.op == crate::types::Op::Retract
                        && d.entity == *e
                        && d.attribute == *a
                        && d.value.to_bytes() == *v
                })
        };
        if !found.iter().all(retracted) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Per participant (by file id), the pin changes its intent carries: the
/// pins a validated batch records in each validation file (one per check per
/// member file), and the (entity, file) releases every member applies.
/// Derived from the batch input only, so a retry has the same digest.
pub(super) fn plan_pins(
    members: &[Member<'_>],
    validations: &[Validation<'_>],
) -> std::collections::BTreeMap<String, PinChanges> {
    let mut plan: std::collections::BTreeMap<String, PinChanges> = Default::default();
    let file = |owner: &GraphSetOwner| owner.owner.catalog_route().file_id.clone();
    let mut release: Vec<(String, String)> = validations
        .iter()
        .flat_map(|v| {
            v.absences
                .iter()
                .map(move |a| (a.entity.clone(), file(v.owner)))
        })
        .collect();
    release.sort();
    release.dedup();
    for validation in validations {
        let mut add: Vec<_> = validation
            .checks
            .iter()
            .flat_map(|c| {
                members.iter().map(move |m| DependencyPin {
                    graph: c.graph.clone(),
                    entity: c.entity.clone(),
                    type_iri: c.type_iri.clone(),
                    dependent_file_id: file(m.owner),
                })
            })
            .collect();
        add.sort();
        add.dedup();
        if !add.is_empty() {
            plan.entry(file(validation.owner)).or_default().add = add;
        }
    }
    if !release.is_empty() {
        for member in members {
            plan.entry(file(member.owner)).or_default().release = release.clone();
        }
    }
    plan
}

/// Refuse a batch into this file that removes a pinned type, unless every
/// pinning file is released by this batch. Removal = a Retract of
/// (E rdf:type T) in G, or an Assert of it with a `valid_to` bound.
pub(super) fn check_pinned_removals(
    store: &Store,
    graphs: &[(String, Vec<crate::store::Datum>)],
    release: &[(String, String)],
) -> Result<()> {
    check_pinned_axioms(store, graphs, release)?;
    let pins = read_pins(store)?;
    if pins.is_empty() {
        return Ok(());
    }
    let Some(rdf_type) = store.lookup(RDF_TYPE)? else {
        return Ok(());
    };
    for pin in &pins {
        if release
            .iter()
            .any(|(e, f)| *e == pin.entity && *f == pin.dependent_file_id)
        {
            continue;
        }
        let (Some(entity), Some(ty)) = (store.lookup(&pin.entity)?, store.lookup(&pin.type_iri)?)
        else {
            continue;
        };
        let removes = graphs
            .iter()
            .filter(|(graph, _)| *graph == pin.graph)
            .flat_map(|(_, datums)| datums)
            .any(|d| {
                d.entity == entity
                    && d.attribute == rdf_type
                    && matches!(d.value, crate::types::Value::Ref(id) if id == ty)
                    && (d.op == crate::types::Op::Retract || d.valid_to.is_some())
            });
        if removes {
            return Err(refuse(format!(
                "type {} a {} in {} is pinned by dependent file {}; change it in a \
                 participant batch that enlists that file with a reference-absence check",
                pin.entity, pin.type_iri, pin.graph, pin.dependent_file_id
            )));
        }
    }
    Ok(())
}

/// Slice B: the same guard for pinned subclass axioms (released with `via`).
fn check_pinned_axioms(
    store: &Store,
    graphs: &[(String, Vec<crate::store::Datum>)],
    release: &[(String, String)],
) -> Result<()> {
    let pins = read_subclass_pins(store)?;
    let Some(subclass) = (!pins.is_empty())
        .then(|| store.lookup(SUBCLASS_OF))
        .transpose()?
        .flatten()
    else {
        return Ok(());
    };
    for pin in &pins {
        if release
            .iter()
            .any(|(e, f)| *e == pin.via && *f == pin.dependent_file_id)
        {
            continue;
        }
        let (Some(class), Some(parent)) = (store.lookup(&pin.class)?, store.lookup(&pin.parent)?)
        else {
            continue;
        };
        let removes = graphs
            .iter()
            .filter(|(graph, _)| *graph == pin.graph)
            .flat_map(|(_, datums)| datums)
            .any(|d| {
                d.entity == class
                    && d.attribute == subclass
                    && matches!(d.value, crate::types::Value::Ref(id) if id == parent)
                    && (d.op == crate::types::Op::Retract || d.valid_to.is_some())
            });
        if removes {
            return Err(refuse(format!(
                "axiom {class} subClassOf {parent} in {graph} is pinned by dependent file \
                 {dependent}, whose committed batch relies on it for {via}. To release it, run \
                 ONE participant batch with this file as a member (the retraction) and \
                 {dependent} enlisted with ReferenceAbsence {{ entity: {via} }} (as a member \
                 retracting its references to {via}, or as a validation participant once it \
                 holds none). That batch needs writer authority over both files; if \
                 {dependent}'s owner is gone or stopped, escalate to an administrator. There \
                 is no bypass.",
                class = pin.class,
                parent = pin.parent,
                graph = pin.graph,
                dependent = pin.dependent_file_id,
                via = pin.via
            )));
        }
    }
    Ok(())
}

/// Apply one intent's pin changes inside its apply transaction.
pub(super) fn apply_pins(store: &Store, pins: &PinChanges) -> Result<()> {
    for pin in &pins.add {
        store.conn.execute(
            "INSERT OR IGNORE INTO dependency_pins(graph, entity, type_iri, dependent_file_id) \
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![pin.graph, pin.entity, pin.type_iri, pin.dependent_file_id],
        )?;
    }
    for pin in &pins.axioms {
        store.conn.execute(
            "INSERT OR IGNORE INTO subclass_pins(graph, class, parent, via, dependent_file_id) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                pin.graph,
                pin.class,
                pin.parent,
                pin.via,
                pin.dependent_file_id
            ],
        )?;
    }
    for (entity, file) in &pins.release {
        store.conn.execute(
            "DELETE FROM dependency_pins WHERE entity = ?1 AND dependent_file_id = ?2",
            rusqlite::params![entity, file],
        )?;
        store.conn.execute(
            "DELETE FROM subclass_pins WHERE via = ?1 AND dependent_file_id = ?2",
            rusqlite::params![entity, file],
        )?;
    }
    Ok(())
}

/// S4, under the fence before any intent: a member may not remove a type
/// another file pinned unless every pinning file is released by this batch.
pub(super) fn check_member_removals(
    order: &[&GraphSetOwner],
    guards: &[Guard<'_>],
    members: &[Member<'_>],
    plan: &std::collections::BTreeMap<String, PinChanges>,
) -> Result<()> {
    for member in members {
        let at = order
            .iter()
            .position(|owner| std::ptr::eq(*owner, member.owner))
            .ok_or_else(|| refuse("member lookup"))?;
        let release = plan
            .get(&member.owner.owner.catalog_route().file_id)
            .map_or(&[][..], |p| p.release.as_slice());
        check_pinned_removals(&guards[at], &member.batch.graphs, release)?;
    }
    Ok(())
}

/// Ordinary writes into a file refuse to remove a type another file pinned
/// (S4); only a participant release batch may (see [`commit_validated`]).
pub(crate) fn check_pinned_write(
    store: &Store,
    graphs: &[(String, Vec<crate::store::Datum>)],
) -> Result<()> {
    check_pinned_removals(store, graphs, &[])
}
