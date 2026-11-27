//! Bounded cross-graph type dependencies for participant batches
//! (aegis-n0wc.5 Phase 3 S2b). A dependency is exactly "entity E has
//! rdf:type T in graph G", where G belongs to a validation-only participant
//! of the same pinned catalog. Its owner is admitted and fenced like any
//! participant (a durable intent with an empty payload), checked under that
//! fence BEFORE any intent, and never written. Anything else refuses before
//! admission. Not provided: inverse invalidation of a later write to G,
//! SHACL-derived dependencies, unbounded or global constraints.
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

/// A validation-only participant and the checks it must satisfy.
pub struct Validation<'a> {
    pub owner: &'a GraphSetOwner,
    pub checks: Vec<TypeDependency>,
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
    let total: usize = validations.iter().map(|v| v.checks.len()).sum();
    if total > MAX_DEPENDENCY_CHECKS {
        return Err(refuse(format!(
            "dependency bound exceeded: {total} checks over {MAX_DEPENDENCY_CHECKS}"
        )));
    }
    for validation in validations {
        if validation.checks.is_empty() {
            return Err(refuse("a validation participant declares no dependency"));
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
pub(super) fn empty_payload(owner: &GraphSetOwner) -> Vec<u8> {
    let route = owner.owner.catalog_route();
    encode_payload(&GraphSetBatch {
        store_id: route.store_id.clone(),
        log_epoch: route.log_epoch.clone(),
        graphs: Vec::new(),
        timestamp: String::new(),
        actor: None,
        source: None,
    })
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
            (v.owner.owner.catalog_route().file_id.clone(), checks)
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
) -> Result<()> {
    for validation in validations {
        let at = order
            .iter()
            .position(|owner| std::ptr::eq(*owner, validation.owner))
            .ok_or_else(|| refuse("validation participant lookup"))?;
        let store = &guards[at];
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
