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
    let object_leaf = matches!(&pattern.predicate, NamedNodePattern::NamedNode(_))
        && match &pattern.object {
            TermPattern::NamedNode(_) | TermPattern::Literal(_) => true,
            TermPattern::Variable(object) => seed
                .get(object.as_str())
                .is_some_and(|value| !matches!(value, crate::Value::Ref(id) if *id < 0)),
            TermPattern::BlankNode(_) => false,
        };
    if (!text_leaf && !bound_leaf && !object_leaf)
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
    let definition: Option<String> = store
        .conn
        .query_row(
            "SELECT sql FROM main.sqlite_master WHERE type='index' AND name='idx_current_g'",
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
    if normalized != "createindexidx_current_gonfacts(g)whereop=1andvalid_toisnull" {
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

#[cfg(test)]
mod tests;

/// A strict value-leading source for an object-bound local leaf.
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
    let bound_object = match &pattern.object {
        TermPattern::NamedNode(_) | TermPattern::Literal(_) => true,
        TermPattern::Variable(object) => seed
            .get(object.as_str())
            .is_some_and(|value| !matches!(value, crate::Value::Ref(id) if *id < 0)),
        TermPattern::BlankNode(_) => false,
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
    Ok(Some("facts INDEXED BY idx_vaet"))
}
