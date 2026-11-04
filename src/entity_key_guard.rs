//! Identity invariants shared by raw writes, shape edits and optional verification.
use std::collections::{BTreeMap, BTreeSet};

use crate::entity_key::{KeySpec, derive_key, key_spec};
use crate::entity_namespaces::{self, Namespace};
use crate::store::Datum;
use crate::{Error, Op, Result, Store, Value};
use rusqlite::params;

fn refused(message: &str) -> Error {
    Error::InvalidValue(format!(
        "key identity: {message}; supersede into a new identity instead"
    ))
}

/// Historical type assertions retain the identity contract even after retirement.
fn specs(store: &Store, ns: &Namespace, entity: i64, graph: i64) -> Result<Vec<(String, KeySpec)>> {
    let history = store.entity_history_in_graph(entity, graph)?;
    let mut types = BTreeSet::new();
    for fact in history {
        if fact.op == Op::Assert
            && store.resolve(fact.attribute)? == crate::namespace::RDF_TYPE
            && let Value::Ref(id) = fact.value
        {
            let ty = store.resolve(id)?;
            if ns.types.contains(&ty) {
                types.insert(ty);
            }
        }
    }
    if types.is_empty() {
        return Ok(Vec::new());
    }
    let shapes = store
        .get_combined_shapes()?
        .ok_or_else(|| refused("allocated key IRI lacks its shape"))?;
    types
        .into_iter()
        .map(|ty| Ok((ty.clone(), key_spec(&shapes, &ty)?)))
        .collect()
}

/// Runs before inferred/superseding writes can alter the old key values.
/// Existing key fields are immutable, including transient retract/reassert batches.
pub(crate) fn check_write(store: &Store, graph: i64, datums: &[Datum]) -> Result<()> {
    let namespaces = entity_namespaces::list(store)?;
    let keyed: Vec<_> = namespaces.iter().filter(|ns| ns.minting == "key").collect();
    if keyed.is_empty() {
        return Ok(());
    }
    let mut subjects = BTreeMap::new();
    for datum in datums {
        subjects
            .entry(datum.entity)
            .or_insert_with(Vec::new)
            .push(datum);
    }
    for (entity, writes) in subjects {
        let iri = store.resolve(entity)?;
        let Some(ns) = keyed.iter().find(|ns| iri.starts_with(&ns.base_iri)) else {
            continue;
        };
        let contracts = specs(store, ns, entity, graph)?;
        if contracts.is_empty() {
            continue;
        }
        let history = store.entity_history_in_graph(entity, graph)?;
        for datum in writes {
            let predicate = store.resolve(datum.attribute)?;
            for (ty, spec) in &contracts {
                if predicate == crate::namespace::RDF_TYPE
                    && let Value::Ref(id) = &datum.value
                {
                    let value = store.resolve(*id)?;
                    if (value == *ty && datum.op != Op::Assert)
                        || (ns.types.contains(&value) && value != *ty)
                    {
                        return Err(refused("cannot change a key IRI's identity type"));
                    }
                }
                if spec.fields.contains(&predicate) {
                    let original = history
                        .iter()
                        .find(|f| f.op == Op::Assert && f.attribute == datum.attribute);
                    if datum.op != Op::Assert || original.is_none_or(|f| f.value != datum.value) {
                        return Err(refused("cannot retract or replace an allocated key field"));
                    }
                }
            }
        }
    }
    Ok(())
}

