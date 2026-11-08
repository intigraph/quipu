//! Which IRIs the vocabulary terms Quipu mints are written under.
//!
//! Quipu writes a small vocabulary of its own (the golden-path predicates
//! such as `stepOf`, and the classes its drafts declare) under the store's
//! `base_ns`. That namespace also names the individuals minted from episode
//! node names, so it can never change on an existing store without forking
//! the identity of every named entity.
//!
//! A [`VocabularyMap`] moves the terms without moving the individuals. It is
//! DATA a store loads (`[quipu] vocabulary_map`), never a list compiled into
//! Quipu: the map belongs to whichever vocabulary pack published the terms,
//! and Quipu does not know or name it. The map is read from Turtle:
//!
//! ```turtle
//! <http://example.org/kb/stepOf>
//!     owl:equivalentProperty <https://example.com/vocab#stepOf> .
//! ```
//!
//! For each `owl:equivalentProperty` or `owl:equivalentClass` statement whose
//! SUBJECT is a term Quipu would mint, the OBJECT is written instead. Every
//! other term stays under `base_ns`. No map means every term stays under
//! `base_ns`, which is the behaviour of a store that never configured one.
//! Stored data is never rewritten.
//!
//! Loading a map on a store whose readers query the old IRIs hides new facts
//! from them unless those readers also see the equivalents. Configure a map
//! only once that holds.

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::{Error, Result};

const OWL_EQUIVALENT_PROPERTY: &str = "http://www.w3.org/2002/07/owl#equivalentProperty";
const OWL_EQUIVALENT_CLASS: &str = "http://www.w3.org/2002/07/owl#equivalentClass";

/// Term IRIs Quipu would mint, each with the IRI to write instead.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VocabularyMap {
    terms: BTreeMap<String, String>,
}

impl VocabularyMap {
    /// Read a map from Turtle. Only IRI-to-IRI `owl:equivalentProperty` and
    /// `owl:equivalentClass` statements are used; every other statement is
    /// ignored, so a published vocabulary document can serve as its own map.
    ///
    /// # Errors
    ///
    /// Refuses unparsable Turtle, and a subject mapped to two different
    /// objects (which IRI to write would be a guess).
    pub fn from_turtle(turtle: &str) -> Result<Self> {
        let mut terms: BTreeMap<String, String> = BTreeMap::new();
        for triple in oxttl::TurtleParser::new().for_reader(turtle.as_bytes()) {
            let triple = triple.map_err(|e| Error::InvalidValue(format!("vocabulary map: {e}")))?;
            let predicate = triple.predicate.as_str();
            if predicate != OWL_EQUIVALENT_PROPERTY && predicate != OWL_EQUIVALENT_CLASS {
                continue;
            }
            let (oxrdf::NamedOrBlankNode::NamedNode(subject), oxrdf::Term::NamedNode(object)) =
                (&triple.subject, &triple.object)
            else {
                continue;
            };
            let (from, to) = (subject.as_str(), object.as_str());
            if from == to {
                continue;
            }
            match terms.get(from) {
                Some(existing) if existing != to => {
                    return Err(Error::InvalidValue(format!(
                        "vocabulary map names two IRIs for {from}: {existing} and {to}"
                    )));
                }
                Some(_) => {}
                None => {
                    terms.insert(from.to_string(), to.to_string());
                }
            }
        }
        Ok(Self { terms })
    }

    /// Read a map from a Turtle file.
    ///
    /// # Errors
    ///
    /// Refuses an unreadable file, and everything [`Self::from_turtle`] refuses.
    pub fn load(path: &Path) -> Result<Self> {
        let turtle = std::fs::read_to_string(path)
            .map_err(|e| Error::InvalidValue(format!("vocabulary map {}: {e}", path.display())))?;
        Self::from_turtle(&turtle)
    }

    /// The IRI to write for `iri`: its mapped equivalent, or `iri` itself.
    #[must_use]
    pub fn resolve(&self, iri: &str) -> String {
        self.terms
            .get(iri)
            .cloned()
            .unwrap_or_else(|| iri.to_string())
    }

    /// The number of mapped terms.
    #[must_use]
    pub fn len(&self) -> usize {
        self.terms.len()
    }

    /// True when no term is mapped (every term stays under `base_ns`).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }
}

/// The IRI for term `local` under `base_ns`, after `map`.
#[must_use]
pub fn term(local: &str, base_ns: &str, map: &VocabularyMap) -> String {
    map.resolve(&format!("{base_ns}{local}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "http://example.org/kb/";
    const MAP: &str = "@prefix owl: <http://www.w3.org/2002/07/owl#> .\n\
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n\
        <http://example.org/kb/stepOf> owl:equivalentProperty <https://example.com/v#stepOf> .\n\
        <http://example.org/kb/GoldenPath> owl:equivalentClass <https://example.com/v#GoldenPath> .\n\
        <https://example.com/v#stepOf> rdfs:label \"step of\" .\n\
        <http://example.org/kb/self> owl:equivalentProperty <http://example.org/kb/self> .\n";

    #[test]
    fn no_map_writes_every_term_under_base_ns() {
        let map = VocabularyMap::default();
        assert!(map.is_empty());
        for local in ["stepOf", "sourceKind", "GoldenPath", "anything"] {
            assert_eq!(term(local, BASE, &map), format!("{BASE}{local}"));
        }
    }

    #[test]
    fn a_map_moves_exactly_its_subjects() {
        let map = VocabularyMap::from_turtle(MAP).unwrap();
        assert_eq!(map.len(), 2, "labels and self-maps are not terms");
        assert_eq!(term("stepOf", BASE, &map), "https://example.com/v#stepOf");
        assert_eq!(
            term("GoldenPath", BASE, &map),
            "https://example.com/v#GoldenPath"
        );
        assert_eq!(term("sourceKind", BASE, &map), format!("{BASE}sourceKind"));
        // Another store's base namespace is not this map's subject.
        assert_eq!(
            term("stepOf", "http://other.example/", &map),
            "http://other.example/stepOf"
        );
    }

    #[test]
    fn the_map_is_directional() {
        // The object is never rewritten back to the subject.
        let map = VocabularyMap::from_turtle(MAP).unwrap();
        assert_eq!(
            map.resolve("https://example.com/v#stepOf"),
            "https://example.com/v#stepOf"
        );
    }

    #[test]
    fn a_term_mapped_twice_is_refused() {
        let twice = format!(
            "{MAP}<http://example.org/kb/stepOf> \
             <http://www.w3.org/2002/07/owl#equivalentProperty> <https://elsewhere.example/stepOf> .\n"
        );
        let err = VocabularyMap::from_turtle(&twice).unwrap_err().to_string();
        assert!(err.contains("two IRIs"), "{err}");
    }

    #[test]
    fn unparsable_turtle_and_a_missing_file_are_refused() {
        assert!(VocabularyMap::from_turtle("<a> <b>").is_err());
        assert!(VocabularyMap::load(Path::new("/nonexistent/vocabulary-map.ttl")).is_err());
    }
}
