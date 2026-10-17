//! Stable, scope-qualified community identities and deterministic overlap matching.
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn digest(parts: &[String]) -> String {
    // Length framing distinguishes e.g. ["ab", "c"] from ["a", "bc"].
    let mut hash = Sha256::new();
    for part in parts {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part.as_bytes());
    }
    hex::encode(hash.finalize())
}

pub(super) struct Identity {
    pub iri: String,
    pub derived_from: Vec<String>,
}

pub(super) fn match_groups(
    scope: &str,
    groups: &[Vec<String>],
    previous: &BTreeMap<String, BTreeSet<String>>,
    threshold: f64,
) -> Vec<Identity> {
    let mut inverted: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (old, members) in previous {
        for member in members {
            inverted.entry(member).or_default().push(old);
        }
    }
    // Only intersecting groups are candidates, rather than a quadratic scan of
    // all old/new pairs. One old identity can survive in at most one child.
    let mut overlaps = Vec::new();
    let mut lineage = vec![Vec::new(); groups.len()];
    for (new, members) in groups.iter().enumerate() {
        let mut intersections: BTreeMap<&str, usize> = BTreeMap::new();
        for member in members {
            for old in inverted.get(member.as_str()).into_iter().flatten() {
                *intersections.entry(old).or_default() += 1;
            }
        }
        for (old, intersection) in intersections {
            lineage[new].push(old.to_string());
            let union = members.len() + previous[old].len() - intersection;
            if intersection as f64 / union as f64 >= threshold {
                overlaps.push((new, old.to_string(), intersection, union));
            }
        }
    }
    overlaps.sort_by(|a, b| {
        // Exact fraction ordering, then intersection, old IRI, new member IRIs.
        ((b.2 as u128) * a.3 as u128)
            .cmp(&((a.2 as u128) * b.3 as u128))
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| groups[a.0].cmp(&groups[b.0]))
    });
    let mut assigned = BTreeMap::new();
    let mut used = BTreeSet::new();
    for (new, old, _, _) in overlaps {
        if !assigned.contains_key(&new) && used.insert(old.clone()) {
            assigned.insert(new, old);
        }
    }
    groups
        .iter()
        .enumerate()
        .map(|(i, members)| {
            let iri = assigned.remove(&i).unwrap_or_else(|| {
                let mut parts = vec![scope.to_string()];
                parts.extend(members.iter().cloned());
                format!("urn:quipu:community:{}", digest(&parts))
            });
            Identity {
                derived_from: lineage[i]
                    .iter()
                    .filter(|old| **old != iri)
                    .cloned()
                    .collect(),
                iri,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn members(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).into()).collect()
    }
    #[test]
    fn unrelated_group_removal_preserves_identity() {
        let old = BTreeMap::from([
            ("old:a".into(), members(&["a", "b"]).into_iter().collect()),
            ("old:z".into(), members(&["y", "z"]).into_iter().collect()),
        ]);
        let result = match_groups("scope", &[members(&["y", "z"])], &old, 0.5);
        assert_eq!(result[0].iri, "old:z");
    }
    #[test]
    fn split_reuses_identity_once_with_deterministic_tie() {
        let old = BTreeMap::from([(
            "old:a".into(),
            members(&["a", "b", "c", "d"]).into_iter().collect(),
        )]);
        let result = match_groups(
            "scope",
            &[members(&["c", "d"]), members(&["a", "b"])],
            &old,
            0.5,
        );
        assert_eq!(result[1].iri, "old:a");
        assert_ne!(result[0].iri, "old:a");
        assert_eq!(result[0].derived_from, vec!["old:a"]);
    }
    #[test]
    fn merge_retains_one_parent_and_records_other() {
        let old = BTreeMap::from([
            ("old:a".into(), members(&["a", "b"]).into_iter().collect()),
            ("old:b".into(), members(&["c", "d"]).into_iter().collect()),
        ]);
        let result = match_groups("scope", &[members(&["a", "b", "c", "d"])], &old, 0.5);
        assert_eq!(result[0].iri, "old:a");
        assert_eq!(result[0].derived_from, vec!["old:b"]);
        let separate = match_groups("other", &[members(&["a"])], &BTreeMap::new(), 0.5);
        let first = match_groups("scope", &[members(&["a"])], &BTreeMap::new(), 0.5);
        assert_ne!(separate[0].iri, first[0].iri);
    }
}
