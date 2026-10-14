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
    let (TermPattern::Variable(_), NamedNodePattern::NamedNode(_), TermPattern::Variable(value)) =
        (&pattern.subject, &pattern.predicate, &pattern.object)
    else {
        return Ok(None);
    };
    if !seed.is_empty()
        || store.has_attachments()
        || ctx.valid_at.is_some()
        || ctx.as_of_tx.is_some()
        || graphs.len() != 1
        || graphs[0] == 0
        || !ctx
            .string_narrows
            .as_ref()
            .is_some_and(|narrows| narrows.iter().any(|n| n.var == value.as_str()))
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
