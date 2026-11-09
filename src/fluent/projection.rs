//! Reconstructible, graph-scoped RDF 1.2 current projection. Never writes slots.
use super::{Current, Scalar, invalid};
use crate::{Result, Store, Value};
use oxrdf::{Literal, NamedNode, Term, Triple};
use sha2::{Digest, Sha256};
use std::str::FromStr;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Projection {
    pub graph: String,
    pub statements: Vec<Triple>,
}

fn project(
    graph: &str,
    fluent: &str,
    subject: &str,
    predicate: &str,
    current: &Current,
) -> Result<Projection> {
    let node = |iri: &str| {
        NamedNode::new(iri.to_owned()).map_err(|_| invalid("projection requires absolute IRIs"))
    };
    if graph
        .len()
        .saturating_add(fluent.len())
        .saturating_add(subject.len())
        .saturating_add(predicate.len())
        > 65536
    {
        return Err(invalid("projection descriptor exceeds byte budget"));
    }
    node(graph)?;
    node(fluent)?;
    let subject = node(subject)?;
    let predicate = node(predicate)?;
    if current.values.len() > 256 || current.produced_by.len() > 4096 {
        return Err(invalid("projection exceeds value or provenance budget"));
    }
    let count = current
        .values
        .len()
        .checked_mul(current.produced_by.len().saturating_add(1))
        .ok_or_else(|| invalid("projection statement budget overflow"))?;
    if count > 4096
        || current
            .produced_by
            .iter()
            .try_fold(0usize, |n, e| n.checked_add(e.len()))
            .is_none_or(|n| n > 65536)
    {
        return Err(invalid(
            "projection exceeds combined statement/provenance budget",
        ));
    }
    let mut statements = Vec::new();
    let mut bytes = 0usize;
    let mut push = |statement: Triple| -> Result<()> {
        let retained = statement
            .to_string()
            .len()
            .saturating_add(8 * std::mem::size_of::<Triple>());
        bytes = bytes
            .checked_add(retained)
            .ok_or_else(|| invalid("projection byte budget overflow"))?;
        if bytes > 8 * 1024 * 1024 {
            return Err(invalid("projection exceeds byte budget"));
        }
        statements.push(statement);
        Ok(())
    };
    for value in &current.values {
        let object = match value {
            Scalar::Integer(n) => Term::Literal(Literal::new_typed_literal(
                n.to_string(),
                oxrdf::vocab::xsd::INTEGER,
            )),
            Scalar::Term(text) => {
                if text.len() > 65536 {
                    return Err(invalid("projection value exceeds byte budget"));
                }
                Term::from_str(text).map_err(|_| invalid("projection value is not an RDF term"))?
            }
        };
        let inner = Triple::new(subject.clone(), predicate.clone(), object);
        // Use the SAME validated, structured native representation as storage.
        let encoded = crate::rdf_triple::TripleValue::from_ox(&inner)?;
        let identity = serde_json::to_vec(&(graph, fluent, &encoded, &current.produced_by))
            .map_err(|_| invalid("projection identity serialization failed"))?;
        let reifier = node(&format!(
            "urn:quipu:fluent-projection:{:x}",
            Sha256::digest(identity)
        ))?;
        push(Triple::new(
            reifier.clone(),
            node("http://www.w3.org/1999/02/22-rdf-syntax-ns#reifies")?,
            Term::Triple(Box::new(inner)),
        ))?;
        // These are computation dependencies, never assertions that every
        // delta asserted the aggregate result. Raw event identities remain.
        for event in &current.produced_by {
            push(Triple::new(
                reifier.clone(),
                node("http://www.w3.org/ns/prov#wasDerivedFrom")?,
                node(event)?,
            ))?;
        }
    }
    Ok(Projection {
        graph: graph.into(),
        statements,
    })
}

impl Store {
    /// Internal read-only projection, with no HTTP/CLI or write admission grant.
    #[allow(dead_code)]
    pub(crate) fn fluent_projection_native(&self, graph: i64, name: &str) -> Result<Projection> {
        // Registry reconstruction authenticates the descriptor and every input.
        let current = self.fluent_current_native(graph, name)?;
        let entity = self
            .lookup(name)?
            .ok_or_else(|| invalid("unknown projection fluent"))?;
        let facts = self.entity_facts_in_graph(entity, graph)?;
        let field = |name: &str| -> Result<String> {
            let attribute = self
                .lookup(&format!("{}{name}", crate::namespace::QUIPU))?
                .ok_or_else(|| invalid("missing projection descriptor field"))?;
            let values: Vec<_> = facts.iter().filter(|f| f.attribute == attribute).collect();
            if values.len() != 1 {
                return Err(invalid("ambiguous projection descriptor"));
            }
            let Value::Ref(id) = values[0].value else {
                return Err(invalid("projection descriptor field is not an IRI"));
            };
            self.resolve(id)
        };
        project(
            &self.graph_iri_of(graph),
            name,
            &field("slotSubject")?,
            &field("slotPredicate")?,
            &current,
        )
    }
}

#[cfg(test)]
#[path = "projection_tests.rs"]
mod tests;
