//! Reconstruct graph-local definitions from authenticated native records.
use super::{Log, Observable, Policy, Registry, invalid, writer};
use crate::{Result, Store, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn load(store: &Store, graph: i64) -> Result<(Registry, Log)> {
    let graph_iri = store.graph_iri_of(graph);
    let mut registry = Registry::new(&graph_iri)?;
    let mut log = Log::default();
    let (Some(type_id), Some(fluent_type)) = (
        store.lookup(crate::namespace::RDF_TYPE)?,
        store.lookup(&format!("{}Fluent", crate::namespace::QUIPU_FLUENT))?,
    ) else {
        return Ok((registry, log));
    };
    let observable_type = store.lookup(&format!("{}Observable", crate::namespace::QUIPU_FLUENT))?;
    let facts = store.current_facts_in_graph(graph)?;
    let entities: BTreeSet<_> = facts
        .iter()
        .filter(|f| f.attribute == type_id && f.value == Value::Ref(fluent_type))
        .map(|f| f.entity)
        .collect();
    if entities.len() > 256 {
        return Err(invalid("native registry exceeds declaration budget"));
    }
    let mut pending = BTreeMap::new();
    let mut known = BTreeSet::new();
    let mut ready = BTreeSet::new();
    for entity in entities {
        let rows: Vec<_> = facts.iter().filter(|f| f.entity == entity).collect();
        let one = |name: &str| -> Result<Value> {
            let attribute = store
                .lookup(&format!("{}{name}", crate::namespace::QUIPU_FLUENT))?
                .ok_or_else(|| invalid("missing native declaration field"))?;
            let values: Vec<_> = rows.iter().filter(|f| f.attribute == attribute).collect();
            if values.len() != 1 {
                return Err(invalid("missing or ambiguous native declaration field"));
            }
            Ok(values[0].value.clone())
        };
        let iri = |name: &str| -> Result<String> {
            let Value::Ref(id) = one(name)? else {
                return Err(invalid("native declaration requires IRI fields"));
            };
            let value = store.resolve(id)?;
            writer::iri(&value)?;
            Ok(value)
        };
        let policy = match one("currentValuePolicy")? {
            Value::Str(text) => match text.as_str() {
                "latest" => Policy::Latest,
                "max" => Policy::Max,
                "sum" => Policy::Sum,
                "set" => Policy::Set,
                _ => return Err(invalid("unknown native declaration policy")),
            },
            _ => return Err(invalid("native declaration policy requires plain string")),
        };
        if iri("inGraph")? != graph_iri {
            return Err(invalid("foreign native declaration graph"));
        }
        let binding = writer::binding_for(
            store,
            graph,
            &iri("slotSubject")?,
            &iri("slotPredicate")?,
            &iri("propertyShape")?,
        )?;
        if binding.declaration.policy != policy {
            return Err(invalid("native declaration shape interpretation changed"));
        }
        let types: Vec<_> = rows
            .iter()
            .filter(|f| f.attribute == type_id)
            .map(|f| f.value.clone())
            .collect();
        let derived = observable_type.is_some_and(|id| types.contains(&Value::Ref(id)));
        let expected_types = if derived { 2 } else { 1 };
        if types.len() != expected_types {
            return Err(invalid("unexpected native declaration type"));
        }
        let origin = rows[0].tx;
        let auth = store
            .transaction_auth(origin)?
            .ok_or_else(|| invalid("native declaration lacks authenticated origin"))?;
        writer::iri(&auth.principal)?;
        if rows.iter().any(|f| f.tx != origin)
            || store
                .get_transaction(origin)?
                .and_then(|tx| tx.source)
                .as_deref()
                != Some("quipu:fluent")
        {
            return Err(invalid("native declaration origin does not match content"));
        }
        let name = store.resolve(entity)?;
        writer::iri(&name)?;
        known.insert(name.clone());
        if derived {
            if one("computedBy")? != Value::Str(writer::policy_name(policy).into()) {
                return Err(invalid("computedBy and currentValuePolicy differ"));
            }
            let dependency = store
                .lookup(&format!("{}observes", crate::namespace::QUIPU_FLUENT))?
                .ok_or_else(|| invalid("observable dependencies missing"))?;
            let mut observes = BTreeSet::new();
            for row in rows.iter().filter(|f| f.attribute == dependency) {
                let Value::Ref(id) = row.value else {
                    return Err(invalid("observable dependency requires an IRI"));
                };
                let value = store.resolve(id)?;
                writer::iri(&value)?;
                observes.insert(value);
            }
            if rows.len() != 8 + observes.len() {
                return Err(invalid("unexpected native observable content"));
            }
            pending.insert(
                name.clone(),
                Observable {
                    graph: graph_iri.clone(),
                    name,
                    observes,
                    computed_by: policy,
                },
            );
        } else {
            if rows.len() != 6 {
                return Err(invalid("unexpected native fluent content"));
            }
            registry.declare(&name, binding)?;
            log.merge(&writer::read_log(store, graph, &name, policy)?)?;
            ready.insert(name);
        }
    }
    if pending.values().any(|o| {
        o.observes
            .iter()
            .any(|dependency| !known.contains(dependency))
    }) {
        return Err(invalid("unknown or cross-graph observable dependency"));
    }
    // Resolve definition order independently of dictionary allocation order.
    while !pending.is_empty() {
        let next = pending
            .iter()
            .find(|(_, o)| o.observes.iter().all(|name| ready.contains(name)))
            .map(|(name, _)| name.clone());
        let Some(name) = next else {
            return Err(invalid("observable dependency cycle"));
        };
        registry.register(pending.remove(&name).expect("selected definition"))?;
        ready.insert(name);
    }
    Ok((registry, log))
}
