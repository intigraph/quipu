//! Refuse SHACL-SPARQL shapes, which quipu does not evaluate (aegis-rk4).
//!
//! quipu evaluates SHACL Core. A SPARQL-based constraint (SHACL 1.0 §5) or a
//! SPARQL-based constraint component (§6) used to load and then never fire:
//! data that violated it conformed and was written, so the write gate claimed
//! more than it checked. A shape is now enforced or refused, never ignored.
//!
//! The check runs wherever shapes enter, beside the cycle guard: the validator
//! (every validation, per-request shapes included, aegis-d19) and the shape
//! registry. Supporting SHACL-SPARQL is planned separately (aegis-1c8q); until
//! all 22 SHACL-SPARQL conformance cases pass, quipu makes no such claim.
//!
//! `sh:prefixes` and `sh:declare` are left alone: they only declare prefixes
//! and carry no constraint of their own.

use std::collections::BTreeMap;

use oxrdf::{NamedOrBlankNode, Term};

use crate::{Error, Result};

const SH: &str = "http://www.w3.org/ns/shacl#";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

/// Predicates that carry a SPARQL-based constraint or validator.
const PREDICATES: &[&str] = &[
    "sparql",
    "select",
    "ask",
    "validator",
    "nodeValidator",
    "propertyValidator",
];
/// Classes that declare one.
const CLASSES: &[&str] = &[
    "SPARQLConstraint",
    "SPARQLSelectValidator",
    "SPARQLAskValidator",
    "ConstraintComponent",
];

/// The stable start of every refusal message, for callers that match on it.
pub const REFUSAL: &str = "unsupported SHACL-SPARQL";

/// One SHACL-SPARQL use found in a shapes graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported {
    /// The node that carries it: an IRI, or `_:label` for a blank node.
    pub node: String,
    /// The nearest named shape that reaches `node`, when `node` is blank.
    pub shape: Option<String>,
    /// The SHACL term: `sh:sparql`, `sh:select`, `sh:ConstraintComponent`...
    pub feature: String,
}

fn key_of_subject(s: &NamedOrBlankNode) -> String {
    match s {
        NamedOrBlankNode::NamedNode(n) => n.as_str().to_string(),
        NamedOrBlankNode::BlankNode(b) => format!("_:{}", b.as_str()),
    }
}

// Not a `match`: the `Term` variants depend on oxrdf's features (see
// `shacl_cycles`).
fn key_of_term(t: &Term) -> Option<String> {
    if let Term::NamedNode(n) = t {
        return Some(n.as_str().to_string());
    }
    if let Term::BlankNode(b) = t {
        return Some(format!("_:{}", b.as_str()));
    }
    None
}

/// The first SHACL-SPARQL use in `shapes_turtle`, in document order.
pub fn find_unsupported(shapes_turtle: &str) -> Result<Option<Unsupported>> {
    let parser = oxttl::TurtleParser::new()
        .with_base_iri("http://quipu.local/shapes")
        .map_err(|e| Error::InvalidValue(format!("shapes base IRI: {e}")))?;
    let mut found: Option<(String, String)> = None;
    // Each node to one node that refers to it, to name a blank node's shape.
    let mut parent: BTreeMap<String, String> = BTreeMap::new();
    for triple in parser.for_reader(shapes_turtle.as_bytes()) {
        let triple = triple.map_err(|e| Error::InvalidValue(format!("SHACL parse error: {e}")))?;
        let subject = key_of_subject(&triple.subject);
        let object = key_of_term(&triple.object);
        if let Some(object) = &object {
            parent
                .entry(object.clone())
                .or_insert_with(|| subject.clone());
        }
        if found.is_some() {
            continue;
        }
        let predicate = triple.predicate.as_str();
        let feature = match predicate.strip_prefix(SH) {
            Some(local) if PREDICATES.contains(&local) => Some(local),
            _ if predicate == RDF_TYPE => object
                .as_deref()
                .and_then(|o| o.strip_prefix(SH))
                .filter(|local| CLASSES.contains(local)),
            _ => None,
        };
        if let Some(local) = feature {
            found = Some((subject, format!("sh:{local}")));
        }
    }
    Ok(found.map(|(node, feature)| {
        let shape = node
            .starts_with("_:")
            .then(|| named_ancestor(&parent, &node))
            .flatten();
        Unsupported {
            node,
            shape,
            feature,
        }
    }))
}

/// The nearest IRI above a blank node. Bounded by the map's size, so a
/// cyclic structure cannot loop.
fn named_ancestor(parent: &BTreeMap<String, String>, node: &str) -> Option<String> {
    let mut current = node;
    for _ in 0..parent.len() {
        let up = parent.get(current)?;
        if !up.starts_with("_:") {
            return Some(up.clone());
        }
        current = up.as_str();
    }
    None
}

/// Refuse a shapes graph that uses SHACL-SPARQL, naming where.
pub fn refuse_unsupported_shapes(shapes_turtle: &str) -> Result<()> {
    let Some(found) = find_unsupported(shapes_turtle)? else {
        return Ok(());
    };
    let place = match &found.shape {
        Some(shape) => format!("{} (in shape {shape})", found.node),
        None => found.node.clone(),
    };
    Err(Error::InvalidValue(format!(
        "{REFUSAL}: {place} uses {}. quipu evaluates SHACL Core only, so this \
         constraint would never be checked; it is refused rather than loaded and \
         ignored. Express it with Core constraints, or remove it.",
        found.feature
    )))
}

#[cfg(test)]
#[path = "shacl_unsupported_tests.rs"]
mod tests;
