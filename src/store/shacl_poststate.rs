//! Cardinality and value admission against staged facts inside the write savepoint.
//!
//! Payload repair alone cannot judge values written before their target type.
//! Check stored values of touched focuses as well, while keeping required-value
//! floors in the existing incremental repair lane. This is not a whole-graph audit.

#[cfg(feature = "shacl")]
use std::collections::BTreeSet;

#[cfg(feature = "shacl")]
mod instrumented_check;
use crate::time::Stopwatch;

use super::{Datum, Store};
use crate::{Error, Result};

pub(super) const CONTEXT_NODE_BUDGET: usize = 1024;
pub(super) const CONTEXT_FACT_BUDGET: u32 = 100_000;

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
        tx_id: i64,
        conditional: bool,
    ) -> Result<()> {
        let observation = self.begin_validator_observation(graph, tx_id, conditional);
        let result = self.enforce_shacl_poststate_observed(datums, graph, tx_id, conditional);
        self.finish_validator_observation(observation, &result);
        result
    }

    fn enforce_shacl_poststate_observed(
        &mut self,
        datums: &[Datum],
        graph: i64,
        tx_id: i64,
        conditional: bool,
    ) -> Result<()> {
        // Verdict/refusal writers must not recursively validate their evidence.
        if self.recording_verdicts {
            return Ok(());
        }
        let started = Stopwatch::start();
        let loaded = if self.shacl_config.validate_on_write || conditional {
            self.observed_loaded_selection();
            self.get_combined_shapes()?
        } else {
            None
        };
        let load = started.elapsed();
        #[cfg(feature = "shacl")]
        let loaded = loaded.map(|shapes| crate::shacl::split_shapes_by_policy(&shapes).reject);
        let split = started.elapsed().saturating_sub(load);
        let shapes = match (&self.write_shapes, loaded) {
            (Some(explicit), Some(loaded)) => format!("{loaded}\n{explicit}"),
            (Some(explicit), None) => explicit.clone(),
            (None, Some(loaded)) => loaded,
            (None, None) => return Ok(()),
        };
        self.poststate_profile.runs += 1;
        self.poststate_profile.load += load;
        self.poststate_profile.split += split;
        #[cfg(feature = "shacl")]
        return self.check_poststate_cardinality(&shapes, datums, graph, tx_id);
        #[cfg(not(feature = "shacl"))]
        {
            let _ = (shapes, datums, graph, tx_id);
            Err(Error::CannotVerify(
                "write SHACL requires the shacl feature".into(),
            ))
        }
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
            if seen.len() > CONTEXT_NODE_BUDGET {
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

    /// Complete the affected predicate-connected component before judging it.
    /// Only bound entity reads and (predicate, object) index probes are used;
    /// no graph scan, cross-graph values or partial-context success is allowed.
    #[cfg(feature = "shacl")]
    fn append_affected_path_context(
        &self,
        facts: &mut Vec<crate::Fact>,
        focuses: &mut BTreeSet<String>,
        seeds: &BTreeSet<i64>,
        graph: i64,
        predicates: &BTreeSet<String>,
    ) -> Result<()> {
        let attributes: BTreeSet<_> = predicates
            .iter()
            .map(|p| self.lookup(p))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();
        let reference_graphs: Vec<_> = self
            .reference_type_graphs(graph)
            .into_iter()
            .filter(|context_graph| *context_graph != graph)
            .collect();
        let rdf_type = if reference_graphs.is_empty() {
            None
        } else {
            self.lookup("http://www.w3.org/1999/02/22-rdf-syntax-ns#type")?
        };
        let mut pending: Vec<_> = seeds.iter().copied().collect();
        let mut seen = BTreeSet::new();
        while let Some(entity) = pending.pop() {
            if !seen.insert(entity) {
                continue;
            }
            if seen.len() > CONTEXT_NODE_BUDGET {
                return Err(Error::CannotVerify(
                    "post-state SHACL affected paths exceed 1024 nodes".into(),
                ));
            }
            focuses.insert(self.resolve(entity)?);
            let start = facts.len();
            self.append_bounded_entity(facts, entity, graph, None)?;
            // A discovered parent can be typed outside the destination.
            // Honor exactly the selected reference-type dataset; never fetch
            // those graphs' values or infer that ROOT is always selected.
            if let Some(rdf_type) = rdf_type {
                for &context_graph in &reference_graphs {
                    self.append_bounded_entity(facts, entity, context_graph, Some(rdf_type))?;
                }
            }
            self.append_object_targets(facts, &crate::Value::Ref(entity), graph, predicates)?;
            for fact in &facts[start..] {
                if attributes.contains(&fact.attribute) {
                    pending.push(fact.entity);
                    if let crate::Value::Ref(object) = fact.value {
                        pending.push(object);
                    }
                }
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
    fn append_value_context(
        &self,
        facts: &mut Vec<crate::Fact>,
        graph: i64,
        node_predicates: &BTreeSet<String>,
        needs_classes: bool,
    ) -> Result<()> {
        // Scalar constraints need no reference walk. sh:node follows only its
        // declared predicate paths; sh:class reads types, never a value closure.
        // A context node is not an additional write-owned focus.
        if node_predicates.is_empty() && !needs_classes {
            return Ok(());
        }
        let node_attributes: BTreeSet<_> = node_predicates
            .iter()
            .map(|p| self.lookup(p))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();
        let rdf_type = self.lookup("http://www.w3.org/1999/02/22-rdf-syntax-ns#type")?;
        let mut seen: BTreeSet<_> = facts.iter().map(|fact| fact.entity).collect();
        let mut start = 0;
        while start < facts.len() {
            let end = facts.len();
            let refs: BTreeSet<_> = facts[start..end]
                .iter()
                .filter_map(|fact| match fact.value {
                    crate::Value::Ref(entity) if node_attributes.contains(&fact.attribute) => {
                        Some(entity)
                    }
                    _ => None,
                })
                .collect();
            if needs_classes && let Some(rdf_type) = rdf_type {
                let classes: BTreeSet<_> = facts[start..end]
                    .iter()
                    .filter_map(|fact| match fact.value {
                        crate::Value::Ref(entity) if fact.attribute != rdf_type => Some(entity),
                        _ => None,
                    })
                    .collect();
                for entity in classes {
                    for g in self.reference_type_graphs(graph) {
                        self.append_bounded_entity(facts, entity, g, Some(rdf_type))?;
                    }
                }
            }
            start = end;
            for entity in refs {
                if !seen.insert(entity) {
                    continue;
                }
                if seen.len() > CONTEXT_NODE_BUDGET {
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

    pub(super) fn cardinality_context(&self, facts: &[crate::Fact]) -> Result<String> {
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
                if facts.len() >= CONTEXT_FACT_BUDGET as usize {
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
    pub(super) fn append_bounded_entity(
        &self,
        facts: &mut Vec<crate::Fact>,
        entity: i64,
        graph: i64,
        attribute: Option<i64>,
    ) -> Result<()> {
        // Bound the indexed per-entity read BEFORE materializing facts.
        let count: u32 = self.conn.query_row(
            "SELECT COUNT(*) FROM (SELECT 1 FROM facts WHERE e=?1 AND g=?2 \
             AND op=1 AND valid_to IS NULL LIMIT ?3)",
            rusqlite::params![entity, graph, CONTEXT_FACT_BUDGET + 1],
            |row| row.get(0),
        )?;
        if count > CONTEXT_FACT_BUDGET
            || facts.len() + count as usize > CONTEXT_FACT_BUDGET as usize
        {
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
}

#[cfg(feature = "shacl")]
pub(super) fn context_budget_exhausted(message: &str) -> bool {
    message.starts_with("post-state SHACL ")
        && (message.contains("exceeds 100000 facts")
            || message.contains("exceed 1024 nodes")
            || message.contains("exceeds 1024 nodes")
            || message.contains("exceeds 1024 classes"))
}

#[cfg(all(test, feature = "shacl"))]
#[path = "shacl_write_path_tests.rs"]
mod write_path_tests;