/// Freeze the effective derivation (including key kinds/cardinality) once a
/// matching subject has ever been asserted, in any graph. Removing a shape is
/// an edit too. Blank-node spelling and explicit default hash are immaterial.
pub(crate) fn check_shapes(store: &Store, candidate: &str) -> Result<()> {
    let namespaces = entity_namespaces::list(store)?;
    let Some(type_attr) = store.lookup(crate::namespace::RDF_TYPE)? else {
        return Ok(());
    };
    let mut previous = None;
    for ns in namespaces.iter().filter(|ns| ns.minting == "key") {
        for ty in &ns.types {
            let Some(type_id) = store.lookup(ty)? else {
                continue;
            };
            let mut stmt = store
                .conn
                .prepare("SELECT DISTINCT e FROM facts WHERE a=?1 AND v=?2 AND op=1")?;
            let subjects = stmt
                .query_map(params![type_attr, Value::Ref(type_id).to_bytes()], |r| {
                    r.get::<_, i64>(0)
                })?;
            let mut allocated = false;
            for subject in subjects {
                if store.resolve(subject?)?.starts_with(&ns.base_iri) {
                    allocated = true;
                    break;
                }
            }
            if !allocated {
                continue;
            }
            let old = previous.get_or_insert(store.get_combined_shapes()?.unwrap_or_default());
            if key_spec(old, ty)? != key_spec(candidate, ty)? {
                return Err(refused(
                    "cannot change an allocated type's keyFields, kind or strategy",
                ));
            }
        }
    }
    Ok(())
}

/// Optional, local verification of an imported/current reference. No allocation,
/// config application, network fetch or state repair. Unknown namespaces refuse.
pub(crate) fn verify(store: &Store, iri: &str, graph: i64) -> Result<serde_json::Value> {
    let namespaces = entity_namespaces::list(store)?;
    let ns = namespaces
        .iter()
        .find(|ns| ns.minting == "key" && iri.starts_with(&ns.base_iri))
        .ok_or_else(|| refused("reference is not in a key namespace"))?;
    let entity = store
        .lookup(iri)?
        .ok_or_else(|| refused("reference is unknown"))?;
    let facts = store.entity_facts_in_graph(entity, graph)?;
    let shapes = store
        .get_combined_shapes()?
        .ok_or_else(|| refused("reference lacks shapes"))?;
    let mut types = Vec::new();
    for fact in &facts {
        if store.resolve(fact.attribute)? == crate::namespace::RDF_TYPE
            && let Value::Ref(id) = fact.value
        {
            let ty = store.resolve(id)?;
            if ns.types.contains(&ty) {
                types.push(ty);
            }
        }
    }
    types.sort();
    types.dedup();
    if types.len() != 1 {
        return Err(refused(
            "reference needs exactly one selected live key type",
        ));
    }
    let ty = &types[0];
    let spec = key_spec(&shapes, ty)?;
    if spec.strategy == crate::entity_key::Strategy::Lexical && ns.types.len() != 1 {
        return Err(refused("lexical namespace must select exactly one type"));
    }
    let mut terms = Vec::new();
    for field in &spec.fields {
        let attribute = store
            .lookup(field)?
            .ok_or_else(|| refused("missing key field"))?;
        let values: Vec<_> = facts.iter().filter(|f| f.attribute == attribute).collect();
        if values.len() != 1 {
            return Err(refused("reference needs exactly one value per key field"));
        }
        terms.push((
            field.clone(),
            crate::rdf::value_to_term(store, &values[0].value)?,
        ));
    }
    let mut keys = serde_json::Map::new();
    for (field, term) in &terms {
        let value = match term {
            oxrdf::Term::NamedNode(n) => serde_json::json!({"@id":n.as_str()}),
            oxrdf::Term::Literal(l) if l.language().is_none() => {
                serde_json::json!({"@value":l.value(),"@type":l.datatype().as_str()})
            }
            _ => return Err(refused("unsupported key term")),
        };
        keys.insert(field.clone(), value);
    }
    let terms = crate::entity_key::key_terms(&spec, &keys)?;
    if iri != format!("{}{}", ns.base_iri, derive_key(&spec, ty, &terms)) {
        return Err(refused("reference does not match its key triples"));
    }
    Ok(serde_json::json!({"iri":iri,"namespace":ns.prefix,"type":ty,"verified":true}))
}
