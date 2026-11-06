//! Common, deterministic review rows, without changing the frozen set operator.
use crate::error::Result;
use crate::merge3::DecisionRecord;
use crate::share_merge::{Graph, by_slot, merge_graphs};
use std::collections::BTreeSet;

pub(crate) fn convergent(record: &DecisionRecord) -> bool {
    record.ours == record.theirs && record.ours != record.base
}

/// Include agreeing changes (including agreeing deletions) in slot order.
/// Unbounded convergence uses `usize::MAX` as the legacy record's numeric bound;
/// it is not silently treated as maxCount 1.
pub(crate) fn merge(
    base: &Graph,
    ours: &Graph,
    theirs: &Graph,
    shapes: &str,
) -> Result<(Graph, Vec<DecisionRecord>)> {
    let (graph, mut rows) = merge_graphs(base, ours, theirs, shapes)?;
    let (b, o, t) = (by_slot(base), by_slot(ours), by_slot(theirs));
    let bounds = crate::merge3::ShapeBounds::parse(shapes)?;
    let view = crate::merge3::TripleView::new(base.iter().chain(ours).chain(theirs));
    let keys: BTreeSet<_> = b.keys().chain(o.keys()).chain(t.keys()).collect();
    let empty = BTreeSet::new();
    for key in keys {
        let bv = b.get(key).unwrap_or(&empty);
        let ov = o.get(key).unwrap_or(&empty);
        let tv = t.get(key).unwrap_or(&empty);
        if ov == tv && ov != bv {
            rows.push(DecisionRecord {
                subject: key.0.clone(),
                predicate: key.1.clone(),
                max_count: bounds.bound(&key.0, &key.1, &view)?.unwrap_or(usize::MAX),
                base: bv.iter().cloned().collect(),
                ours: ov.iter().cloned().collect(),
                theirs: tv.iter().cloned().collect(),
            });
        }
    }
    rows.sort_by(|a, b| (&a.subject, &a.predicate).cmp(&(&b.subject, &b.predicate)));
    Ok((graph, rows))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::share_merge::parse_graph;

    #[test]
    fn agreement_and_agreeing_deletion_are_reported_in_stable_slot_order() {
        let base = parse_graph("<urn:s> <urn:p> \"a\" .\n<urn:s> <urn:q> \"b\" .", "base").unwrap();
        let next = parse_graph("<urn:s> <urn:p> \"c\" .", "side").unwrap();
        let first = merge(&base, &next, &next, "").unwrap();
        assert_eq!(first.1.len(), 2);
        assert!(first.1.iter().all(convergent));
        assert!(first.1[1].ours.is_empty());
        assert_eq!(first.0, next);
        let bytes = serde_json::to_vec(&first.1).unwrap();
        assert_eq!(
            bytes,
            serde_json::to_vec(&merge(&base, &next, &next, "").unwrap().1).unwrap()
        );
        // The frozen legacy operator still emits no conflict for agreement.
        assert!(merge_graphs(&base, &next, &next, "").unwrap().1.is_empty());
    }
}
