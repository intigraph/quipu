//! Capability obligations follow the same compiled shape identities and
//! deactivation semantics as batch validation, including shared references.

use std::collections::{BTreeSet, HashSet};

use rudof_rdf::rdf_core::SHACLPath;
use shacl_engine::ir::IRComponent;
use shacl_engine::types::Target;

use super::Validator;
use crate::{Error, Result};

pub(crate) struct CardinalityPlan {
    pub(crate) has_upper_bound: bool,
    pub(crate) incoming_predicates: BTreeSet<String>,
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
            has_upper_bound: false,
            incoming_predicates: BTreeSet::new(),
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
                }
            }
            pending.extend(shape.property_shapes().iter().copied());
            for component in shape.components() {
                match component {
                    IRComponent::MaxCount(_) => {
                        plan.has_upper_bound = true;
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
                    IRComponent::Node(value) => pending.push(*value.shape()),
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
    Error::CannotVerify("post-state maxCount requires a predicate or inverse predicate path".into())
}
