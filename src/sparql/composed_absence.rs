//! Conservative absence candidates across read-only composed layers.
//!
//! A plain property on the SAME physical subject proves absence false. An
//! alias on a different physical subject can only retain extra candidates;
//! the original BGP and filter still check those with normal term identity.

use std::collections::HashSet;

use spargebra::algebra::GraphPattern;

use super::{Bindings, TemporalContext};
use crate::{Result, Store, Value};

pub(super) fn candidates(
    store: &Store,
    terms: [&str; 3],
    graph: i64,
    inner: &GraphPattern,
    ctx: &TemporalContext,
) -> Result<Option<(Vec<Bindings>, Vec<String>)>> {
    if ctx.row_cap.is_some_and(|cap| cap < 50_001) {
        return Ok(None);
    }
    let [subject, class, property] = terms;
    let types = store.lookup_all(crate::namespace::RDF_TYPE)?;
    let classes = store.lookup_all(class)?;
    let properties = store.lookup_all(property)?;
    if types.is_empty() || classes.is_empty() || properties.is_empty() {
        return Ok(None);
    }
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(graph)];
    let mut add = |values: Vec<Box<dyn rusqlite::types::ToSql>>| {
        let mut slots = Vec::new();
        for value in values {
            slots.push(format!("?{}", params.len() + 1));
            params.push(value);
        }
        slots.join(",")
    };
    let types = add(types
        .into_iter()
        .map(|id| Box::new(id) as Box<dyn rusqlite::types::ToSql>)
        .collect());
    let classes = add(classes
        .into_iter()
        .map(|id| Box::new(Value::Ref(id).to_bytes()) as Box<dyn rusqlite::types::ToSql>)
        .collect());
    let property_count = properties.len();
    let properties = add(properties
        .into_iter()
        .map(|id| Box::new(id) as Box<dyn rusqlite::types::ToSql>)
        .collect());
    let facts = store.facts_source();
    let facts = facts.strip_suffix(" AS facts").unwrap_or(facts);
    // Candidate planning must not collect a whole composed class. Counting a
    // bounded prefix also avoids evaluating the anti-join for a dense class;
    // an oversized probe keeps the original selective BGP.
    let typed = format!(
        "SELECT COUNT(*) FROM (SELECT 1 FROM {facts} AS t
        WHERE t.g=?1 AND t.a IN ({types}) AND t.v IN ({classes})
        AND t.op=1 AND t.valid_to IS NULL LIMIT 50001)"
    );
    // The property parameters are not referenced by this preliminary probe.
    let count: i64 = store.prepare(&typed)?.query_row(
        rusqlite::params_from_iter(
            params[..params.len() - property_count]
                .iter()
                .map(std::convert::AsRef::as_ref),
        ),
        |row| row.get(0),
    )?;
    if count > 50_000 {
        return Ok(None);
    }
    let sql = format!(
        "SELECT DISTINCT t.e FROM {facts} AS t
        WHERE t.g=?1 AND t.a IN ({types}) AND t.v IN ({classes})
        AND t.op=1 AND t.valid_to IS NULL
        AND NOT EXISTS (SELECT 1 FROM {facts} AS p
            WHERE p.g=?1 AND p.e=t.e AND p.a IN ({properties})
            AND p.op=1 AND p.valid_to IS NULL AND quipu_plain_literal(p.v))"
    );
    let mut statement = store.prepare(&sql)?;
    let mut rows = statement.query(rusqlite::params_from_iter(
        params.iter().map(std::convert::AsRef::as_ref),
    ))?;
    let mut ids = Vec::new();
    while let Some(row) = rows.next()? {
        super::pattern_util::check_eval_budget(ctx, ids.len(), ids.len())?;
        ids.push(row.get::<_, i64>(0)?);
    }
    drop(rows);
    drop(statement);
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for (index, id) in ids.into_iter().enumerate() {
        super::pattern_util::check_eval_budget(ctx, index, result.len())?;
        let id = store.canonical_id(id)?;
        if !seen.insert(id) {
            continue;
        }
        let mut seed = Bindings::new();
        seed.insert(subject.to_string(), Value::Ref(id));
        // Recheck EVERY original constraint and multiplicity: candidates are
        // a superset, never a replacement for the composed BGP.
        result.extend(super::pattern::eval_pattern_seeded(store, inner, ctx, &seed)?.0);
    }
    Ok(Some((result, vec![subject.to_string()])))
}
