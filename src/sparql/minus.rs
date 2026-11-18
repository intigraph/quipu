//! `MINUS` by hashed lookup instead of a left x right nested loop (aegis-49s).
//!
//! SPARQL 1.1 section 8.3: a left solution is removed when SOME right solution
//! shares at least one variable with it and is compatible with it. The old
//! evaluator tested every (left, right) pair and allocated a merged row per
//! test; on the repository share (323k left rows against every regex-matched
//! triple) that never finished inside the 30 s budget.
//!
//! Semantics are kept exactly, including the old evaluator's edge cases:
//! - the shared domain counts every key, federation metadata included;
//! - compatibility ignores the federation metadata keys, as `merge_bindings`
//!   does (they never make two rows incompatible);
//! - values compare by `Value::term_key`, which is equivalent to `Value`'s
//!   `PartialEq` (the RDF term identity), so `Ref` and `Str` stay distinct.
//!
//! Right rows are grouped by their exact key set. For a left row with domain
//! D and a group with key set K, the shared domain is K and D; if it is empty
//! the group cannot exclude the row. Otherwise the row is excluded when the
//! group holds a row equal to it on the compatibility keys, found by a hash
//! index built once per (group, compatibility keys) and then reused.

use std::collections::{HashMap, HashSet};

use super::Bindings;
use super::TemporalContext;
use crate::error::Result;

/// Response metadata keys that `merge_bindings` never treats as a conflict.
fn is_metadata(key: &str) -> bool {
    matches!(key, "_provider" | "_trust" | "_freshness")
}

/// Projected term keys of one group's rows, per (group, compatibility keys).
type Indexes = HashMap<(usize, Vec<String>), HashSet<Vec<Vec<u8>>>>;

struct Group {
    keys: Vec<String>,
    rows: Vec<Bindings>,
}

/// The rows of `left` that no row of `right` excludes, in their original order.
pub(super) fn subtract(
    left: Vec<Bindings>,
    right: &[Bindings],
    ctx: &TemporalContext,
) -> Result<Vec<Bindings>> {
    let mut groups: Vec<Group> = Vec::new();
    let mut by_keys: HashMap<Vec<String>, usize> = HashMap::new();
    for row in right {
        let mut keys: Vec<String> = row.keys().cloned().collect();
        keys.sort_unstable();
        let index = *by_keys.entry(keys.clone()).or_insert_with(|| {
            groups.push(Group {
                keys,
                rows: Vec::new(),
            });
            groups.len() - 1
        });
        groups[index].rows.push(row.clone());
    }

    let mut indexes: Indexes = HashMap::new();
    let mut kept = Vec::with_capacity(left.len());
    let mut probes = 0usize;
    for row in left {
        let mut excluded = false;
        for (index, group) in groups.iter().enumerate() {
            probes += 1;
            let shared: Vec<&String> = group
                .keys
                .iter()
                .filter(|key| row.contains_key(key.as_str()))
                .collect();
            if shared.is_empty() {
                continue;
            }
            let compare: Vec<String> = shared
                .into_iter()
                .filter(|key| !is_metadata(key))
                .cloned()
                .collect();
            if compare.is_empty() {
                // Shares only metadata: compatible with any row of the group.
                excluded = true;
                break;
            }
            let probe: Vec<Vec<u8>> = compare.iter().map(|key| row[key].term_key()).collect();
            let set = indexes
                .entry((index, compare))
                .or_insert_with_key(|(_, compare)| {
                    group
                        .rows
                        .iter()
                        .map(|r| compare.iter().map(|key| r[key].term_key()).collect())
                        .collect()
                });
            if set.contains(&probe) {
                excluded = true;
                break;
            }
        }
        super::pattern_util::check_eval_budget(ctx, probes, kept.len())?;
        if !excluded {
            kept.push(row);
        }
    }
    Ok(kept)
}

#[cfg(test)]
#[path = "minus_tests.rs"]
mod tests;
