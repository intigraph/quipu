//! Store scans and text rendering for the proposal plan (aegis-493), split
//! from `proposal_plan.rs`.

use std::collections::BTreeSet;

use super::{ProposalPlan, Terms};
use crate::error::Result;
use crate::store::Store;
use crate::types::Value;

/// Names of current stored queries whose template names a term in `changed`,
/// as a full IRI or as `prefix:local` under a `PREFIX` the template declares.
pub(super) fn stored_queries_naming(
    store: &Store,
    changed: &BTreeSet<String>,
) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for q in store.query_list()? {
        let prefixes = declared_prefixes(&q.template);
        let names = |iri: &str| {
            q.template.contains(&format!("<{iri}>"))
                || prefixes.iter().any(|(p, ns)| {
                    iri.strip_prefix(ns.as_str())
                        .is_some_and(|local| names_prefixed(&q.template, &format!("{p}:{local}")))
                })
        };
        if changed.iter().any(|t| names(t)) {
            out.push(q.name);
        }
    }
    Ok(out)
}

/// Whether `template` uses the prefixed name `pname` as a whole token, so
/// `ex:email` does not match inside `ex:emailAddress` or `aex:email`.
pub(super) fn names_prefixed(template: &str, pname: &str) -> bool {
    let name_char = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | ':');
    template.match_indices(pname).any(|(at, _)| {
        let before = template[..at].chars().next_back();
        let after = template[at + pname.len()..].chars().next();
        !before.is_some_and(name_char) && !after.is_some_and(name_char)
    })
}

/// `(prefix, namespace)` for every `PREFIX p: <ns>` in a SPARQL template.
fn declared_prefixes(template: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = template;
    while let Some(at) = rest.to_ascii_uppercase().find("PREFIX") {
        rest = &rest[at + "PREFIX".len()..];
        let Some((name, tail)) = rest.split_once(':') else {
            break;
        };
        let Some((ns, _)) = tail
            .trim_start()
            .strip_prefix('<')
            .and_then(|t| t.split_once('>'))
        else {
            continue;
        };
        out.push((name.trim().to_owned(), ns.to_owned()));
    }
    out
}

/// Named graphs, split into (live, frozen snapshot), whose data uses a
/// proposed or removed property as a predicate, or a class as a type.
pub(super) fn graphs_using(
    store: &Store,
    proposed: &Terms,
    removed: &BTreeSet<String>,
) -> Result<(Vec<String>, Vec<String>)> {
    // A removed term may have been a class or a property: look for both uses.
    let patterns = proposed
        .properties
        .iter()
        .chain(removed)
        .map(|p| format!("?s <{p}> ?o"))
        .chain(
            proposed
                .classes
                .iter()
                .chain(removed)
                .map(|c| format!("?s a <{c}>")),
        )
        .collect::<Vec<_>>();
    let mut graphs = BTreeSet::new();
    for pattern in &patterns {
        let q = format!("SELECT DISTINCT ?g WHERE {{ GRAPH ?g {{ {pattern} }} }}");
        for row in crate::sparql::query(store, &q)?.rows() {
            if let Some(Value::Ref(id)) = row.get("g") {
                graphs.insert(store.resolve(*id)?);
            }
        }
    }
    // Frozen graphs are cold: `GRAPH ?g` does not compose them, so each is
    // asked by name, which does.
    let frozen: Vec<String> = store
        .conn
        .prepare("SELECT DISTINCT graph_iri FROM frozen_packs WHERE thawed_at IS NULL")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<_, _>>()?;
    let mut snapshots = Vec::new();
    for g in frozen {
        graphs.remove(&g);
        for pattern in &patterns {
            let q = format!("SELECT ?s WHERE {{ GRAPH <{g}> {{ {pattern} }} }} LIMIT 1");
            if !crate::sparql::query(store, &q)?.rows().is_empty() {
                snapshots.push(g);
                break;
            }
        }
    }
    Ok((graphs.into_iter().collect(), snapshots))
}

/// The plan as human text, one fact per line.
#[must_use]
pub fn render_text(plan: &ProposalPlan) -> String {
    let list = |s: &mut String, label: &str, items: &mut dyn Iterator<Item = &String>| {
        let items: Vec<&String> = items.collect();
        s.push_str(&format!("{label}: {}\n", items.len()));
        for i in items {
            s.push_str(&format!("  {i}\n"));
        }
    };
    let mut s = format!(
        "proposal {} ({:?} {}): risk {}/{} -> {:?}\n",
        plan.proposal_id,
        plan.kind,
        plan.target,
        plan.risk.score,
        plan.risk.threshold,
        plan.risk.verdict
    );
    list(&mut s, "classes", &mut plan.classes.iter());
    list(&mut s, "properties", &mut plan.properties.iter());
    list(&mut s, "removed terms", &mut plan.removed_terms.iter());
    s.push_str(&format!(
        "instances in scope: {}\nnewly failing SHACL: {} instance(s), {} violation(s)\n",
        plan.instances_in_scope, plan.newly_failing.instances, plan.newly_failing.violations
    ));
    for i in &plan.newly_failing.sample {
        s.push_str(&format!(
            "  {} {} {}\n",
            i.focus_node,
            i.path.as_deref().unwrap_or("-"),
            i.component
        ));
    }
    list(&mut s, "stored queries", &mut plan.stored_queries.iter());
    list(&mut s, "named graphs", &mut plan.named_graphs.iter());
    list(&mut s, "snapshots", &mut plan.snapshots.iter());
    s.push_str("risk inputs:\n");
    for i in &plan.risk.inputs {
        s.push_str(&format!(
            "  {:<24} {:>6} -> {:>2}/{}\n",
            i.name, i.value, i.points, i.max
        ));
    }
    for u in &plan.undetermined {
        s.push_str(&format!("undetermined: {u}\n"));
    }
    s
}
