//! Indexed graph slices for fluent binding (aegis-550z).
//!
//! `targets::bindings` reads exactly four things from the selected graph's
//! data: the subject's `rdf:type` objects and outgoing predicates, the
//! predicates of facts whose object is the subject (`sh:targetObjectsOf`), and
//! the graph's `rdfs:subClassOf` facts (class closure). `sh:targetNode` reads
//! no data. Selecting over this slice is therefore identical to selecting over
//! the whole graph, without serializing facts that cannot change the answer.
use super::invalid;
use crate::{Fact, Result, Store, Value};

/// Upper bound on one binding slice. The slice no longer grows with unrelated
/// facts, but a graph with a huge class hierarchy or a subject with a huge
/// in-degree would still be reserialized under the write lock per append, so
/// that case refuses instead of stalling every writer.
pub(super) const BINDING_SLICE_LIMIT: usize = 50_000;

pub(super) fn binding_facts(store: &Store, graph: i64, subject: &str) -> Result<Vec<Fact>> {
    let mut facts = Vec::new();
    if let Some(id) = store.lookup(subject)? {
        facts.extend(store.entity_facts_in_graph(id, graph)?);
        facts.extend(store.current_facts_with_value_in_graph(&Value::Ref(id), graph)?);
    }
    if let Some(sub_class_of) = store.lookup(crate::namespace::RDFS_SUBCLASS_OF)? {
        facts.extend(store.current_facts_for_attributes_in_graph(&[sub_class_of], graph)?);
    }
    if facts.len() > BINDING_SLICE_LIMIT {
        return Err(invalid(
            "fluent binding slice exceeds 50000 facts (aegis-550z); declare the fluent in a smaller graph",
        ));
    }
    Ok(facts)
}

#[cfg(test)]
include!("slice_tests.rs");
