//! Version-pinned, one-hop schema correspondences. A mapping grants no access.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

pub mod catalog;
pub mod facts;
pub(crate) mod write_scope;

/// Hash RDF content, not whitespace, prefix spelling or blank-node labels.
pub fn canonical_shape_digest(turtle: &str) -> Result<String> {
    let mut triples = Vec::new();
    for row in
        oxrdfio::RdfParser::from_format(oxrdfio::RdfFormat::Turtle).for_reader(turtle.as_bytes())
    {
        let row = row.map_err(|e| Error::InvalidValue(format!("shape digest: {e}")))?;
        triples.extend_from_slice(format!("{} .\n", oxrdf::Triple::from(row)).as_bytes());
    }
    Ok(crate::share::sha256(&crate::share::canonicalize_ntriples(
        &triples,
    )?))
}

/// Add content identity without rewriting old text, timestamps or transaction IDs.
/// Legacy unparsable rows remain unpinnable (NULL), never a fabricated digest.
pub(crate) fn migrate_registry_digests(conn: &rusqlite::Connection) -> Result<()> {
    for table in ["shapes", "ontologies"] {
        let present = conn
            .prepare(&format!(
                "SELECT 1 FROM pragma_table_info('{table}') WHERE name='digest'"
            ))?
            .exists([])?;
        if !present {
            conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN digest TEXT"))?;
        }
        let rows = conn
            .prepare(&format!(
                "SELECT name,valid_from,turtle FROM {table} WHERE digest IS NULL"
            ))?
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (name, at, turtle) in rows {
            if let Ok(digest) = canonical_shape_digest(&turtle) {
                conn.execute(
                    &format!("UPDATE {table} SET digest=?3 WHERE name=?1 AND valid_from=?2 AND digest IS NULL"),
                    rusqlite::params![name, at, digest],
                )?;
            }
        }
    }
    Ok(())
}

/// Registry namespace is part of a pin: identical names in two registries differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistryKind {
    Shapes,
    Ontologies,
}

impl RegistryKind {
    pub(crate) const fn table(self) -> &'static str {
        match self {
            Self::Shapes => "shapes",
            Self::Ontologies => "ontologies",
        }
    }
}

/// Exact version, never an instruction to follow the newest version.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShapePin {
    pub registry: RegistryKind,
    pub name: String,
    pub valid_from: String,
    pub digest: String,
}

/// A reviewed SSSOM correspondence, restricted to the C1 five predicates.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Correspondence {
    pub subject_id: String,
    pub predicate_id: String,
    pub object_id: String,
    pub mapping_justification: String,
    pub author_id: String,
    pub quipu_review: String,
}

/// A complete mapping proposal. Unknown fields never silently alter a decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShapeMapping {
    pub id: String,
    pub graph: String,
    pub maps_from: BTreeSet<String>,
    pub maps_to: BTreeSet<String>,
    pub pins: BTreeSet<ShapePin>,
    pub rows: BTreeSet<Correspondence>,
}

const EQUIVALENT_CLASS: &str = "http://www.w3.org/2002/07/owl#equivalentClass";
const EQUIVALENT_PROPERTY: &str = "http://www.w3.org/2002/07/owl#equivalentProperty";
const INVERSE: &str = "http://www.w3.org/2002/07/owl#inverseOf";
const SUBCLASS: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
const SUBPROPERTY: &str = "http://www.w3.org/2000/01/rdf-schema#subPropertyOf";

fn invalid(message: &str) -> Error {
    Error::InvalidValue(format!("shape mapping: {message}"))
}

fn iri(value: &str) -> bool {
    value.len() <= 2048 && oxrdf::NamedNode::new(value).is_ok()
}

fn valid_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Correspondence {
    fn invertible(&self) -> bool {
        matches!(
            self.predicate_id.as_str(),
            EQUIVALENT_CLASS | EQUIVALENT_PROPERTY | INVERSE
        )
    }
}

