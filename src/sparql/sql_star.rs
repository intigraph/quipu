//! Bounded SQL relations for a current single named-graph subject star.
//!
//! An unrelated archive must not turn a selective star into one composed SQL
//! evaluation per owner. Each relation uses the ordinary triple evaluator,
//! including its term identity and graph semantics, then the existing binding
//! join. A complete relation is required: the extra row selects the original
//! planner rather than returning a truncated answer.

use spargebra::term::{NamedNodePattern, TermPattern, TriplePattern};

use super::pattern_util::{check_eval_budget, triple_pattern_vars};
use super::triple::eval_triple_pattern_limited;
use super::{Bindings, TemporalContext};
use crate::{Result, Store};

const MAX_RELATION_ROWS: usize = 50_000;

pub(super) fn try_eval(
    store: &Store,
    patterns: &[TriplePattern],
    ctx: &TemporalContext,
    seed: &Bindings,
) -> Result<Option<(Vec<Bindings>, Vec<String>)>> {
    if !(2..=8).contains(&patterns.len())
        || !store.has_attachments()
        || !seed.is_empty()
        || ctx.valid_at.is_some()
        || ctx.as_of_tx.is_some()
        || ctx.row_limit.is_some()
        || ctx.row_cap.is_some_and(|cap| cap < MAX_RELATION_ROWS + 1)
        || ctx.string_narrows.is_some()
        || !ctx.graph.single_graph().is_some_and(|graph| graph != 0)
        || !matches!(patterns[0].subject, TermPattern::Variable(_))
        || patterns.iter().any(|pattern| {
            pattern.subject != patterns[0].subject
                || !matches!(pattern.predicate, NamedNodePattern::NamedNode(_))
                || matches!(pattern.object, TermPattern::BlankNode(_))
        })
        || !patterns.iter().any(|pattern| {
            matches!(
                pattern.object,
                TermPattern::NamedNode(_) | TermPattern::Literal(_)
            )
        })
    {
        return Ok(None);
    }
    let mut variables = Vec::new();
    for pattern in patterns {
        for variable in triple_pattern_vars(pattern) {
            if !variables.contains(&variable) {
                variables.push(variable);
            }
        }
    }
    let mut relations = Vec::with_capacity(patterns.len());
    for (index, pattern) in patterns.iter().enumerate() {
        check_eval_budget(ctx, index, 0)?;
        let rows = match eval_triple_pattern_limited(
            store,
            pattern,
            seed,
            ctx,
            Some(MAX_RELATION_ROWS + 1),
        ) {
            Ok(rows) => rows,
            // An unbound relation may encounter a malformed unrelated row
            // that the original seeded lookup never visits. Retain that
            // evaluator rather than introduce a speculative-plan error.
            // SQLite wraps term-key decoder errors as SQL errors too. An
            // interrupted statement and all query budgets still propagate;
            // the original lookup decides other errors on its selected rows.
            Err(crate::Error::InvalidValue(_) | crate::Error::UnknownTerm(_)) => return Ok(None),
            Err(crate::Error::Sqlite(error))
                if !matches!(&error, rusqlite::Error::SqliteFailure(code, _)
                    if code.code == rusqlite::ErrorCode::OperationInterrupted) =>
            {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        if rows.len() > MAX_RELATION_ROWS {
            return Ok(None);
        }
        if rows.is_empty() {
            return Ok(Some((Vec::new(), variables)));
        }
        relations.push(rows);
    }
    // Keep source order; headers and binding compatibility share the existing
    // implementation. No cap is pushed through an intermediate relation.
    let mut rows = vec![seed.clone()];
    for (index, relation) in relations.iter().enumerate() {
        check_eval_budget(ctx, index, rows.len())?;
        rows = super::join::hash_join_bindings(&rows, relation, ctx)?;
        if rows.is_empty() {
            break;
        }
    }
    Ok(Some((rows, variables)))
}

#[cfg(test)]
mod tests;
