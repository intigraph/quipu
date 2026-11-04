//! Read current graph membership from its rowid index for a narrowed title leaf.
//!
//! The predicate/value index supplies the selected triple, but checking `g`
//! against the fact table faults payload pages even when the string filter
//! leaves few candidates. Its current graph index carries the same rowid.
//! Keep the ordinary graph predicate for every unproved shape or index.
use rusqlite::OptionalExtension;
use spargebra::term::{NamedNodePattern, TermPattern, TriplePattern};

use super::{Bindings, TemporalContext};
use crate::{Result, Store};

pub(super) fn predicate(
    store: &Store,
    ctx: &TemporalContext,
    pattern: &TriplePattern,
    seed: &Bindings,
    graphs: &[i64],
    params: &mut Vec<Box<dyn rusqlite::types::ToSql>>,
) -> Result<Option<String>> {
    let text_leaf = matches!(
        (&pattern.subject, &pattern.predicate, &pattern.object),
        (TermPattern::Variable(_), NamedNodePattern::NamedNode(_), TermPattern::Variable(value))
        if seed.is_empty() && ctx.string_narrows.as_ref()
            .is_some_and(|narrows| narrows.iter().any(|n| n.var == value.as_str()))
    );
    // A positive local subject and constant predicate already narrow the leaf.
    // Only graph membership changes: every original value, temporal and bag
    // condition remains in the caller, and the same physical rowid is tested.
    let bound_leaf = matches!(
        (&pattern.subject, &pattern.predicate),
        (TermPattern::NamedNode(_), NamedNodePattern::NamedNode(_))
    ) || matches!(
        (&pattern.subject, &pattern.predicate),
        (TermPattern::Variable(subject), NamedNodePattern::NamedNode(_))
        if matches!(seed.get(subject.as_str()), Some(crate::Value::Ref(id)) if *id >= 0)
    );
    // Object-bound metadata (identifier/type) has the same narrow value
    // condition in the caller. Read graph membership from the same physical
    // assertion rowid without fetching its potentially wide fact-table page.
    // SPARQL 1.2 adds Triple through feature unification; unsupported terms
    // must retain the ordinary evaluator in either feature configuration.
    #[allow(clippy::match_wildcard_for_single_variants)]
    let object_leaf = matches!(&pattern.predicate, NamedNodePattern::NamedNode(_))
        && match &pattern.object {
            TermPattern::NamedNode(_) | TermPattern::Literal(_) => true,
            TermPattern::Variable(object) => seed
                .get(object.as_str())
                .is_some_and(|value| !matches!(value, crate::Value::Ref(id) if *id < 0)),
            _ => false,
        };
    let predicate_leaf = plain_predicate_leaf(pattern, seed)
        && ctx.string_narrows.is_none()
        && matches!(&ctx.graph, super::GraphScope::Named(active) if active.as_slice() == graphs);
    if (!text_leaf && !bound_leaf && !object_leaf && !predicate_leaf)
        || store.has_attachments()
        || ctx.valid_at.is_some()
        || ctx.as_of_tx.is_some()
        || graphs.len() != 1
        || graphs[0] == 0
    {
        return Ok(None);
    }
    // A same-named index with another partial predicate cannot prove complete
    // current membership. Older stores and alternate profiles retain fallback.
    if !current_g_index(store)? {
        return Ok(None);
    }
    let graph_param = params.len() + 1;
    params.push(Box::new(graphs[0]));
    Ok(Some(format!(
        "EXISTS (SELECT 1 FROM main.facts AS scope INDEXED BY idx_current_g
            WHERE scope.g=?{graph_param} AND scope.rowid=facts.rowid
            AND scope.op=1 AND scope.valid_to IS NULL)"
    )))
}

/// Does `idx_current_g` exist with exactly the current-membership definition?
/// The answer only changes with the schema, so it is memoized against the
/// schema cookie: a correlated leaf asks once per outer row, and scanning
/// `sqlite_master` for it was a third of a board aggregate (aegis-ek4).
fn current_g_index(store: &Store) -> Result<bool> {
    let version: i64 = store
        .conn
        .prepare_cached("PRAGMA main.schema_version")?
        .query_row([], |row| row.get(0))?;
    if let Some((seen, exact)) = store.current_g_index.get()
        && seen == version
    {
        return Ok(exact);
    }
    let definition: Option<String> = store
        .conn
        .prepare_cached(
            "SELECT sql FROM main.sqlite_master WHERE type='index' AND name='idx_current_g'",
        )?
        .query_row([], |row| row.get(0))
        .optional()?;
    let exact = definition.is_some_and(|definition| {
        let normalized: String = definition
            .chars()
            .filter(|c| !c.is_ascii_whitespace())
            .flat_map(char::to_lowercase)
            .collect();
        normalized == "createindexidx_current_gonfacts(g)whereop=1andvalid_toisnull"
    });
    store.current_g_index.set(Some((version, exact)));
    Ok(exact)
}

#[cfg(test)]
mod tests;

