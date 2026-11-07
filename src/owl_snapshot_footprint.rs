//! Complete premise selection, without copying unrelated application facts.
//!
//! Property rules consume only axiom-named predicates; class rules consume
//! rdf:type. Identity additionally needs EVERY fact whose subject or reference
//! object participates in sameAs. Attributes alone would lose those rewrites.
//! If property rules can manufacture new identity members, retain the original
//! full-snapshot path: selection must never silently omit possible rewrites.

use std::collections::BTreeSet;

use rusqlite::{Row, params, params_from_iter, types::Value as SqlValue};

use crate::{Error, Result, Store, Value};

use super::PLANE_SOURCE;

const CHUNK: usize = 256;
const CURRENT: &str = "f.op=1 AND f.valid_to IS NULL AND NOT EXISTS \
    (SELECT 1 FROM transactions t WHERE t.id=f.tx AND t.source=?2)";

pub(super) enum Footprint {
    All,
    Scoped {
        attributes: BTreeSet<i64>,
        identities: BTreeSet<i64>,
    },
}

impl Footprint {
    pub(super) fn discover(
        source: &Store,
        ontologies: &[(String, String, String)],
        companion: Option<i64>,
        budget: usize,
    ) -> Result<Self> {
        let mut properties = BTreeSet::from([
            crate::namespace::OWL_SAME_AS.to_owned(),
            crate::namespace::RDF_TYPE.to_owned(),
        ]);
        for (_, turtle, _) in ontologies {
            // Exhaustive on purpose: a new axiom family must revisit the input
            // completeness argument. Class axioms consume the selected rdf:type
            // facts; disjointness is a validation constraint, not a derivation.
            let crate::owl::Axioms {
                subclass_of: _,
                subproperty_of,
                disjoint_with: _,
                inverse_of,
                functional_properties,
                symmetric_properties,
                transitive_properties,
                equivalent_classes: _,
                equivalent_properties,
                domains,
                ranges,
            } = crate::owl::Ontology::from_turtle(turtle)?.axioms;
            for pairs in [&subproperty_of, &equivalent_properties, &inverse_of] {
                for (left, right) in pairs {
                    if left == crate::namespace::OWL_SAME_AS
                        || right == crate::namespace::OWL_SAME_AS
                    {
                        return Ok(Self::All);
                    }
                    properties.insert(left.clone());
                    properties.insert(right.clone());
                }
            }
            for (property, _) in domains.iter().chain(&ranges) {
                properties.insert(property.clone());
            }
            properties.extend(symmetric_properties);
            properties.extend(transitive_properties);
            properties.extend(functional_properties);
        }
        let mut attributes = BTreeSet::new();
        for property in properties {
            if let Some(id) = source.lookup(&property)? {
                attributes.insert(id);
            }
        }
        let mut identities = BTreeSet::new();
        if let Some(same_as) = source.lookup(crate::namespace::OWL_SAME_AS)? {
            let mut count = 0;
            for graph in std::iter::once(0).chain(companion) {
                let mut stmt = source.conn.prepare(&format!(
                    "SELECT f.e,f.v FROM facts f WHERE f.g=?1 AND {CURRENT} AND f.a=?3"
                ))?;
                let mut rows = stmt.query(params![graph, PLANE_SOURCE, same_as])?;
                while let Some(row) = rows.next()? {
                    count += 1;
                    if count > budget {
                        return Err(Error::InvalidValue(
                            "OWL snapshot premise budget exceeded".into(),
                        ));
                    }
                    if let Value::Ref(other) = Value::from_bytes(&row.get::<_, Vec<u8>>(1)?)? {
                        identities.insert(row.get(0)?);
                        identities.insert(other);
                    }
                }
            }
        }
        Ok(Self::Scoped {
            attributes,
            identities,
        })
    }

    pub(super) fn for_each(
        &self,
        source: &Store,
        graph: i64,
        mut visit: impl FnMut(&Row<'_>) -> Result<()>,
    ) -> Result<()> {
        let Self::Scoped {
            attributes,
            identities,
        } = self
        else {
            let mut stmt = source.conn.prepare(&format!(
                "SELECT f.e,f.a,f.v,f.valid_from FROM facts f WHERE f.g=?1 AND {CURRENT}"
            ))?;
            let mut rows = stmt.query(params![graph, PLANE_SOURCE])?;
            while let Some(row) = rows.next()? {
                visit(row)?;
            }
            return Ok(());
        };
        // Three disjoint passes preserve physical-row accounting at the hard
        // premise cap. Rows selected by an earlier pass are never copied again.
        // Each IN list remains below SQLite's minimum supported variable limit.
        for pass in 0..3 {
            let (column, values): (_, Vec<SqlValue>) = match pass {
                0 => (
                    "a",
                    attributes.iter().copied().map(SqlValue::Integer).collect(),
                ),
                1 => (
                    "e",
                    identities.iter().copied().map(SqlValue::Integer).collect(),
                ),
                _ => (
                    "v",
                    identities
                        .iter()
                        .map(|id| SqlValue::Blob(Value::Ref(*id).to_bytes()))
                        .collect(),
                ),
            };
            for chunk in values.chunks(CHUNK) {
                let placeholders = (3..3 + chunk.len())
                    .map(|n| format!("?{n}"))
                    .collect::<Vec<_>>()
                    .join(",");
                let mut stmt = source.conn.prepare(&format!(
                    "SELECT f.e,f.a,f.v,f.valid_from FROM facts f \
                     WHERE f.g=?1 AND {CURRENT} AND f.{column} IN ({placeholders})"
                ))?;
                let mut parameters = vec![
                    SqlValue::Integer(graph),
                    SqlValue::Text(PLANE_SOURCE.into()),
                ];
                parameters.extend_from_slice(chunk);
                let mut rows = stmt.query(params_from_iter(parameters))?;
                while let Some(row) = rows.next()? {
                    if pass > 0 && attributes.contains(&row.get(1)?) {
                        continue;
                    }
                    if pass > 1 && identities.contains(&row.get(0)?) {
                        continue;
                    }
                    visit(row)?;
                }
            }
        }
        Ok(())
    }
}
