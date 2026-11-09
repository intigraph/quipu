//! Explicit property-shape declarations; unmarked properties stay ordinary slots.
use super::{Policy, invalid};
use crate::Result;
use oxrdf::{NamedOrBlankNode, Term};
use std::collections::{BTreeMap, BTreeSet};

const SH_PATH: &str = "http://www.w3.org/ns/shacl#path";

/// One property shape's fluent declaration; shape targeting remains the
/// caller's responsibility, just as for ordinary SHACL bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub shape: String,
    pub predicate: String,
    pub policy: Policy,
}

/// Parse explicit `qp:currentValuePolicy "latest|max|sum|set"` annotations.
/// Ambiguous/malformed policies and non-IRI paths refuse, never default.
pub fn declarations(turtle: &str) -> Result<Vec<Declaration>> {
    let parser = oxttl::TurtleParser::new();
    let mut paths: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut policies: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for triple in parser.for_reader(turtle.as_bytes()) {
        let triple = triple.map_err(|_| invalid("invalid declaration Turtle"))?;
        let subject = match triple.subject {
            NamedOrBlankNode::NamedNode(n) => n.as_str().to_owned(),
            NamedOrBlankNode::BlankNode(n) => format!("_:{n}"),
        };
        match triple.predicate.as_str() {
            SH_PATH => {
                let value = match triple.object {
                    Term::NamedNode(n) => n.as_str().to_owned(),
                    _ => String::new(),
                };
                paths.entry(subject).or_default().insert(value);
            }
            predicate
                if predicate.strip_prefix(crate::namespace::QUIPU)
                    == Some("currentValuePolicy") =>
            {
                let value = match triple.object {
                    Term::Literal(n)
                        if n.language().is_none() && n.datatype() == oxrdf::vocab::xsd::STRING =>
                    {
                        n.value().to_owned()
                    }
                    _ => String::new(),
                };
                policies.entry(subject).or_default().insert(value);
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for (shape, values) in policies {
        oxrdf::NamedNode::new(shape.as_str())
            .map_err(|_| invalid("fluent property shape identity must be an IRI"))?;
        if values.len() != 1 {
            return Err(invalid(
                "property shape requires exactly one current-value policy",
            ));
        }
        let policy = match values.first().map(String::as_str) {
            Some("latest") => Policy::Latest,
            Some("max") => Policy::Max,
            Some("sum") => Policy::Sum,
            Some("set") => Policy::Set,
            _ => return Err(invalid("unknown current-value policy")),
        };
        let paths = paths
            .get(&shape)
            .ok_or_else(|| invalid("fluent declaration lacks sh:path"))?;
        if paths.len() != 1 || paths.first().is_some_and(String::is_empty) {
            return Err(invalid("fluent declaration requires one IRI path"));
        }
        out.push(Declaration {
            shape,
            predicate: paths.first().expect("one path").clone(),
            policy,
        });
    }
    Ok(out)
}
