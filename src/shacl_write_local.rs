//! Value-local logical checks and addition-safe closed/qualified verdicts.
//!
//! Counts, classes, required values and referenced neighborhoods can change
//! truth after later writes. Their logical wrappers retain existing policy.
//! Unknown and recursive expressions are conservatively left unchanged.

use std::collections::{HashMap, HashSet};

use shacl_engine::ir::{IRComponent, IRSchema, IRShape, ShapeLabelIdx};

pub(super) fn components(schema: &IRSchema, shape: &IRShape) -> Vec<&'static str> {
    let mut admitted = Vec::new();
    let mut memo = HashMap::new();
    // Closed property shapes inspect their values' neighborhoods, which this
    // plan deliberately does not add to the optional value closure.
    if shape.closed() && shape.path().is_none() {
        admitted.push("ClosedConstraintComponent");
    }
    for component in shape.components() {
        let mut local = |index| value_local(schema, index, &mut HashSet::new(), &mut memo);
        match component {
            IRComponent::And(value) if value.shapes().iter().all(&mut local) => {
                admitted.push("AndConstraintComponent");
            }
            IRComponent::Or(value) if value.shapes().iter().all(&mut local) => {
                admitted.push("OrConstraintComponent");
            }
            IRComponent::Xone(value) if value.shapes().iter().all(&mut local) => {
                admitted.push("XoneConstraintComponent");
            }
            IRComponent::Not(value) if local(value.shape()) => {
                admitted.push("NotConstraintComponent");
            }
            IRComponent::QualifiedValueShape(value)
                if value.qualified_max_count().is_some()
                    && local(value.shape())
                    && value.siblings().iter().all(&mut local) =>
            {
                // Qualified minCount is intentionally not admitted here.
                admitted.push("QualifiedMaxCountConstraintComponent");
            }
            _ => {}
        }
    }
    admitted
}

fn value_local(
    schema: &IRSchema,
    index: &ShapeLabelIdx,
    visiting: &mut HashSet<ShapeLabelIdx>,
    memo: &mut HashMap<ShapeLabelIdx, bool>,
) -> bool {
    if let Some(result) = memo.get(index) {
        return *result;
    }
    // Deep/recursive expressions retain their old policy. Planning must not
    // consume an unbounded call stack or re-expand a shared logical DAG.
    if visiting.len() >= 128 || !visiting.insert(*index) {
        return false;
    }
    let result = schema.get_shape_from_idx(index).is_some_and(|shape| {
        if shape.deactivated() {
            return true;
        }
        shape.path().is_none()
            && shape.property_shapes().is_empty()
            && !shape.closed()
            && shape.components().iter().all(|component| match component {
                IRComponent::Datatype(_)
                | IRComponent::NodeKind(_)
                | IRComponent::Pattern(_)
                | IRComponent::MinLength(_)
                | IRComponent::MaxLength(_)
                | IRComponent::MinExclusive(_)
                | IRComponent::MinInclusive(_)
                | IRComponent::MaxExclusive(_)
                | IRComponent::MaxInclusive(_)
                | IRComponent::In(_)
                | IRComponent::LanguageIn(_)
                | IRComponent::Deactivated(_) => true,
                IRComponent::And(value) => value
                    .shapes()
                    .iter()
                    .all(|i| value_local(schema, i, visiting, memo)),
                IRComponent::Or(value) => value
                    .shapes()
                    .iter()
                    .all(|i| value_local(schema, i, visiting, memo)),
                IRComponent::Xone(value) => value
                    .shapes()
                    .iter()
                    .all(|i| value_local(schema, i, visiting, memo)),
                IRComponent::Not(value) => value_local(schema, value.shape(), visiting, memo),
                _ => false,
            })
    });
    visiting.remove(index);
    memo.insert(*index, result);
    result
}
