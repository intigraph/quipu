//! Capability obligations follow the same compiled shape identities and
//! deactivation semantics as batch validation, including shared references.

use std::collections::{BTreeSet, HashSet};

use rudof_rdf::rdf_core::term::literal::Literal as _;
use rudof_rdf::rdf_core::{Rdf, SHACLPath};
use rudof_rdf::rdf_impl::OxigraphInMemory;
use shacl_engine::ir::IRComponent;
use shacl_engine::types::Target;

use super::Validator;
use crate::{Error, Result};

pub(crate) struct CardinalityPlan {
    pub(crate) has_value_set_constraint: bool,
    pub(crate) has_value_constraint: bool,
    pub(crate) has_class_constraint: bool,
    /// (predicate, class) for every sh:class on a predicate path: the values
    /// a participant member may need typed by an enlisted source (slice B).
    pub(crate) class_requirements: BTreeSet<(String, String)>,
    pub(crate) incoming_predicates: BTreeSet<String>,
    pub(crate) object_target_predicates: BTreeSet<String>,
    pub(crate) subject_target_predicates: BTreeSet<String>,
    pub(crate) node_reference_predicates: BTreeSet<String>,
    /// Every forward predicate a reachable shape reads from a focus: property
    /// paths, `sh:targetSubjectsOf`, and the other side of `sh:equals`,
    /// `sh:disjoint`, `sh:lessThan` and `sh:lessThanOrEquals` (aegis-7aee). Meaningful only
    /// when [`Self::subject_context_bounded`] holds.
    pub(crate) subject_predicates: BTreeSet<String>,
    /// Whether a focus's stored context may be narrowed to
    /// [`Self::subject_predicates`] plus `rdf:type`. False when some reachable
    /// shape's reach cannot be bounded that way: `sh:closed`, SPARQL
    /// constraints, `sh:subsetOf`, a complex path, an unknown target kind, or a
    /// shape reached again through `sh:node` (recursion or reuse). In that case
    /// the post-state check loads a focus's full current facts, as before.
    pub(crate) subject_context_bounded: bool,
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
            class_requirements: BTreeSet::new(),
            incoming_predicates: BTreeSet::new(),
            object_target_predicates: BTreeSet::new(),
            subject_target_predicates: BTreeSet::new(),
            node_reference_predicates: BTreeSet::new(),
            subject_predicates: BTreeSet::new(),
            subject_context_bounded: true,
        };
        while let Some(index) = pending.pop() {
            if !seen.insert(index) {
                continue;
            }
            let shape = schema.get_shape_from_idx(&index).ok_or_else(|| {
                Error::CannotVerify("compiled cardinality shape reference missing".into())
            })?;
            // Only a constant true expression can remove a static obligation.
            // Dynamic expressions need the data/focus and remain conservative here.
            if shape
                .deactivated()
                .and_then(shacl_engine::ir::IRNodeExpr::as_constant)
                .map(|value| OxigraphInMemory::object_as_term(&value))
                .and_then(|term| OxigraphInMemory::term_as_literal(&term).ok())
                .and_then(|literal| literal.to_bool())
                .unwrap_or(false)
            {
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
                        plan.subject_predicates
                            .insert(predicate.as_str().to_owned());
                    }
                    Target::Node(_) | Target::Class(_) | Target::ImplicitClass(_) => {}
                    _ => plan.subject_context_bounded = false,
                }
            }
            if let Some(path) = shape.path() {
                plan_subject_path(path, &mut plan);
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
                match component {
                    IRComponent::Closed(_)
                    | IRComponent::BasicSparql(_)
                    | IRComponent::SubsetOf(_) => {
                        plan.subject_context_bounded = false;
                    }
                    IRComponent::Equals(other) => {
                        plan.subject_predicates
                            .insert(other.iri().as_str().to_owned());
                    }
                    IRComponent::Disjoint(other) => {
                        plan.subject_predicates
                            .insert(other.iri().as_str().to_owned());
                    }
                    IRComponent::LessThan(other) => {
                        plan.subject_predicates
                            .insert(other.iri().as_str().to_owned());
                    }
                    IRComponent::LessThanOrEquals(other) => {
                        plan.subject_predicates
                            .insert(other.iri().as_str().to_owned());
                    }
                    IRComponent::Node(value) if seen.contains(value.shape()) => {
                        plan.subject_context_bounded = false;
                    }
                    _ => {}
                }
                if let IRComponent::Class(class) = component {
                    plan.has_class_constraint = true;
                    let term = OxigraphInMemory::object_as_term(class.class_rule()).to_string();
                    let iri = term.strip_prefix('<').and_then(|t| t.strip_suffix('>'));
                    if let (Some(SHACLPath::Predicate { pred }), Some(iri)) = (shape.path(), iri) {
                        plan.class_requirements
                            .insert((pred.as_str().to_owned(), iri.to_owned()));
                    }
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

/// Record which forward predicates a shape's path reads from its focus
/// (aegis-7aee). An inverse predicate reads incoming edges, which the
/// incoming-target lane loads, not the focus's own facts. Anything else cannot
/// be bounded, so the plan falls back to the full focus context.
fn plan_subject_path(path: &SHACLPath, plan: &mut CardinalityPlan) {
    match path {
        SHACLPath::Predicate { pred } => {
            plan.subject_predicates.insert(pred.as_str().to_owned());
        }
        SHACLPath::Inverse { path: inner }
            if matches!(inner.as_ref(), SHACLPath::Predicate { .. }) => {}
        SHACLPath::Alternative { paths } => {
            for member in paths {
                match member {
                    SHACLPath::Predicate { .. } | SHACLPath::Inverse { .. } => {
                        plan_subject_path(member, plan);
                    }
                    _ => plan.subject_context_bounded = false,
                }
            }
        }
        _ => plan.subject_context_bounded = false,
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

#[cfg(test)]
mod pin_deactivation_tests {
    use super::*;

    #[test]
    fn static_true_skips_and_false_keeps_cardinality_obligation() {
        for (value, active) in [("true", false), ("false", true)] {
            let shapes = format!(
                "@prefix sh: <http://www.w3.org/ns/shacl#> .
                @prefix ex: <urn:test:> .
                ex:S a sh:NodeShape ; sh:targetClass ex:C ; sh:deactivated {value} ;
                sh:property [ sh:path ex:p ; sh:maxCount 1 ] ."
            );
            let validator = Validator::from_turtle(&shapes).unwrap();
            assert_eq!(
                validator
                    .cardinality_plan()
                    .unwrap()
                    .has_value_set_constraint,
                active
            );
        }
    }
}
