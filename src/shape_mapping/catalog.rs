//! Read-only proposal qualification. Declared reviews grant no write authority.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::{RegistryKind, ShapeMapping, ShapePin, invalid, iri};
use crate::{Result, Store};

impl Store {
    /// Current exact registry versions. A legacy row without a content digest
    /// cannot become a pin; missing schema/read errors never become empty data.
    pub fn current_shape_pins(&self, kind: RegistryKind, name: &str) -> Result<Vec<ShapePin>> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT valid_from,digest FROM {} WHERE name=?1 AND valid_to IS NULL ORDER BY valid_from",
            kind.table()
        ))?;
        let rows = statement
            .query_map(params![name], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(valid_from, digest)| {
                Ok(ShapePin {
                    registry: kind,
                    name: name.into(),
                    valid_from,
                    digest: digest
                        .ok_or_else(|| invalid("registry content has no verified digest"))?,
                })
            })
            .collect()
    }

    /// Check every declared pin, including duplicate-name ambiguity. A changed
    /// or closed version is stale even if some other version has identical RDF.
    pub fn qualify_mapping_pins(&self, mapping: &ShapeMapping) -> Result<()> {
        mapping.validate(false)?;
        let mut seen = BTreeSet::new();
        for pin in &mapping.pins {
            if !seen.insert((pin.registry, &pin.name)) {
                return Err(invalid(
                    "one mapping cannot pin two versions of one registry name",
                ));
            }
            let current = self.current_shape_pins(pin.registry, &pin.name)?;
            if current.as_slice() != std::slice::from_ref(pin) {
                return Err(invalid(
                    "mapping-stale: exact current shape version required",
                ));
            }
        }
        Ok(())
    }

    /// Proposal reads existing registered scope and current pins. It creates no
    /// graph, decision, record or transaction and performs no automatic matching.
    pub fn propose_shape_mapping(&self, mapping: ShapeMapping) -> Result<MappingProposal> {
        mapping.validate(false)?;
        if mapping.graph == crate::schema::ROOT_GRAPH_IRI {
            return Err(invalid("mapping needs an explicit named committed graph"));
        }
        for graph in std::iter::once(&mapping.graph)
            .chain(mapping.maps_from.iter())
            .chain(mapping.maps_to.iter())
        {
            let graph_id = if graph == crate::schema::ROOT_GRAPH_IRI {
                crate::schema::ROOT_GRAPH
            } else {
                self.committed_graph_id(graph)?
                    .ok_or_else(|| invalid("scope needs an existing registered committed graph"))?
            };
            if self.graph_class(graph_id)?.as_deref() != Some("committed") {
                return Err(invalid("mapping scope must be committed"));
            }
        }
        self.qualify_mapping_pins(&mapping)?;
        Ok(MappingProposal {
            expected_digest: mapping.digest()?,
            mapping,
        })
    }
}

/// The exact proposal snapshot whose digest is carried into a decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MappingProposal {
    pub mapping: ShapeMapping,
    pub expected_digest: String,
}

/// Decisions bind full correspondence keys, not input array positions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RowDecision {
    pub subject_id: String,
    pub predicate_id: String,
    pub object_id: String,
    pub accept: bool,
}

/// The explicit decision result; declined candidates are reported, not applied.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecidedMapping {
    pub mapping: ShapeMapping,
    pub expected_digest: String,
    pub declined_rows: Vec<RowDecision>,
}

/// A committed operation receipt and the mapping's current lifecycle status.
/// Exact replay returns the original record/tx without reopening a retired map.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingReceipt {
    pub mapping: String,
    pub digest: String,
    pub record: String,
    pub tx: i64,
    pub status: String,
    pub replayed: bool,
    pub assert_writable: BTreeMap<String, bool>,
}

impl MappingProposal {
    /// A pure transformation, never a signature or authorization decision.
    /// Every candidate needs an explicit unique decision; a stale input digest
    /// refuses before any row can be selected or review identity changed.
    pub fn decide(self, reviewer: &str, decisions: Vec<RowDecision>) -> Result<DecidedMapping> {
        if !iri(reviewer)
            || decisions.len() != self.mapping.rows.len()
            || self.mapping.digest()? != self.expected_digest
        {
            return Err(invalid("stale proposal digest or invalid reviewer"));
        }
        let mut selected = BTreeMap::new();
        for decision in decisions {
            let key = (
                decision.subject_id.clone(),
                decision.predicate_id.clone(),
                decision.object_id.clone(),
            );
            if selected.insert(key, decision).is_some() {
                return Err(invalid("duplicate correspondence decision"));
            }
        }
        let mut mapping = self.mapping;
        let mut accepted = BTreeSet::new();
        let mut declined_rows = Vec::new();
        for mut row in mapping.rows {
            let key = (
                row.subject_id.clone(),
                row.predicate_id.clone(),
                row.object_id.clone(),
            );
            let decision = selected
                .remove(&key)
                .ok_or_else(|| invalid("every candidate needs an explicit decision"))?;
            if decision.accept {
                row.author_id = reviewer.into();
                row.quipu_review = "accepted".into();
                row.mapping_justification = "semapv:ManualMappingCuration".into();
                accepted.insert(row);
            } else {
                declined_rows.push(decision);
            }
        }
        if !selected.is_empty() || accepted.is_empty() {
            return Err(invalid("foreign decisions or no accepted correspondence"));
        }
        mapping.rows = accepted;
        mapping.validate(true)?;
        Ok(DecidedMapping {
            expected_digest: mapping.digest()?,
            mapping,
            declined_rows,
        })
    }
}
