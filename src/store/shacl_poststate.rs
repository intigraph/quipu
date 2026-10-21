//! Cardinality admission against staged current facts, inside the write savepoint.
//!
//! Payload context repair remains monotone for the other SHACL constraints.
//! Upper bounds cannot use that repair: an individually valid delta can make a
//! stored focus node invalid. No whole-graph revalidation or history deletion.

#[cfg(feature = "shacl")]
use std::collections::BTreeSet;

use super::{Datum, Store};
use crate::{Error, Result};

impl Store {
    /// Supply explicit shapes to every transaction made by one operation.
    /// Nested scopes and unwinding restore the prior context. This does not
    /// enable the persistent `validate_on_write` setting or load any shapes.
    pub fn with_write_shapes<T>(
        &mut self,
        shapes: Option<&str>,
        operation: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        let previous = self.write_shapes.take();
        self.write_shapes = match (&previous, shapes) {
            (Some(old), Some(new)) => Some(format!("{old}\n{new}")),
            (Some(old), None) => Some(old.clone()),
            (None, new) => new.map(str::to_owned),
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(self)));
        self.write_shapes = previous;
        match result {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    pub(super) fn enforce_shacl_poststate(
        &mut self,
        datums: &[Datum],
        graph: i64,
        conditional: bool,
    ) -> Result<()> {
        // Verdict/refusal writers must not recursively validate their evidence.
        if self.recording_verdicts {
            return Ok(());
        }
        let loaded = if self.shacl_config.validate_on_write || conditional {
            self.get_combined_shapes()?
        } else {
            None
        };
        #[cfg(feature = "shacl")]
        let loaded = loaded.map(|shapes| crate::shacl::split_shapes_by_policy(&shapes).reject);
        let shapes = match (&self.write_shapes, loaded) {
            (Some(explicit), Some(loaded)) => format!("{loaded}\n{explicit}"),
            (Some(explicit), None) => explicit.clone(),
            (None, Some(loaded)) => loaded,
            (None, None) => return Ok(()),
        };
        #[cfg(feature = "shacl")]
        return self.check_poststate_cardinality(&shapes, datums, graph);
        #[cfg(not(feature = "shacl"))]
        {
            let _ = (shapes, datums, graph);
            Err(Error::CannotVerify(
                "write SHACL requires the shacl feature".into(),
            ))
        }
    }

    #[cfg(feature = "shacl")]
    fn check_poststate_cardinality(
        &self,
        shapes: &str,
        datums: &[Datum],
        graph: i64,
    ) -> Result<()> {
        use oxrdfio::{RdfFormat, RdfParser};
        const MAX: &str = "http://www.w3.org/ns/shacl#maxCount";
        const PATH: &str = "http://www.w3.org/ns/shacl#path";
        const COMPONENT: &str = "http://www.w3.org/ns/shacl#MaxCountConstraintComponent";
        // The caller selects reject shapes using its existing admission policy.
        // CLI --shapes and inline episode shapes remain strict, as before.
        let quads = RdfParser::from_format(RdfFormat::Turtle)
            .for_reader(shapes.as_bytes())
            .map(|q| q.map_err(|e| Error::InvalidValue(format!("write shapes: {e}"))))
            .collect::<Result<Vec<_>>>()?;
        let upper_bounds: BTreeSet<_> = quads
            .iter()
            .filter(|q| q.predicate.as_str() == MAX)
            .map(|q| q.subject.to_string())
            .collect();
        if upper_bounds.is_empty() || datums.is_empty() {
            return Ok(());
        }
        // Resolve inverse predicate paths without walking unrelated entities.
        // More complex paths still refuse rather than checking partial data.
        let mut inverse_predicates = BTreeSet::new();
        for q in &quads {
            if !upper_bounds.contains(&q.subject.to_string()) || q.predicate.as_str() != PATH {
                continue;
            }
            match &q.object {
                oxrdf::Term::NamedNode(_) => {}
                oxrdf::Term::BlankNode(path) => {
                    let inverse = quads.iter().find(|edge| {
                        edge.subject == oxrdf::NamedOrBlankNode::BlankNode(path.clone())
                            && edge.predicate.as_str() == "http://www.w3.org/ns/shacl#inversePath"
                    });
                    if let Some(oxrdf::Quad {
                        object: oxrdf::Term::NamedNode(predicate),
                        ..
                    }) = inverse
                    {
                        inverse_predicates.insert(predicate.as_str().to_owned());
                    } else {
                        return Err(Error::CannotVerify(
                            "post-state maxCount requires a predicate or inverse predicate path"
                                .into(),
                        ));
                    }
                }
                _ => return Err(Error::CannotVerify("invalid maxCount path".into())),
            }
        }
        let subjects: BTreeSet<_> = datums
            .iter()
            .flat_map(|d| {
                // Object targets can become applicable when this write adds an
                // incoming edge. Include referenced focuses without validating
                // unrelated subjects elsewhere in the graph.
                std::iter::once(d.entity).chain(match d.value {
                    crate::Value::Ref(object) => Some(object),
                    _ => None,
                })
            })
            .collect();
        let mut incoming_targets: BTreeSet<_> = quads
            .iter()
            .filter_map(|q| {
                if q.predicate.as_str() == "http://www.w3.org/ns/shacl#targetObjectsOf"
                    && let oxrdf::Term::NamedNode(predicate) = &q.object
                {
                    return Some(predicate.as_str().to_owned());
                }
                None
            })
            .collect();
        incoming_targets.extend(inverse_predicates);
        let mut facts = Vec::new();
        let mut focuses = BTreeSet::new();
        for entity in subjects {
            focuses.insert(self.resolve(entity)?);
            self.append_bounded_entity(&mut facts, entity, graph, None)?;
            self.append_object_targets(
                &mut facts,
                &crate::Value::Ref(entity),
                graph,
                &incoming_targets,
            )?;
            if graph != 0 {
                // ROOT types may target a named-graph focus, but ROOT values
                // never contribute to that graph's cardinality.
                if let Some(rdf_type) =
                    self.lookup("http://www.w3.org/1999/02/22-rdf-syntax-ns#type")?
                {
                    self.append_bounded_entity(&mut facts, entity, 0, Some(rdf_type))?;
                }
            }
        }
        if !incoming_targets.is_empty() {
            let mut seen = BTreeSet::new();
            for datum in datums {
                if !matches!(datum.value, crate::Value::Ref(_))
                    && seen.insert(datum.value.term_key())
                {
                    if let oxrdf::Term::Literal(literal) =
                        crate::rdf::value_to_term(self, &datum.value)?
                    {
                        focuses.insert(literal.value().to_owned());
                    }
                    self.append_object_targets(&mut facts, &datum.value, graph, &incoming_targets)?;
                }
            }
        }
        self.append_subclass_context(&mut facts, graph)?;
        let data = self.cardinality_context(&facts)?;
        let feedback = crate::validate_shapes(shapes, &data)?;
        let issues: Vec<_> = feedback
            .results
            .iter()
            .filter(|issue| {
                issue.component.trim_matches(['<', '>']) == COMPONENT
                    && focuses.contains(issue.focus_node.trim_matches(['<', '>']))
                    && issue.severity.eq_ignore_ascii_case("violation")
            })
            .collect();
        if issues.is_empty() {
            return Ok(());
        }
        Err(Error::ValidationFailed {
            violations: issues.len(),
            messages: issues
                .iter()
                .map(|issue| {
                    format!(
                        "post-state {} on {}: {}",
                        issue.component,
                        issue.focus_node,
                        issue
                            .message
                            .as_deref()
                            .unwrap_or("maximum cardinality exceeded"),
                    )
                })
                .collect(),
        })
    }
    #[cfg(feature = "shacl")]
    fn cardinality_context(&self, facts: &[crate::Fact]) -> Result<String> {
        use oxrdf::{BlankNode, NamedNode, NamedOrBlankNode, Triple};
        use oxrdfio::{RdfFormat, RdfSerializer};
        // Export canonicalization renames blank nodes. Preserve their stored
        // identities here so native feedback can be matched to touched focuses.
        let mut writer = RdfSerializer::from_format(RdfFormat::NTriples).for_writer(Vec::new());
        for fact in facts {
            let entity = self.resolve(fact.entity)?;
            let subject = if let Some(blank) = entity.strip_prefix("_:") {
                NamedOrBlankNode::BlankNode(
                    BlankNode::new(blank).map_err(|e| Error::InvalidValue(e.to_string()))?,
                )
            } else {
                NamedOrBlankNode::NamedNode(
                    NamedNode::new(entity).map_err(|e| Error::InvalidValue(e.to_string()))?,
                )
            };
            let predicate = NamedNode::new(self.resolve(fact.attribute)?)
                .map_err(|e| Error::InvalidValue(e.to_string()))?;
            let triple = Triple::new(
                subject,
                predicate,
                crate::rdf::value_to_term(self, &fact.value)?,
            );
            writer
                .serialize_triple(&triple)
                .map_err(|e| Error::Serialization(e.to_string()))?;
        }
        String::from_utf8(
            writer
                .finish()
                .map_err(|e| Error::Serialization(e.to_string()))?,
        )
        .map_err(|e| Error::Serialization(e.to_string()))
    }

    #[cfg(feature = "shacl")]
    fn append_object_targets(
        &self,
        facts: &mut Vec<crate::Fact>,
        object: &crate::Value,
        graph: i64,
        predicates: &BTreeSet<String>,
    ) -> Result<()> {
        // Existing incoming target edges need not have been in this payload.
        // Read only (predicate, bound object) through the existing value index.
        for predicate in predicates {
            let Some(attribute) = self.lookup(predicate)? else {
                continue;
            };
            let bytes = object.to_bytes();
            let mut stmt = self.conn.prepare(
                "SELECT e,a,v,tx,valid_from,valid_to,op FROM facts WHERE v=?1 AND a=?2 \
                 AND g=?3 AND op=1 AND valid_to IS NULL LIMIT 100001",
            )?;
            let mut rows = stmt.query(rusqlite::params![bytes, attribute, graph])?;
            while let Some(row) = rows.next()? {
                if facts.len() >= 100_000 {
                    return Err(Error::CannotVerify(
                        "post-state SHACL context exceeds 100000 facts".into(),
                    ));
                }
                facts.push(crate::Fact {
                    entity: row.get(0)?,
                    attribute: row.get(1)?,
                    value: crate::Value::from_bytes(&row.get::<_, Vec<u8>>(2)?)?,
                    tx: row.get(3)?,
                    valid_from: row.get(4)?,
                    valid_to: row.get(5)?,
                    op: crate::Op::Assert,
                });
            }
        }
        Ok(())
    }

    #[cfg(feature = "shacl")]
    fn append_bounded_entity(
        &self,
        facts: &mut Vec<crate::Fact>,
        entity: i64,
        graph: i64,
        attribute: Option<i64>,
    ) -> Result<()> {
        // Bound the indexed per-entity read BEFORE materializing facts.
        let count: u32 = self.conn.query_row(
            "SELECT COUNT(*) FROM (SELECT 1 FROM facts WHERE e=?1 AND g=?2 \
             AND op=1 AND valid_to IS NULL LIMIT 100001)",
            rusqlite::params![entity, graph],
            |row| row.get(0),
        )?;
        if count > 100_000 || facts.len() + count as usize > 100_000 {
            return Err(Error::CannotVerify(
                "post-state SHACL context exceeds 100000 facts".into(),
            ));
        }
        facts.extend(
            self.entity_facts_in_graph(entity, graph)?
                .into_iter()
                .filter(|fact| attribute.is_none_or(|attribute| fact.attribute == attribute)),
        );
        Ok(())
    }

    #[cfg(feature = "shacl")]
    fn append_subclass_context(&self, facts: &mut Vec<crate::Fact>, graph: i64) -> Result<()> {
        let Some(rdf_type) = self.lookup("http://www.w3.org/1999/02/22-rdf-syntax-ns#type")? else {
            return Ok(());
        };
        let Some(subclass) = self.lookup("http://www.w3.org/2000/01/rdf-schema#subClassOf")? else {
            return Ok(());
        };
        let mut pending: Vec<_> = facts
            .iter()
            .filter_map(|f| match f.value {
                crate::Value::Ref(class) if f.attribute == rdf_type => Some(class),
                _ => None,
            })
            .collect();
        let mut seen = BTreeSet::new();
        while let Some(class) = pending.pop() {
            if !seen.insert(class) {
                continue;
            }
            if seen.len() > 1024 {
                return Err(Error::CannotVerify(
                    "post-state SHACL type hierarchy exceeds 1024 classes".into(),
                ));
            }
            let start = facts.len();
            for g in BTreeSet::from([0, graph]) {
                self.append_bounded_entity(facts, class, g, Some(subclass))?;
            }
            pending.extend(facts[start..].iter().filter_map(|f| match f.value {
                crate::Value::Ref(parent) => Some(parent),
                _ => None,
            }));
        }
        Ok(())
    }
}
