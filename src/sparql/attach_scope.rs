//! Whether a query can read a graph an attachment contributes (aegis-xacr).
//!
//! Production attaches one frozen pack. Every fast path gated on
//! `Store::has_attachments` therefore declined for every query, including the
//! ones that read only local graphs and so could never see a row of it:
//! on a copy of the 2026-10-21 board, priorities took 0.36 s instead of
//! 0.08 s and closes 3.9 s instead of 2.1 s, with identical answers.
//!
//! An attachment contributes rows only to the graphs it registered, and their
//! ids come from its own term space. A query whose every graph scope names
//! none of them reads only local rows. Those rows answer exactly as on the
//! detached store unless one of them names a term by its ATTACHED id where the
//! same IRI also has a local one (an alias): only the composed paths
//! canonicalise those, so such a scope keeps the composed view. So does
//! anything else this cannot prove: `GRAPH ?g`, `SERVICE`, and history reads.

use rusqlite::params;
use spargebra::Query;
use spargebra::term::NamedNodePattern;

use super::root_scope::{Scope, visit_scopes};
use super::{GraphScope, TemporalContext};
use crate::error::Result;
use crate::store::Store;
use crate::types::Value;

/// Past this many aliases the per-graph probe stops being cheap.
const MAX_ALIASES: usize = 4096;

/// Whether graph ?1 holds a local fact naming id ?2 (as ?3, its Ref blob).
const ALIAS_PROBE: &str = "SELECT EXISTS(SELECT 1 FROM main.facts WHERE g = ?1 AND e = ?2) \
         OR EXISTS(SELECT 1 FROM main.facts WHERE a = ?2 AND +g = ?1) \
         OR EXISTS(SELECT 1 FROM main.facts WHERE v = ?3 AND +g = ?1)";

/// `true` when evaluating `query` under `dataset` must see the composed store.
///
/// # Errors
/// Store errors while resolving a `GRAPH <iri>` name or probing for aliases.
pub(super) fn composes(
    store: &Store,
    query: &Query,
    dataset: &GraphScope,
    ctx: &TemporalContext,
) -> Result<bool> {
    let attached = store.attached_graphs();
    if attached.is_empty() {
        return Ok(false);
    }
    if ctx.valid_at.is_some() || ctx.as_of_tx.is_some() {
        return Ok(true);
    }
    let mut graphs = match dataset {
        GraphScope::Default(ids) | GraphScope::Named(ids) => ids.clone(),
        GraphScope::AnyNamed { .. } => return Ok(true),
    };
    let mut open_scope = false;
    visit_scopes(query, |scope| {
        match scope {
            Scope::Graph(NamedNodePattern::NamedNode(iri)) => {
                graphs.extend(store.lookup_all(iri.as_str())?);
            }
            Scope::Graph(NamedNodePattern::Variable(_)) | Scope::Service => open_scope = true,
        }
        Ok(())
    })?;
    if open_scope || graphs.iter().any(|g| attached.binary_search(g).is_ok()) {
        return Ok(true);
    }
    names_an_alias(store, &graphs)
}

/// Whether a local fact in one of `graphs` names an attached alias id.
fn names_an_alias(store: &Store, graphs: &[i64]) -> Result<bool> {
    let mut aliases = Vec::new();
    {
        let mut stmt = store.prepare("SELECT alias_id FROM term_alias LIMIT ?1")?;
        let mut rows = stmt.query(params![i64::try_from(MAX_ALIASES + 1).unwrap_or(i64::MAX)])?;
        while let Some(row) = rows.next()? {
            aliases.push(row.get::<_, i64>(0)?);
        }
    }
    if aliases.len() > MAX_ALIASES {
        return Ok(true);
    }
    // Each probe is one index seek: (g, e) on idx_geav, a on idx_aevt and the
    // exact Ref blob on idx_vaet. `+g` keeps the planner off idx_geav for the
    // last two: with no sqlite_stat1 it otherwise picks the graph prefix and
    // walks every row of the graph, per alias (measured: over 60 s on the
    // production board). History rows count too, so the check is a property
    // of the graph rather than of one point in time.
    let mut stmt = store.prepare(ALIAS_PROBE)?;
    for &g in graphs.iter().filter(|g| **g >= 0) {
        for &alias in &aliases {
            let named: bool = stmt
                .query_row(params![g, alias, Value::Ref(alias).to_bytes()], |r| {
                    r.get(0)
                })?;
            if named {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
#[path = "attach_scope_tests.rs"]
mod tests;
