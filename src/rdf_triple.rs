//! First-class, dictionary-independent triple terms for fluent projections.
//!
//! This qualified subset has IRI subjects/predicates and IRI or literal objects.
//! Nested triples, blank nodes and directional literals remain unsupported.
use crate::{Error, Result};
use oxrdf::{Literal, NamedNode};
use serde::{Deserialize, Serialize};

/// Object of a qualified projection triple. Literal lexical identity is exact.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum TripleObject {
    /// An absolute IRI, independent of any store's dictionary allocation.
    Iri { iri: String },
    /// An RDF literal, including its datatype and optional language tag.
    Literal {
        lexical: String,
        datatype: String,
        language: Option<String>,
    },
}

/// A genuine triple term; its tagged storage never aliases a string literal.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "WireTriple")]
pub struct TripleValue {
    subject: String,
    predicate: String,
    object: TripleObject,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireTriple {
    subject: String,
    predicate: String,
    object: TripleObject,
}
impl TryFrom<WireTriple> for TripleValue {
    type Error = Error;
    fn try_from(wire: WireTriple) -> Result<Self> {
        Self::new(wire.subject, wire.predicate, wire.object)
    }
}

const MAX_BYTES: usize = 64 * 1024;

impl TripleValue {
    /// Construct the supported projection subset, refusing malformed RDF terms.
    pub fn new(subject: String, predicate: String, object: TripleObject) -> Result<Self> {
        let mut value = Self {
            subject,
            predicate,
            object,
        };
        if let TripleObject::Literal {
            language: Some(lang),
            ..
        } = &mut value.object
        {
            *lang = lang.to_ascii_lowercase();
        }
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<()> {
        let object_bytes = match &self.object {
            TripleObject::Iri { iri } => iri.len(),
            TripleObject::Literal {
                lexical,
                datatype,
                language,
            } => lexical
                .len()
                .saturating_add(datatype.len())
                .saturating_add(language.as_ref().map_or(0, String::len)),
        };
        if self
            .subject
            .len()
            .saturating_add(self.predicate.len())
            .saturating_add(object_bytes)
            > MAX_BYTES
        {
            return Err(Error::InvalidValue(
                "projection triple exceeds byte budget".into(),
            ));
        }
        let invalid = |e| Error::InvalidValue(format!("invalid projection triple: {e}"));
        NamedNode::new(&self.subject).map_err(invalid)?;
        NamedNode::new(&self.predicate).map_err(invalid)?;
        match &self.object {
            TripleObject::Iri { iri } => {
                NamedNode::new(iri).map_err(invalid)?;
            }
            TripleObject::Literal {
                lexical,
                datatype,
                language,
            } => {
                NamedNode::new(datatype).map_err(invalid)?;
                if let Some(lang) = language {
                    if datatype != oxrdf::vocab::rdf::LANG_STRING.as_str() {
                        return Err(Error::InvalidValue(
                            "triple language/datatype mismatch".into(),
                        ));
                    }
                    Literal::new_language_tagged_literal(lexical, lang)
                        .map_err(|e| Error::InvalidValue(format!("bad triple language: {e}")))?;
                } else if datatype == oxrdf::vocab::rdf::LANG_STRING.as_str() {
                    return Err(Error::InvalidValue("triple language is missing".into()));
                }
            }
        }
        if self.encode().len() > MAX_BYTES {
            return Err(Error::InvalidValue(
                "projection triple exceeds byte budget".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        self.subject
            .capacity()
            .saturating_add(self.predicate.capacity())
            .saturating_add(match &self.object {
                TripleObject::Iri { iri } => iri.capacity(),
                TripleObject::Literal {
                    lexical,
                    datatype,
                    language,
                } => lexical
                    .capacity()
                    .saturating_add(datatype.capacity())
                    .saturating_add(language.as_ref().map_or(0, String::capacity)),
            })
    }

    pub(crate) fn encode(&self) -> Vec<u8> {
        // Only strings and tagged structs: serialization cannot fail.
        serde_json::to_vec(self).expect("triple term consists only of strings")
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_BYTES {
            return Err(Error::InvalidValue(
                "projection triple exceeds byte budget".into(),
            ));
        }
        let value: Self = serde_json::from_slice(bytes)
            .map_err(|e| Error::InvalidValue(format!("invalid triple storage: {e}")))?;
        value.validate()?;
        Ok(value)
    }

    /// The subject IRI.
    pub fn subject(&self) -> &str {
        &self.subject
    }
    /// The predicate IRI.
    pub fn predicate(&self) -> &str {
        &self.predicate
    }
    /// The structured object.
    pub fn object(&self) -> &TripleObject {
        &self.object
    }

    #[cfg(feature = "shacl")]
    pub(crate) fn from_ox(triple: &oxrdf::Triple) -> Result<Self> {
        let subject = match &triple.subject {
            oxrdf::NamedOrBlankNode::NamedNode(n) => n.as_str(),
            oxrdf::NamedOrBlankNode::BlankNode(_) => {
                return Err(Error::InvalidValue(
                    "blank projection subject unsupported".into(),
                ));
            }
        };
        let object_bytes = match &triple.object {
            oxrdf::Term::NamedNode(n) => n.as_str().len(),
            oxrdf::Term::Literal(l) => l
                .value()
                .len()
                .saturating_add(l.datatype().as_str().len())
                .saturating_add(l.language().map_or(0, str::len)),
            _ => {
                return Err(Error::InvalidValue(
                    "nested/blank projection object unsupported".into(),
                ));
            }
        };
        if subject
            .len()
            .saturating_add(triple.predicate.as_str().len())
            .saturating_add(object_bytes)
            > MAX_BYTES
        {
            return Err(Error::InvalidValue(
                "projection triple exceeds byte budget".into(),
            ));
        }
        if matches!(&triple.object, oxrdf::Term::Literal(l) if l.direction().is_some()) {
            return Err(Error::InvalidValue(
                "directional projection literal unsupported".into(),
            ));
        }
        let object = match &triple.object {
            oxrdf::Term::NamedNode(n) => TripleObject::Iri {
                iri: n.as_str().to_owned(),
            },
            oxrdf::Term::Literal(l) => TripleObject::Literal {
                lexical: l.value().to_owned(),
                datatype: l.datatype().as_str().to_owned(),
                language: l.language().map(str::to_owned),
            },
            _ => {
                return Err(Error::InvalidValue(
                    "nested/blank projection object unsupported".into(),
                ));
            }
        };
        Self::new(
            subject.to_owned(),
            triple.predicate.as_str().to_owned(),
            object,
        )
    }

    #[cfg(feature = "shacl")]
    pub(crate) fn to_ox(&self) -> Result<oxrdf::Triple> {
        let object = match &self.object {
            TripleObject::Iri { iri } => {
                oxrdf::Term::NamedNode(NamedNode::new_unchecked(iri.clone()))
            }
            TripleObject::Literal {
                lexical,
                datatype,
                language,
            } => oxrdf::Term::Literal(if let Some(lang) = language {
                Literal::new_language_tagged_literal(lexical, lang)
                    .map_err(|e| Error::InvalidValue(format!("bad triple language: {e}")))?
            } else {
                Literal::new_typed_literal(lexical, NamedNode::new_unchecked(datatype.clone()))
            }),
        };
        Ok(oxrdf::Triple::new(
            NamedNode::new_unchecked(self.subject.clone()),
            NamedNode::new_unchecked(self.predicate.clone()),
            object,
        ))
    }
}

#[cfg(test)]
#[path = "rdf_triple_tests.rs"]
mod tests;
