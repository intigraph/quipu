//! The shared time-free graph projection used by pendant and DataBook Git adapters.
use super::{Decisions, invalid};
use crate::error::Result;
use crate::share_merge::{Graph, parse_graph};
use std::collections::BTreeMap;

pub(super) fn prepare(
    base: &Graph,
    ours: &Graph,
    theirs: &Graph,
    shapes: &str,
    inputs: [String; 3],
) -> Result<(Graph, Decisions)> {
    let (graph, _) = super::core::merge(base, ours, theirs, shapes)?;
    let (_, conflicts) = crate::merge_review::merge(base, ours, theirs, shapes)?;
    let kinds = conflicts
        .iter()
        .map(crate::share_merge_decisions_view::kind)
        .collect();
    let resolutions = conflicts
        .iter()
        .enumerate()
        .filter(|(_, r)| crate::merge_review::convergent(r))
        .map(|(i, _)| (format!("conflict:{i}"), "continue".into()))
        .collect();
    let decisions = Decisions {
        schema: "https://github.com/scbrown/quipu/git-decisions/v2".into(),
        inputs,
        conflicts,
        kinds,
        aliases: super::propose(base, ours, theirs),
        resolutions,
        resolved_by: BTreeMap::new(),
    };
    Ok((graph, decisions))
}

pub(super) fn apply(
    source: &str,
    decisions: &mut Decisions,
    recorded: Option<&Decisions>,
) -> Result<(Graph, bool)> {
    if let Some(recorded) = recorded {
        let mut expected = decisions.clone();
        if recorded.schema == "https://github.com/scbrown/quipu/git-decisions/v1" {
            expected.schema = recorded.schema.clone();
            expected
                .conflicts
                .retain(|r| !crate::merge_review::convergent(r));
            expected.kinds.clear();
        }
        expected.resolutions = recorded.resolutions.clone();
        expected.resolved_by = recorded.resolved_by.clone();
        if expected != *recorded {
            return Err(invalid(
                "stale or altered decisions: inputs/proposals differ",
            ));
        }
        *decisions = expected;
    }
    let mut graph = parse_graph(source, "merge output")?;
    let mut known = std::collections::BTreeSet::new();
    let mut unresolved = 0;
    for (i, c) in decisions.conflicts.iter().enumerate() {
        let key = format!("conflict:{i}");
        known.insert(key.clone());
        let values = match decisions.resolutions.get(&key) {
            Some(choice) => super::resolved_values(c, choice)?,
            None => {
                unresolved += 1;
                continue;
            }
        };
        graph.retain(|t| t.subject.to_string() != c.subject || t.predicate.as_str() != c.predicate);
        for value in &values {
            graph.extend(parse_graph(
                &format!("{} <{}> {} .", c.subject, c.predicate, value),
                "resolution",
            )?);
        }
    }
    for (i, a) in decisions.aliases.iter().enumerate() {
        let key = format!("alias:{i}");
        known.insert(key.clone());
        match decisions.resolutions.get(&key).map(String::as_str) {
            Some("accept") => graph.extend(parse_graph(
                &format!(
                    "{} <http://www.w3.org/2002/07/owl#sameAs> {} .",
                    a.ours, a.theirs
                ),
                "alias resolution",
            )?),
            Some("reject") => {}
            None => unresolved += 1,
            _ => return Err(invalid("alias resolution must be accept/reject")),
        }
    }
    if decisions.resolutions.keys().any(|k| !known.contains(k)) {
        return Err(invalid("unknown decision key"));
    }
    Ok((graph, unresolved == 0))
}