impl ShapeMapping {
    /// Validate bounded syntax and semantic restrictions before reading a store.
    pub fn validate(&self, decided: bool) -> Result<()> {
        if !iri(&self.id)
            || !iri(&self.graph)
            || self.maps_from.is_empty()
            || self.maps_to.is_empty()
            || self.maps_from.len() > 64
            || self.maps_to.len() > 64
            || self.maps_from.iter().chain(&self.maps_to).any(|v| !iri(v))
            || !self.maps_from.is_disjoint(&self.maps_to)
            || self.pins.is_empty()
            || self.pins.len() > 128
            || self.rows.is_empty()
            || self.rows.len() > 1024
        {
            return Err(invalid("invalid identity, scope, or bounds"));
        }
        for pin in &self.pins {
            if pin.name.is_empty()
                || pin.name.len() > 256
                || crate::time::epoch_of_rfc3339(&pin.valid_from).is_none()
                || !valid_digest(&pin.digest)
            {
                return Err(invalid("invalid shape pin"));
            }
        }
        let mut destinations = BTreeMap::new();
        let mut sources = BTreeSet::new();
        let mut targets = BTreeSet::new();
        let mut row_keys = BTreeSet::new();
        for row in &self.rows {
            if !row_keys.insert((&row.subject_id, &row.predicate_id, &row.object_id)) {
                return Err(invalid(
                    "duplicate correspondence key with different metadata",
                ));
            }
            if !iri(&row.subject_id)
                || !iri(&row.object_id)
                || row.author_id.len() > 2048
                || row.quipu_review.len() > 32
                || row.subject_id == row.object_id
                || !matches!(
                    row.predicate_id.as_str(),
                    EQUIVALENT_CLASS | EQUIVALENT_PROPERTY | INVERSE | SUBCLASS | SUBPROPERTY
                )
            {
                return Err(invalid(
                    "invalid term or disallowed correspondence predicate",
                ));
            }
            crate::align::sssom::Justification::parse(&row.mapping_justification)?;
            if decided && (!iri(&row.author_id) || row.quipu_review != "accepted") {
                return Err(invalid(
                    "every applied row needs an explicit accepted review",
                ));
            }
            let destination = (&row.object_id, &row.predicate_id);
            if let Some(old) = destinations.insert(&row.subject_id, destination)
                && old != destination
            {
                return Err(invalid("conflicting rows for a source term"));
            }
            sources.insert(&row.subject_id);
            targets.insert(&row.object_id);
        }
        if !sources.is_disjoint(&targets) {
            return Err(invalid("mapping chains are unsupported"));
        }
        Ok(())
    }

    /// Canonical JSON uses sorted scope, pins and rows; binds the whole proposal.
    pub fn digest(&self) -> Result<String> {
        self.validate(false)?;
        Ok(crate::share::sha256(
            &serde_json::to_vec(self).map_err(|e| Error::Serialization(e.to_string()))?,
        ))
    }

    /// Per target, assert only through one exact invertible source/kind pair.
    pub fn assert_writable(&self) -> Result<BTreeMap<String, bool>> {
        self.validate(true)?;
        let mut inverse: BTreeMap<String, BTreeSet<(&str, &str)>> = BTreeMap::new();
        for row in &self.rows {
            inverse
                .entry(row.object_id.clone())
                .or_default()
                .insert((&row.subject_id, &row.predicate_id));
        }
        Ok(inverse
            .into_iter()
            .map(|(target, rows)| {
                let writable = rows.len() == 1
                    && self
                        .rows
                        .iter()
                        .any(|row| row.object_id == target && row.invertible());
                (target, writable)
            })
            .collect())
    }

    /// Refuse one-hop conflicts against other decided mappings in overlapping scope.
    pub fn check_other(&self, other: &Self) -> Result<()> {
        self.validate(true)?;
        other.validate(true)?;
        if self.id == other.id
            || (self.maps_from.is_disjoint(&other.maps_from)
                && self.maps_to.is_disjoint(&other.maps_to)
                && self.maps_from.is_disjoint(&other.maps_to)
                && self.maps_to.is_disjoint(&other.maps_from))
        {
            return Ok(());
        }
        for row in &self.rows {
            for old in &other.rows {
                if row.subject_id == old.object_id || row.object_id == old.subject_id {
                    return Err(invalid(&format!("chain with mapping {}", other.id)));
                }
                if row.subject_id == old.subject_id
                    && (row.object_id != old.object_id || row.predicate_id != old.predicate_id)
                {
                    return Err(invalid(&format!("conflict with mapping {}", other.id)));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
