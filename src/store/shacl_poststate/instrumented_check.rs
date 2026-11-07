//! Incremental post-state CHECK with nested timing observations.

use std::collections::BTreeSet;

use super::{Datum, Store, context_budget_exhausted};
use crate::store::shacl_new_graph::{COMPONENTS, VALUE_SETS};
use crate::time::Stopwatch;
use crate::{Error, Result};

impl Store {
    pub(super) fn check_poststate_cardinality(
        &mut self,
        shapes: &str,
        datums: &[Datum],
        graph: i64,
        tx_id: i64,
    ) -> Result<()> {
        // The caller selects reject shapes using its existing admission policy.
        // CLI --shapes and inline episode shapes remain strict, as before.
        // Reuse batch's compiled schema and relative-IRI base. Inactive shapes
        // must not impose unsupported-path or context-gathering obligations.
        let planning = Stopwatch::start();
        let plan = crate::shacl::cached_validator(shapes)?.cardinality_plan()?;
        self.poststate_profile.plan += planning.elapsed();
        if !(plan.has_value_set_constraint || plan.has_value_constraint) || datums.is_empty() {
            return Ok(());
        }
        let checking = Stopwatch::start();
        // Time the result before propagating it: refused/error paths are observations too.
        macro_rules! timed {
            ($field:ident, $work:expr) => {{
                let started = Stopwatch::start();
                let result = $work;
                self.poststate_profile.$field += started.elapsed();
                result
            }};
        }

        // A graph with no history before this write holds exactly what it
        // staged: judge it whole, in batch, rather than per focus under the
        // incremental context cap (aegis-t16).
        let is_new = timed!(check_graph, self.graph_is_new(graph, tx_id));
        let is_new = match is_new {
            Ok(is_new) => is_new,
            Err(error) => {
                self.poststate_profile.check += checking.elapsed();
                return Err(error);
            }
        };
        if is_new {
            let result = timed!(
                check_new_graph,
                self.check_new_graph_in_batch(shapes, graph)
            );
            self.poststate_profile.check += checking.elapsed();
            return result;
        }
        let result = (|| {
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
            self.poststate_profile.check_subjects += subjects.len() as u64;
            let (mut value_focuses, mut owned) = timed!(
                check_ownership,
                (|| {
                    Ok::<_, Error>((
                        self.value_focuses(datums, &plan.object_target_predicates)?,
                        self.value_set_ownership(datums, graph, tx_id, &plan)?,
                    ))
                })()
            )?;
            let incoming_targets = plan.incoming_predicates.clone();
            let mut facts = Vec::new();
            timed!(
                check_entities,
                (|| {
                    for &entity in &subjects {
                        self.append_bounded_entity(&mut facts, entity, graph, None)?;
                        self.append_object_targets(
                            &mut facts,
                            &crate::Value::Ref(entity),
                            graph,
                            &incoming_targets,
                        )?;
                        if graph != 0 && self.reference_type_graphs(graph).contains(&0) {
                            // ROOT types can target focuses; ROOT values never affect cardinality.
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
                                    owned.whole.insert(literal.value().to_owned());
                                }
                                self.append_object_targets(
                                    &mut facts,
                                    &datum.value,
                                    graph,
                                    &incoming_targets,
                                )?;
                            }
                        }
                    }
                    self.append_subclass_context(&mut facts, graph)?;
                    Ok::<_, Error>(())
                })()
            )?;
            // Preserve the pre-value-admission context. Exhausting this optional
            // repair budget falls back to the existing payload/maxCount path; a
            // partial context must never manufacture a new value violation.
            let base_len = facts.len();
            let value_context = timed!(
                check_values,
                (|| {
                    self.append_node_dependents(
                        &mut facts,
                        &mut value_focuses,
                        &subjects,
                        graph,
                        &plan.node_reference_predicates,
                    )?;
                    self.append_value_context(
                        &mut facts,
                        graph,
                        &plan.node_reference_predicates,
                        plan.has_class_constraint,
                    )?;
                    self.append_subclass_context(&mut facts, graph)
                })()
            );
            let complete_values = match value_context {
                Ok(()) => true,
                Err(Error::CannotVerify(message)) if context_budget_exhausted(&message) => {
                    facts.truncate(base_len);
                    self.poststate_profile.check_value_fallbacks += 1;
                    false
                }
                Err(error) => return Err(error),
            };
            self.poststate_profile.check_facts += facts.len() as u64;
            let data = timed!(check_encode, self.cardinality_context(&facts))?;
            let feedback = timed!(check_validate, crate::validate_shapes(shapes, &data))?;
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
                    let focus = issue.focus_node.trim_matches(['<', '>']);
                    let mine = if VALUE_SETS.contains(&component) {
                        owned.owns(component, focus, issue.path.as_deref())
                    } else {
                        complete_values
                            && COMPONENTS.contains(&component)
                            && value_focuses.contains(focus)
                    };
                    mine && issue.severity.eq_ignore_ascii_case("violation")
                })
                .collect();
            // Value debt the focus had before this transaction (aegis-g51c C12).
            let issues = timed!(
                check_debt,
                crate::shacl_debt::new_only(self, shapes, &data, issues, graph, tx_id)
            )?;
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
        })();
        self.poststate_profile.check += checking.elapsed();
        match result {
            Err(Error::CannotVerify(message))
                if !plan.has_value_set_constraint && context_budget_exhausted(&message) =>
            {
                Ok(())
            }
            result => result,
        }
    }
}