/// Choose a covering source for a current, object-bound local leaf.
pub(super) fn object_source(
    store: &Store,
    ctx: &TemporalContext,
    pattern: &TriplePattern,
    seed: &Bindings,
) -> Result<Option<&'static str>> {
    let local_graph = matches!(&ctx.graph,
        super::GraphScope::Named(graphs) if graphs.len() == 1 && graphs[0] > 0);
    let unbound_subject = matches!(&pattern.subject,
        TermPattern::Variable(subject) if !seed.contains_key(subject.as_str()));
    // The wildcard also excludes the feature-dependent RDF-star Triple term.
    #[allow(clippy::match_wildcard_for_single_variants)]
    let bound_object = match &pattern.object {
        TermPattern::NamedNode(_) | TermPattern::Literal(_) => true,
        TermPattern::Variable(object) => seed
            .get(object.as_str())
            .is_some_and(|value| !matches!(value, crate::Value::Ref(id) if *id < 0)),
        _ => false,
    };
    if !local_graph
        || !unbound_subject
        || !bound_object
        || !matches!(&pattern.predicate, NamedNodePattern::NamedNode(_))
        || store.has_attachments()
        || ctx.valid_at.is_some()
        || ctx.as_of_tx.is_some()
    {
        return Ok(None);
    }
    let definition: Option<String> = store
        .conn
        .query_row(
            "SELECT sql FROM main.sqlite_master WHERE type='index' AND name='idx_vaet'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let Some(definition) = definition else {
        return Ok(None);
    };
    let normalized: String = definition
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if normalized != "createindexidx_vaetonfacts(v,a,e,valid_from)" {
        return Ok(None);
    }
    if dense_plain_object(store, pattern, seed)? {
        return Ok(Some("facts INDEXED BY idx_current_aev"));
    }
    Ok(Some("facts INDEXED BY idx_vaet"))
}

/// A value seek is cheap until a common literal carries a large history. Prove
/// that case from the covering value index before considering the current-only
/// predicate index; absent/selective owners and identifiers keep point seeks.
fn dense_plain_object(store: &Store, pattern: &TriplePattern, seed: &Bindings) -> Result<bool> {
    const DENSE_ROWS: i64 = 4096;
    const MAX_CURRENT_ROWS: i64 = 262_144;
    let Some(value @ crate::Value::Str(_)) =
        super::pattern_util::resolve_object_pattern(store, &pattern.object, seed)?
    else {
        return Ok(false);
    };
    let NamedNodePattern::NamedNode(predicate) = &pattern.predicate else {
        return Ok(false);
    };
    let ids = store.lookup_all(predicate.as_str())?;
    let [predicate] = ids.as_slice() else {
        return Ok(false);
    };
    let aliases = store.literal_aliases(&value, false)?;
    let mut parameters: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(*predicate)];
    let placeholders = aliases
        .into_iter()
        .map(|bytes| {
            parameters.push(Box::new(bytes) as Box<dyn rusqlite::types::ToSql>);
            format!("?{}", parameters.len())
        })
        .collect::<Vec<_>>()
        .join(",");
    // No op/valid_to filter here: both columns live off the value index. This
    // bounded, index-only probe measures history without fetching its pages.
    let sql = format!(
        "SELECT COUNT(*) FROM (SELECT 1 FROM facts INDEXED BY idx_vaet
        WHERE a=?1 AND v IN ({placeholders}) LIMIT {})",
        DENSE_ROWS + 1
    );
    let rows: i64 = store.prepare(&sql)?.query_row(
        rusqlite::params_from_iter(parameters.iter().map(std::convert::AsRef::as_ref)),
        |r| r.get(0),
    )?;
    if rows <= DENSE_ROWS {
        return Ok(false);
    }
    if !current_aev_index(store)? {
        return Ok(false);
    }
    // Refuse a global current scan whose size was not proved modest. Neither
    // the admission probe nor the chosen scan may grow with unlimited history.
    let current: i64 = store
        .prepare(&format!(
            "SELECT COUNT(*) FROM
        (SELECT 1 FROM facts INDEXED BY idx_current_aev WHERE a=?1
        AND op=1 AND valid_to IS NULL LIMIT {})",
            MAX_CURRENT_ROWS + 1
        ))?
        .query_row([predicate], |r| r.get(0))?;
    Ok(current <= MAX_CURRENT_ROWS)
}

fn current_aev_index(store: &Store) -> Result<bool> {
    let definition: Option<String> = store
        .conn
        .query_row(
            "SELECT sql FROM main.sqlite_master WHERE type='index' AND name='idx_current_aev'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(definition.is_some_and(|sql| {
        sql.chars()
            .filter(|c| !c.is_ascii_whitespace())
            .flat_map(char::to_lowercase)
            .collect::<String>()
            == "createindexidx_current_aevonfacts(a,e,v)whereop=1andvalid_toisnull"
    }))
}

fn plain_predicate_leaf(pattern: &TriplePattern, seed: &Bindings) -> bool {
    seed.is_empty()
        && matches!(
            (&pattern.subject, &pattern.predicate, &pattern.object),
            (
                TermPattern::Variable(_),
                NamedNodePattern::NamedNode(_),
                TermPattern::Variable(_)
            )
        )
}

/// Preflight absence still scans all matches when none belongs to the graph.
/// Intersect current covering indexes by the same physical row instead of
/// faulting every candidate's wide fact payload just to inspect graph membership.
pub(super) fn predicate_source(
    store: &Store,
    ctx: &TemporalContext,
    pattern: &TriplePattern,
    seed: &Bindings,
) -> Result<Option<&'static str>> {
    if !plain_predicate_leaf(pattern, seed)
        || store.has_attachments()
        || ctx.valid_at.is_some()
        || ctx.as_of_tx.is_some()
        || ctx.string_narrows.is_some()
        || ctx.entails_rdfs
        || !matches!(&ctx.graph, super::GraphScope::Named(graphs) if graphs.len()==1 && graphs[0]>0)
    {
        return Ok(None);
    }
    if current_g_index(store)? && current_aev_index(store)? {
        Ok(Some("facts INDEXED BY idx_current_aev"))
    } else {
        Ok(None)
    }
}
