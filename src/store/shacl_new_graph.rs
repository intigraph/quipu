//! Post-state admission for a graph that has no history before the write
//! (aegis-t16): a share's staging graph, a promotion or compose into a fresh
//! graph. Its post-state is exactly what the write staged, so it is judged
//! whole, in one batch validation, instead of per focus under the incremental
//! context cap, which refused any such graph larger than 100,000 facts. The
//! same components are judged and the same refusal is returned; only the way
//! context is gathered differs. A graph with any prior history keeps the
//! incremental path and its cap.

#[cfg(feature = "shacl")]
use std::collections::BTreeSet;

use super::Store;
#[cfg(feature = "shacl")]
use crate::Error;
#[cfg(feature = "shacl")]
use crate::Result;

// Value-set components (aegis-7ur8) are judged like maxCount: on the
// written focuses, over base context, independent of the value closure.
#[cfg(feature = "shacl")]
pub(super) const VALUE_SETS: &[&str] = &[
    "MaxCountConstraintComponent",
    "UniqueLangConstraintComponent",
    "DisjointConstraintComponent",
    "LessThanConstraintComponent",
    "LessThanOrEqualsConstraintComponent",
];
// Value components, judged once the value closure is complete.
#[cfg(feature = "shacl")]
pub(super) const COMPONENTS: &[&str] = &[
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

#[cfg(feature = "shacl")]
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
#[cfg(feature = "shacl")]
const SUBCLASS_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";

impl Store {
    #[cfg(feature = "shacl")]
    /// True when no fact row of `graph`, current or historical, was written by
    /// any transaction other than `tx_id`.
    pub(super) fn graph_is_new(&self, graph: i64, tx_id: i64) -> Result<bool> {
        let older: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM facts WHERE g = ?1 AND tx <> ?2)",
            rusqlite::params![graph, tx_id],
            |row| row.get(0),
        )?;
        Ok(!older)
    }

    #[cfg(feature = "shacl")]
    /// Asserted facts selected by `sql` (columns `e, a, v, tx, valid_from, valid_to`).
    fn post_state_facts(&self, sql: &str, params: &[i64]) -> Result<Vec<crate::Fact>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        })?;
        rows.map(|row| {
            let (entity, attribute, value, tx, valid_from, valid_to) = row?;
            Ok(crate::Fact {
                entity,
                attribute,
                value: crate::Value::from_bytes(&value)?,
                tx,
                valid_from,
                valid_to,
                op: crate::Op::Assert,
            })
        })
        .collect()
    }

    #[cfg(feature = "shacl")]
    pub(super) fn check_new_graph_in_batch(&self, shapes: &str, graph: i64) -> Result<()> {
        let mut facts = self.post_state_facts(
            "SELECT e,a,v,tx,valid_from,valid_to FROM facts \
             WHERE g = ?1 AND op = 1 AND valid_to IS NULL",
            &[graph],
        )?;
        let rdf_type = self.lookup(RDF_TYPE)?;
        let explicit_context =
            matches!(&self.write_reference_graphs, Some((destination, _)) if *destination == graph);
        // Capture write-owned focuses before adding any referenced types.
        let subjects: BTreeSet<i64> = facts.iter().map(|fact| fact.entity).collect();
        let mut owned: BTreeSet<String> = subjects
            .iter()
            .map(|entity| self.resolve(*entity))
            .collect::<Result<_>>()?;
        let object_targets = crate::shacl::cached_validator(shapes)?
            .cardinality_plan()?
            .object_target_predicates;
        for fact in &facts {
            if object_targets.contains(&self.resolve(fact.attribute)?) {
                match fact.value {
                    crate::Value::Ref(object) => {
                        owned.insert(self.resolve(object)?);
                    }
                    _ => {
                        if let oxrdf::Term::Literal(literal) =
                            crate::rdf::value_to_term(self, &fact.value)?
                        {
                            owned.insert(literal.value().to_owned());
                        }
                    }
                }
            }
        }
        let objects: BTreeSet<i64> = facts
            .iter()
            .filter_map(|fact| match fact.value {
                crate::Value::Ref(object) if explicit_context || !subjects.contains(&object) => {
                    Some(object)
                }
                _ => None,
            })
            .collect();
        if graph != 0
            && self.reference_type_graphs(graph).contains(&0)
            && let Some(rdf_type) = rdf_type
        {
            // ROOT types may target a named-graph focus, as on the incremental path.
            facts.extend(self.post_state_facts(
                "SELECT e,a,v,tx,valid_from,valid_to FROM facts \
                 WHERE g = 0 AND a = ?1 AND op = 1 AND valid_to IS NULL AND e IN \
                 (SELECT e FROM facts WHERE g = ?2 AND op = 1 AND valid_to IS NULL)",
                &[rdf_type, graph],
            )?);
        }
        if let Some(rdf_type) = rdf_type {
            // Only referenced types cross the explicit dataset boundary.
            for object in objects {
                for context_graph in self.reference_type_graphs(graph) {
                    if context_graph == graph {
                        continue; // The whole destination is already present.
                    }
                    facts.extend(
                        self.entity_facts_in_graph(object, context_graph)?
                            .into_iter()
                            .filter(|fact| fact.attribute == rdf_type),
                    );
                }
            }
        }
        if let (Some(rdf_type), Some(subclass)) = (rdf_type, self.lookup(SUBCLASS_OF)?) {
            let mut pending: Vec<i64> = facts
                .iter()
                .filter_map(|f| match f.value {
                    crate::Value::Ref(class) if f.attribute == rdf_type => Some(class),
                    _ => None,
                })
                .collect();
            let mut seen = BTreeSet::new();
            let mut hierarchy = Vec::new();
            while let Some(class) = pending.pop() {
                if !seen.insert(class) {
                    continue;
                }
                if seen.len() > 1024 {
                    return Err(Error::CannotVerify(
                        "post-state SHACL type hierarchy exceeds 1024 classes".into(),
                    ));
                }
                for g in BTreeSet::from([0, graph]) {
                    for fact in self.entity_facts_in_graph(class, g)? {
                        if fact.attribute == subclass {
                            if let crate::Value::Ref(parent) = fact.value {
                                pending.push(parent);
                            }
                            hierarchy.push(fact);
                        }
                    }
                }
            }
            facts.extend(hierarchy);
        }
        let data = self.cardinality_context(&facts)?;
        let feedback = crate::validate_shapes(shapes, &data)?;
        // Every focus in a new graph was written by this write, and its value
        // closure is complete, so no ownership filter applies.
        // The validator can report one result more than once; count each once.
        let mut reported = BTreeSet::new();
        let issues: Vec<_> = feedback
            .results
            .iter()
            .filter(|issue| {
                !explicit_context || owned.contains(issue.focus_node.trim_matches(['<', '>']))
            })
            .filter(|issue| {
                reported.insert((
                    issue.component.clone(),
                    issue.focus_node.clone(),
                    issue.path.clone(),
                    issue.message.clone(),
                ))
            })
            .filter(|issue| {
                issue
                    .component
                    .trim_matches(['<', '>'])
                    .strip_prefix("http://www.w3.org/ns/shacl#")
                    .is_some_and(|c| VALUE_SETS.contains(&c) || COMPONENTS.contains(&c))
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
}

#[cfg(all(test, feature = "shacl"))]
#[path = "shacl_new_graph_tests.rs"]
mod tests;
