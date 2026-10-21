//! Cardinality and value admission against staged facts inside the write savepoint.
//!
//! Payload repair alone cannot judge values written before their target type.
//! Check stored values of touched focuses as well, while keeping required-value
//! floors in the existing incremental repair lane. This is not a whole-graph audit.

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
        const COMPONENTS: &[&str] = &[
            "MaxCountConstraintComponent",
            "DatatypeConstraintComponent",
            "NodeKindConstraintComponent",
            "ClassConstraintComponent",
            "PatternConstraintComponent",
            "MinLengthConstraintComponent",
            "MaxLengthConstraintComponent",
            "MinExclusiveConstraintComponent",
            "MinInclusiveConstraintComponent",
            "MaxExclusiveConstraintComponent",
            "MaxInclusiveConstraintComponent",
            "InConstraintComponent",
            "LanguageInConstraintComponent",
            "NodeConstraintComponent",
        ];
        // The caller selects reject shapes using its existing admission policy.
        // CLI --shapes and inline episode shapes remain strict, as before.
        // Reuse batch's compiled schema and relative-IRI base. Inactive shapes
        // must not impose unsupported-path or context-gathering obligations.
        let plan = crate::shacl::cached_validator(shapes)?.cardinality_plan()?;
        if !(plan.has_upper_bound || plan.has_value_constraint) || datums.is_empty() {
            return Ok(());
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
        let mut value_focuses = self.value_focuses(datums, &plan.object_target_predicates)?;
        let incoming_targets = plan.incoming_predicates;
        let mut facts = Vec::new();
        let mut focuses = BTreeSet::new();
        self.append_node_dependents(
            &mut facts,
            &mut value_focuses,
            &subjects,
            graph,
            &plan.node_reference_predicates,
        )?;
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
        if plan.has_value_constraint {
            self.append_value_context(&mut facts, graph)?;
        }
        self.append_subclass_context(&mut facts, graph)?;
        let data = self.cardinality_context(&facts)?;
        let feedback = crate::validate_shapes(shapes, &data)?;
        let issues: Vec<_> = feedback
            .results
            .iter()
            .filter(|issue| {
                let Some(component) = issue
                    .component
                    .trim_matches(['<', '>'])
                    .strip_prefix("http://www.w3.org/ns/shacl#")
                else {
                    return false;
                };
                let owned = if component == "MaxCountConstraintComponent" {
                    &focuses
                } else {
                    &value_focuses
                };
                COMPONENTS.contains(&component)
                    && owned.contains(issue.focus_node.trim_matches(['<', '>']))
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
                            .unwrap_or("post-state value constraint violated"),
                    )
                })
                .collect(),
        })
    }
    #[cfg(feature = "shacl")]
    fn append_node_dependents(
        &self,
        facts: &mut Vec<crate::Fact>,
        focuses: &mut BTreeSet<String>,
        subjects: &BTreeSet<i64>,
        graph: i64,
        predicates: &BTreeSet<String>,
    ) -> Result<()> {
        if predicates.is_empty() {
            return Ok(());
        }
        let mut pending: Vec<_> = subjects.iter().copied().collect();
        let mut seen = BTreeSet::new();
        let mut parents = BTreeSet::new();
        while let Some(child) = pending.pop() {
            if !seen.insert(child) {
                continue;
            }
            if seen.len() > 1024 {
                return Err(Error::CannotVerify(
                    "post-state SHACL node dependencies exceed 1024 nodes".into(),
                ));
            }
            let start = facts.len();
            self.append_object_targets(facts, &crate::Value::Ref(child), graph, predicates)?;
            let incoming: BTreeSet<_> = facts[start..].iter().map(|fact| fact.entity).collect();
            for parent in incoming {
                if !parents.insert(parent) {
                    continue;
                }
                focuses.insert(self.resolve(parent)?);
                self.append_bounded_entity(facts, parent, graph, None)?;
                if graph != 0
                    && let Some(rdf_type) =
                        self.lookup("http://www.w3.org/1999/02/22-rdf-syntax-ns#type")?
                {
                    self.append_bounded_entity(facts, parent, 0, Some(rdf_type))?;
                }
                pending.push(parent);
            }
        }
        Ok(())
    }

    #[cfg(feature = "shacl")]
    fn value_focuses(
        &self,
        datums: &[Datum],
        object_targets: &BTreeSet<String>,
    ) -> Result<BTreeSet<String>> {
        let mut focuses = BTreeSet::new();
        for datum in datums {
            focuses.insert(self.resolve(datum.entity)?);
            if object_targets.contains(&self.resolve(datum.attribute)?) {
                match &datum.value {
                    crate::Value::Ref(entity) => {
                        focuses.insert(self.resolve(*entity)?);
                    }
                    _ => {
                        if let oxrdf::Term::Literal(literal) =
                            crate::rdf::value_to_term(self, &datum.value)?
                        {
                            focuses.insert(literal.value().to_owned());
                        }
                    }
                }
            }
        }
        Ok(focuses)
    }

    #[cfg(feature = "shacl")]
    fn append_value_context(&self, facts: &mut Vec<crate::Fact>, graph: i64) -> Result<()> {
        // A late sh:node target can inspect values on nodes already referenced
        // by the focus. Bound this closure, preserve graph isolation, and never
        // turn those context nodes into additional write-owned focuses.
        let mut seen: BTreeSet<_> = facts.iter().map(|fact| fact.entity).collect();
        let mut start = 0;
        while start < facts.len() {
            let end = facts.len();
            let refs: BTreeSet<_> = facts[start..end]
                .iter()
                .filter_map(|fact| match fact.value {
                    crate::Value::Ref(entity) => Some(entity),
                    _ => None,
                })
                .collect();
            start = end;
            for entity in refs {
                if !seen.insert(entity) {
                    continue;
                }
                if seen.len() > 1024 {
                    return Err(Error::CannotVerify(
                        "post-state SHACL value context exceeds 1024 nodes".into(),
                    ));
                }
                self.append_bounded_entity(facts, entity, graph, None)?;
                if graph != 0
                    && let Some(rdf_type) =
                        self.lookup("http://www.w3.org/1999/02/22-rdf-syntax-ns#type")?
                {
                    self.append_bounded_entity(facts, entity, 0, Some(rdf_type))?;
                }
            }
        }
        Ok(())
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
