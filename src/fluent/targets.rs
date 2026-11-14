//! Resolve fluent declarations against one explicitly selected data graph.
use super::{Declaration, declarations, invalid};
use crate::Result;
use crate::merge3::{FocusView, TripleView};
use oxrdf::{NamedNode, Term, Triple};
use std::collections::{BTreeMap, BTreeSet};

const SH: &str = "http://www.w3.org/ns/shacl#";
const CLASS: &str = "http://www.w3.org/2000/01/rdf-schema#Class";

/// A selected declaration. Graph identity is explicit, never an all-graph union.
/// This is target matching, not authorization or proof of the data's origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub graph: String,
    pub subject: String,
    pub declaration: Declaration,
}

fn triples(turtle: &str) -> Result<Vec<Triple>> {
    oxttl::TurtleParser::new()
        .for_reader(turtle.as_bytes())
        .map(|t| t.map_err(|_| invalid("invalid target-resolution Turtle")))
        .collect()
}

fn active(properties: &[(String, Term)]) -> Result<bool> {
    let values: Vec<_> = properties
        .iter()
        .filter(|(p, _)| p == &format!("{SH}deactivated"))
        .map(|(_, o)| o)
        .collect();
    match values.as_slice() {
        [] => Ok(true),
        [Term::Literal(value)] if value.datatype() == oxrdf::vocab::xsd::BOOLEAN => {
            match value.value() {
                "true" | "1" => Ok(false),
                "false" | "0" => Ok(true),
                _ => Err(invalid("invalid shape deactivation boolean")),
            }
        }
        _ => Err(invalid("ambiguous shape deactivation")),
    }
}

fn selects(
    node: &str,
    properties: &[(String, Term)],
    subject: &String,
    view: &TripleView,
) -> Result<bool> {
    if !active(properties)? {
        return Ok(false);
    }
    let mut selected = false;
    for (predicate, object) in properties {
        let kind = predicate.strip_prefix(SH);
        let iri = || match object {
            Term::NamedNode(n) => Ok(n.as_str()),
            _ => Err(invalid("target class/predicate must be an IRI")),
        };
        // Evaluate every declaration, even after a positive match, so custom or
        // malformed targets cannot hide behind an earlier supported target.
        selected |= match kind {
            Some("targetNode") => view.is_term(subject, &object.to_string())?,
            Some("targetClass") => view.instance_of(subject, iri()?)?,
            Some("targetSubjectsOf") => view.has_out(subject, iri()?)?,
            Some("targetObjectsOf") => view.has_in(subject, iri()?)?,
            Some(name) if name.starts_with("target") => {
                return Err(invalid("unsupported fluent shape target"));
            }
            _ if predicate == crate::namespace::RDF_TYPE
                && matches!(object, Term::NamedNode(n) if n.as_str() == CLASS) =>
            {
                let class = node
                    .strip_prefix('<')
                    .and_then(|n| n.strip_suffix('>'))
                    .ok_or_else(|| invalid("implicit target class must be an IRI"))?;
                view.instance_of(subject, class)?
            }
            _ => false,
        };
    }
    Ok(selected)
}

/// Match explicit fluent property shapes, including their owning node shapes,
/// against `data` serialized from exactly `graph`. The Store caller must supply
/// the selected graph's data and enforce its normal admission independently.
/// Bare predicate annotations select nothing; ambiguous same-slot declarations
/// and unsupported custom targets refuse rather than enabling a global switch.
pub fn bindings(shapes: &str, data: &str, graph: &str, subject: &str) -> Result<Vec<Binding>> {
    NamedNode::new(graph).map_err(|_| invalid("selected graph must be an IRI"))?;
    let subject_term = NamedNode::new(subject)
        .map_err(|_| invalid("fluent subject must be an IRI"))?
        .to_string();
    let shape_triples = triples(shapes)?;
    let data = triples(data)?;
    let view = TripleView::new(&data);
    let mut properties: BTreeMap<String, Vec<(String, Term)>> = BTreeMap::new();
    let mut parents: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for triple in shape_triples {
        let node = triple.subject.to_string();
        if triple.predicate.as_str() == format!("{SH}property") {
            parents
                .entry(triple.object.to_string())
                .or_default()
                .insert(node.clone());
        }
        properties
            .entry(node)
            .or_default()
            .push((triple.predicate.as_str().into(), triple.object));
    }
    let mut out = Vec::new();
    let mut paths = BTreeSet::new();
    for declaration in declarations(shapes)? {
        let node = NamedNode::new(declaration.shape.as_str())
            .map_err(|_| invalid("invalid property shape"))?
            .to_string();
        let own = properties
            .get(&node)
            .ok_or_else(|| invalid("missing property shape"))?;
        if !active(own)? {
            continue;
        }
        let mut selected = selects(&node, own, &subject_term, &view)?;
        for parent in parents.get(&node).into_iter().flatten() {
            selected |= selects(parent, &properties[parent], &subject_term, &view)?;
        }
        if selected {
            if !paths.insert(declaration.predicate.clone()) {
                return Err(invalid("multiple fluent declarations select the same slot"));
            }
            out.push(Binding {
                graph: graph.into(),
                subject: subject.into(),
                declaration,
            });
        }
    }
    Ok(out)
}
