//! CONSTRUCT / DESCRIBE result building (split from `sparql/mod.rs`, quipu-sd1).

use std::collections::HashSet;

use crate::error::Result;
use crate::store::Store;
use crate::types::Value;

use super::{Bindings, Triple};

/// Result-triple dedupe in first-seen order (aegis-49s). `Vec::contains` made
/// CONSTRUCT quadratic: 160k rows took 18.7 s and the 360k-triple repository
/// share never fit its budget. The key is the exact identity `Triple`'s
/// `PartialEq` uses: subject and predicate text plus the object's
/// `Value::term_key`, which is equivalent to `Value`'s `PartialEq`.
#[derive(Default)]
struct Seen(HashSet<(String, String, Vec<u8>)>);

impl Seen {
    /// True the first time this triple is offered.
    fn first(&mut self, triple: &Triple) -> bool {
        self.0.insert((
            triple.subject.clone(),
            triple.predicate.clone(),
            triple.object.term_key(),
        ))
    }
}

/// Instantiate a CONSTRUCT template with each row of bindings.
pub(super) fn eval_construct(
    store: &Store,
    template: &[spargebra::term::TriplePattern],
    rows: &[Bindings],
) -> Result<Vec<Triple>> {
    use spargebra::term::{NamedNodePattern, TermPattern};

    let mut triples = Vec::new();
    let mut seen = Seen::default();

    // A blank node in the template is a FRESH node for each solution (SPARQL
    // 1.1 section 16.2.1). Reusing the template label for every row fused all
    // solutions into one node: CONSTRUCT { [ rdf:subject ?s ] } collapsed
    // eight reified statements into one (W3C sparql10 construct-3/-4,
    // aegis-soqv1r). The `c<row>-` prefix keeps them apart from data blank
    // nodes too.
    let fresh = |index: usize, label: &str| format!("_:c{index}-{label}");

    for (index, row) in rows.iter().enumerate() {
        for tp in template {
            let subject = match &tp.subject {
                TermPattern::NamedNode(n) => n.as_str().to_string(),
                TermPattern::Variable(v) => match row.get(v.as_str()) {
                    Some(Value::Ref(id)) => store.resolve(*id)?,
                    Some(Value::Str(s)) => s.clone(),
                    _ => continue,
                },
                TermPattern::BlankNode(b) => fresh(index, b.as_str()),
                TermPattern::Literal(_) => continue,
                #[cfg(feature = "shacl")]
                TermPattern::Triple(_) => continue,
            };

            let predicate = match &tp.predicate {
                NamedNodePattern::NamedNode(n) => n.as_str().to_string(),
                NamedNodePattern::Variable(v) => match row.get(v.as_str()) {
                    Some(Value::Ref(id)) => store.resolve(*id)?,
                    Some(Value::Str(s)) => s.clone(),
                    _ => continue,
                },
            };

            let object = match &tp.object {
                TermPattern::NamedNode(n) => {
                    if let Some(id) = store.lookup(n.as_str())? {
                        Value::Ref(id)
                    } else {
                        Value::Str(n.as_str().to_string())
                    }
                }
                TermPattern::Literal(lit) => super::filter::literal_to_value(lit),
                TermPattern::Variable(v) => match row.get(v.as_str()) {
                    Some(val) => val.clone(),
                    None => continue,
                },
                TermPattern::BlankNode(b) => Value::Str(fresh(index, b.as_str())),
                #[cfg(feature = "shacl")]
                TermPattern::Triple(_) => continue,
            };

            let triple = Triple {
                subject,
                predicate,
                object,
            };
            if seen.first(&triple) {
                triples.push(triple);
            }
        }
    }

    Ok(triples)
}

/// Gather all triples for each entity mentioned in the result rows.
pub(super) fn eval_describe(store: &Store, rows: &[Bindings]) -> Result<Vec<Triple>> {
    let mut entity_ids = Vec::new();
    let mut seen_ids = HashSet::new();

    // Collect all Ref values from all bindings.
    for row in rows {
        for val in row.values() {
            if let Value::Ref(id) = val
                && seen_ids.insert(*id)
            {
                entity_ids.push(*id);
            }
        }
    }

    let mut triples = Vec::new();
    let mut seen = Seen::default();
    for eid in &entity_ids {
        let facts = store.entity_facts(*eid)?;
        let subject_iri = store.resolve(*eid)?;
        for fact in &facts {
            let predicate = store.resolve(fact.attribute)?;
            let object = match &fact.value {
                Value::Ref(id) => Value::Ref(*id),
                other => other.clone(),
            };
            let triple = Triple {
                subject: subject_iri.clone(),
                predicate,
                object,
            };
            if seen.first(&triple) {
                triples.push(triple);
            }
        }
    }

    Ok(triples)
}

#[cfg(test)]
#[path = "construct_tests.rs"]
mod tests;
