//! Terms a proposal's Turtle names (aegis-493), split from `proposal_plan.rs`.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::error::{Error, Result};

const SH: &str = "http://www.w3.org/ns/shacl#";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const CLASS_TYPES: [&str; 2] = [
    "http://www.w3.org/2000/01/rdf-schema#Class",
    "http://www.w3.org/2002/07/owl#Class",
];
const PROPERTY_TYPES: [&str; 4] = [
    "http://www.w3.org/1999/02/22-rdf-syntax-ns#Property",
    "http://www.w3.org/2002/07/owl#ObjectProperty",
    "http://www.w3.org/2002/07/owl#DatatypeProperty",
    "http://www.w3.org/2002/07/owl#AnnotationProperty",
];

/// Terms named by a Turtle fragment.
#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct Terms {
    /// Classes named (targets, `sh:class`, typed classes, subclass/domain/range ends).
    pub classes: BTreeSet<String>,
    /// Properties named (`sh:path`, typed properties, subproperty ends, domain/range subjects).
    pub properties: BTreeSet<String>,
    /// Shapes that are also classes: SHACL targets their instances implicitly.
    #[serde(skip)]
    pub(super) implicit_targets: BTreeSet<String>,
    /// Why the fragment cannot be planned exactly, if it cannot.
    #[serde(skip)]
    pub(super) undetermined: Vec<String>,
}

impl Terms {
    pub(super) fn all(&self) -> BTreeSet<&str> {
        self.classes
            .iter()
            .chain(&self.properties)
            .map(String::as_str)
            .collect()
    }
}

/// Classes and properties `turtle` names, by the RDFS/OWL/SHACL predicates
/// that name them.
///
/// # Errors
/// The fragment is not valid Turtle.
pub fn terms(turtle: &str) -> Result<Terms> {
    let mut t = Terms::default();
    let (mut shapes, mut typed_classes) = (BTreeSet::new(), BTreeSet::new());
    let parser = oxrdfio::RdfParser::from_format(oxrdfio::RdfFormat::Turtle);
    for quad in parser.for_reader(turtle.as_bytes()) {
        let quad = quad.map_err(|e| Error::InvalidValue(format!("proposal Turtle: {e}")))?;
        let subject = match &quad.subject {
            oxrdf::NamedOrBlankNode::NamedNode(n) => Some(n.as_str().to_owned()),
            oxrdf::NamedOrBlankNode::BlankNode(_) => None,
        };
        let predicate = quad.predicate.as_str();
        if predicate == format!("{SH}target") {
            t.undetermined.push(
                "a SPARQL-based sh:target selects focus nodes this plan cannot enumerate".into(),
            );
        }
        let oxrdf::Term::NamedNode(object) = &quad.object else {
            continue;
        };
        let o = object.as_str().to_owned();
        match (predicate, subject) {
            (RDF_TYPE, Some(s)) if CLASS_TYPES.contains(&o.as_str()) => {
                typed_classes.insert(s.clone());
                t.classes.insert(s);
            }
            (RDF_TYPE, Some(s)) if PROPERTY_TYPES.contains(&o.as_str()) => {
                t.properties.insert(s);
            }
            (RDF_TYPE, Some(s)) if o == format!("{SH}NodeShape") => {
                shapes.insert(s);
            }
            ("http://www.w3.org/2000/01/rdf-schema#subClassOf", s) => {
                t.classes.extend(s);
                t.classes.insert(o);
            }
            ("http://www.w3.org/2000/01/rdf-schema#subPropertyOf", s) => {
                t.properties.extend(s);
                t.properties.insert(o);
            }
            (
                "http://www.w3.org/2000/01/rdf-schema#domain"
                | "http://www.w3.org/2000/01/rdf-schema#range",
                s,
            ) => {
                t.properties.extend(s);
                if !o.starts_with("http://www.w3.org/2001/XMLSchema#") {
                    t.classes.insert(o);
                }
            }
            (p, _) => match p.strip_prefix(SH) {
                Some("targetClass" | "class") => {
                    t.classes.insert(o);
                }
                Some("path" | "targetSubjectsOf" | "targetObjectsOf") => {
                    t.properties.insert(o);
                }
                _ => {}
            },
        }
    }
    t.implicit_targets = shapes.intersection(&typed_classes).cloned().collect();
    Ok(t)
}
