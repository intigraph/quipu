//! Batched plain-property absence for already selected current named subjects.
use super::pattern_util::check_eval_budget;
use super::triple::subject_relation;
use super::{Bindings, TemporalContext};
use crate::{Result, Store, Value};
use spargebra::algebra::{Expression, GraphPattern};
use spargebra::term::{NamedNodePattern, TermPattern};
use std::collections::HashSet;

pub(super) fn filter(
    store: &Store,
    expr: &Expression,
    rows: &[Bindings],
    ctx: &TemporalContext,
) -> Result<Option<Vec<Bindings>>> {
    if ctx.valid_at.is_some()
        || ctx.as_of_tx.is_some()
        || ctx.entails_rdfs
        || ctx.string_narrows.is_some()
        || ctx.row_limit.is_some()
        || !matches!(&ctx.graph, super::GraphScope::Named(ids) if !ids.is_empty() && ids.iter().all(|id| *id > 0))
        || ctx.row_cap.is_some_and(|cap| cap < 50_001)
        || rows.len() > 16_384
    {
        return Ok(None);
    }
    let Expression::Not(negated) = expr else {
        return Ok(None);
    };
    let Expression::Exists(inner) = negated.as_ref() else {
        return Ok(None);
    };
    let GraphPattern::Filter { expr: guard, inner } = inner.as_ref() else {
        return Ok(None);
    };
    let GraphPattern::Bgp { patterns } = inner.as_ref() else {
        return Ok(None);
    };
    let [pattern] = patterns.as_slice() else {
        return Ok(None);
    };
    let (
        TermPattern::Variable(subject),
        NamedNodePattern::NamedNode(_),
        TermPattern::Variable(value),
    ) = (&pattern.subject, &pattern.predicate, &pattern.object)
    else {
        return Ok(None);
    };
    if subject == value
        || !super::status_pushdown::plain_guard(guard, value.as_str())
        || rows.iter().any(|row| row.contains_key(value.as_str()))
    {
        return Ok(None);
    }
    let mut unique = HashSet::new();
    let mut groups = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        check_eval_budget(ctx, i, 0)?;
        let Some(Value::Ref(id)) = row.get(subject.as_str()) else {
            return Ok(None);
        };
        if *id <= 0 {
            return Ok(None);
        }
        if !unique.insert(*id) {
            continue;
        }
        let aliases = store.lookup_all(&store.resolve(*id)?)?;
        if aliases.is_empty() || aliases.len() > 128 {
            return Ok(None);
        }
        groups.push(aliases);
    }
    let mut present = HashSet::new();
    for (i, chunk) in groups.chunks(128).enumerate() {
        check_eval_budget(ctx, i, present.len())?;
        let ids: Vec<_> = chunk.iter().flatten().copied().collect();
        let Some(properties) = super::batch_star::speculative(subject_relation(
            store,
            pattern,
            ctx,
            &ids,
            50_001,
            super::batch_star::MAX_BYTES,
        ))?
        else {
            return Ok(None);
        };
        if properties.len() > 50_000 {
            return Ok(None);
        }
        for (index, property) in properties.iter().enumerate() {
            check_eval_budget(ctx, index, present.len())?;
            if super::filter::eval_filter(store, guard, property, ctx)?
                && let Some(Value::Ref(id)) = property.get(subject.as_str())
            {
                present.insert(*id);
            }
        }
    }
    Ok(Some(
        rows.iter()
            .filter(|row| match row.get(subject.as_str()) {
                Some(Value::Ref(id)) => !present.contains(id),
                _ => false,
            })
            .cloned()
            .collect(),
    ))
}
