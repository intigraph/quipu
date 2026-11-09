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
    pub(crate) subject_target_predicates: BTreeSet<String>,
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
            subject_target_predicates: BTreeSet::new(),
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
                match target {
                    Target::ObjectsOf(predicate) => {
                        plan.incoming_predicates
                            .insert(predicate.as_str().to_owned());
                        plan.object_target_predicates
                            .insert(predicate.as_str().to_owned());
                    }
                    Target::SubjectsOf(predicate) => {
                        plan.subject_target_predicates
                            .insert(predicate.as_str().to_owned());
                    }
                    _ => {}
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
                        if let Some(path) = shape.path() {
                            plan_value_set_path(path, &mut plan.incoming_predicates)?;
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

/// Record what a value-set constraint on `path` needs from the post-state
/// context. A predicate path needs only the focus's own facts; an inverse
/// predicate also needs the facts pointing at it; an alternative of those is
/// the union of its members. Any other path cannot be checked incrementally.
fn plan_value_set_path(path: &SHACLPath, incoming: &mut BTreeSet<String>) -> Result<()> {
    match path {
        SHACLPath::Predicate { .. } => Ok(()),
        SHACLPath::Inverse { path: inner } => match inner.as_ref() {
            SHACLPath::Predicate { pred } => {
                incoming.insert(pred.as_str().to_owned());
                Ok(())
            }
            _ => Err(unsupported_path(path)),
        },
        SHACLPath::Alternative { paths } => {
            for member in paths {
                match member {
                    SHACLPath::Predicate { .. } | SHACLPath::Inverse { .. } => {
                        plan_value_set_path(member, incoming)?;
                    }
                    _ => return Err(unsupported_path(path)),
                }
            }
            Ok(())
        }
        _ => Err(unsupported_path(path)),
    }
}

fn unsupported_path(path: &SHACLPath) -> Error {
    Error::CannotVerify(format!(
        "post-state value-set constraints (sh:maxCount, sh:uniqueLang, sh:disjoint, \
         sh:lessThan, sh:lessThanOrEquals) need a predicate, inverse predicate, or an \
         alternative of those as their path; {path} is not one"
    ))
}

/// Refuse a shapes set whose value-set constraints the write path cannot check.
///
/// The cardinality plan is computed over every loaded shape, so one stored
/// shape it cannot plan would make EVERY later write refuse, including writes
/// that never touch that shape. Refusing at load keeps bad data from bricking
/// the write path.
///
/// # Errors
///
/// When the set does not compile, or a value-set constraint has an unsupported path.
pub fn refuse_unverifiable_write_paths(shapes_turtle: &str) -> Result<()> {
    let validator = Validator::from_turtle(shapes_turtle)?;
    match validator.cardinality_plan() {
        Ok(_) => Ok(()),
        Err(Error::CannotVerify(reason)) => Err(Error::InvalidValue(format!(
            "shapes refused: {reason}. Loaded, this shape would make every write fail \
             post-state verification, so it is refused instead. Use a predicate path \
             (or split the constraint per predicate)."
        ))),
        Err(other) => Err(other),
    }
}
