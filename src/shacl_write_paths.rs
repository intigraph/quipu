//! Capability obligations follow the same compiled shape identities and
//! deactivation semantics as batch validation, including shared references.

use std::collections::{BTreeSet, HashSet};

use rudof_rdf::rdf_core::SHACLPath;
use shacl_engine::ir::IRComponent;
use shacl_engine::types::Target;

use super::Validator;
use crate::{Error, Result};

pub(crate) struct CardinalityPlan {
    pub(crate) has_value_set_constraint: bool,
    pub(crate) has_value_constraint: bool,
    pub(crate) has_class_constraint: bool,
    pub(crate) incoming_predicates: BTreeSet<String>,
    pub(crate) object_target_predicates: BTreeSet<String>,
    pub(crate) node_reference_predicates: BTreeSet<String>,
}

impl Validator {
    pub(crate) fn cardinality_plan(&self) -> Result<CardinalityPlan> {
        let schema = self
            .schema
            .lock()
            .map_err(|e| Error::CannotVerify(format!("shapes lock poisoned: {e}")))?;
        let mut pending: Vec<_> = schema
            .iter_with_targets()
            .filter_map(|(label, _)| schema.get_idx(label).copied())
            .collect();
        let mut seen = HashSet::new();
        let mut plan = CardinalityPlan {
            has_value_set_constraint: false,
            has_value_constraint: false,
            has_class_constraint: false,
            incoming_predicates: BTreeSet::new(),
            object_target_predicates: BTreeSet::new(),
            node_reference_predicates: BTreeSet::new(),
        };
        while let Some(index) = pending.pop() {
            if !seen.insert(index) {
                continue;
            }
            let shape = schema.get_shape_from_idx(&index).ok_or_else(|| {
                Error::CannotVerify("compiled cardinality shape reference missing".into())
            })?;
            if shape.deactivated() {
                continue;
            }
            for target in shape.targets() {
                if let Target::ObjectsOf(predicate) = target {
                    plan.incoming_predicates
                        .insert(predicate.as_str().to_owned());
                    plan.object_target_predicates
                        .insert(predicate.as_str().to_owned());
                }
            }
            pending.extend(shape.property_shapes().iter().copied());
            for component in shape.components() {
                if matches!(
                    component,
                    IRComponent::Datatype(_)
                        | IRComponent::NodeKind(_)
                        | IRComponent::Class(_)
                        | IRComponent::Pattern(_)
                        | IRComponent::MinLength(_)
                        | IRComponent::MaxLength(_)
                        | IRComponent::MinExclusive(_)
                        | IRComponent::MinInclusive(_)
                        | IRComponent::MaxExclusive(_)
                        | IRComponent::MaxInclusive(_)
                        | IRComponent::In(_)
                        | IRComponent::LanguageIn(_)
                        | IRComponent::Node(_)
                ) {
                    plan.has_value_constraint = true;
                }
                if matches!(component, IRComponent::Class(_)) {
                    plan.has_class_constraint = true;
                }
                // sh:uniqueLang false imposes nothing (aegis-7ur8).
                if matches!(component, IRComponent::UniqueLang(value) if !value.unique_lang()) {
                    continue;
                }
                match component {
                    IRComponent::MaxCount(_)
                    | IRComponent::UniqueLang(_)
                    | IRComponent::Disjoint(_)
                    | IRComponent::LessThan(_)
                    | IRComponent::LessThanOrEquals(_) => {
                        plan.has_value_set_constraint = true;
                        match shape.path() {
                            None | Some(SHACLPath::Predicate { .. }) => {}
                            Some(SHACLPath::Inverse { path }) => {
                                if let SHACLPath::Predicate { pred } = path.as_ref() {
                                    plan.incoming_predicates.insert(pred.as_str().to_owned());
                                } else {
                                    return Err(unsupported_path());
                                }
                            }
                            _ => return Err(unsupported_path()),
                        }
                    }
                    IRComponent::And(value) => pending.extend(value.shapes().iter().copied()),
                    IRComponent::Or(value) => pending.extend(value.shapes().iter().copied()),
                    IRComponent::Xone(value) => pending.extend(value.shapes().iter().copied()),
                    IRComponent::Node(value) => {
                        if let Some(SHACLPath::Predicate { pred }) = shape.path() {
                            plan.node_reference_predicates
                                .insert(pred.as_str().to_owned());
                        }
                        pending.push(*value.shape());
                    }
                    IRComponent::Not(value) => pending.push(*value.shape()),
                    IRComponent::QualifiedValueShape(value) => {
                        pending.push(*value.shape());
                        pending.extend(value.siblings().iter().copied());
                    }
                    _ => {}
                }
            }
        }
        Ok(plan)
    }
}

fn unsupported_path() -> Error {
    Error::CannotVerify(
        "post-state value-set constraints require a predicate or inverse predicate path".into(),
    )
}
