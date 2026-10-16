//! Collection modes for the shared triple visitor.
use super::{
    Bindings, Result, Store, TemporalContext, TriplePattern, visit_triple_pattern_limited,
};

/// A bounded complete relation for SQL-star's speculative composed plan.
/// Rust canonical deduplication already removes every physical duplicate;
/// avoid sorting the physical relation again before that pass. An overflow
/// restores the ordinary planner, so this order never selects a result prefix.
pub(in crate::sparql) fn eval_complete_relation(
    store: &Store,
    tp: &TriplePattern,
    ctx: &TemporalContext,
    limit: usize,
) -> Result<Vec<Bindings>> {
    collect_triple_pattern(
        store,
        tp,
        &Bindings::new(),
        ctx,
        Some(limit),
        ScanMode::CompleteRelation,
    )
}

#[derive(Clone, Copy)]
pub(super) enum ScanMode<'a> {
    BoundedRelation(Option<&'a [i64]>, usize),
    Retained,
    Streamed,
    CompleteRelation,
}

pub(super) fn collect_triple_pattern(
    store: &Store,
    tp: &TriplePattern,
    bindings: &Bindings,
    ctx: &TemporalContext,
    limit: Option<usize>,
    mode: ScanMode<'_>,
) -> Result<Vec<Bindings>> {
    let mut results = Vec::new();
    let mut bytes = 0usize;
    visit_triple_pattern_limited(
        store,
        tp,
        bindings,
        ctx,
        limit,
        mode,
        Some(&mut |row| {
            if let ScanMode::BoundedRelation(_, limit) = mode {
                bytes = bytes.saturating_add(binding_bytes(&row));
                if bytes > limit {
                    return Err(crate::Error::InvalidValue(
                        "speculative relation byte bound exceeded".into(),
                    ));
                }
            }
            results.push(row);
            Ok(())
        }),
    )?;
    Ok(results)
}

/// Same canonical triple visitor, restricted to a complete physical subject set.
pub(in crate::sparql) fn subject_relation(
    store: &Store,
    tp: &TriplePattern,
    ctx: &TemporalContext,
    ids: &[i64],
    limit: usize,
    bytes: usize,
) -> Result<Vec<Bindings>> {
    collect_triple_pattern(
        store,
        tp,
        &Bindings::new(),
        ctx,
        Some(limit),
        ScanMode::BoundedRelation(Some(ids), bytes),
    )
}

pub(super) fn subject_condition(
    store: &Store,
    tp: &TriplePattern,
    bindings: &Bindings,
    direct_refs: bool,
    mode: ScanMode<'_>,
    params: &mut Vec<Box<dyn rusqlite::types::ToSql>>,
) -> Result<Option<String>> {
    use crate::Value;
    use spargebra::term::TermPattern;
    if let ScanMode::BoundedRelation(Some(ids), _) = mode {
        return Ok(Some(if ids.is_empty() {
            String::new()
        } else {
            super::sql_id_in("e", ids, params)
        }));
    }
    let subject_ref = match &tp.subject {
        TermPattern::Variable(v) => bindings.get(v.as_str()),
        TermPattern::BlankNode(v) => bindings.get(v.as_str()),
        _ => None,
    };
    if direct_refs
        && let Some(Value::Ref(id)) = subject_ref
        && *id >= 0
    {
        let condition = format!("e = ?{}", params.len() + 1);
        params.push(Box::new(*id));
        return Ok(Some(condition));
    }
    if let Some(iri) = super::resolve_subject_pattern(store, &tp.subject, bindings)? {
        let ids = store.lookup_all(&iri)?;
        return Ok(Some(if ids.is_empty() {
            String::new()
        } else {
            super::sql_id_in("e", &ids, params)
        }));
    }
    Ok(None)
}

/// Byte-accounted complete relation for a speculative driving pattern.
pub(in crate::sparql) fn selected_relation(
    store: &Store,
    tp: &TriplePattern,
    ctx: &TemporalContext,
    limit: usize,
    bytes: usize,
) -> Result<Vec<Bindings>> {
    collect_triple_pattern(
        store,
        tp,
        &Bindings::new(),
        ctx,
        Some(limit),
        ScanMode::BoundedRelation(None, bytes),
    )
}

/// Conservative retained-binding accounting; not a total-RSS measurement.
pub(in crate::sparql) fn binding_bytes(row: &Bindings) -> usize {
    use crate::Value;
    row.iter().fold(
        row.capacity().saturating_add(1).saturating_mul(256),
        |sum, (key, value)| {
            let payload = match value {
                Value::Str(s) => s.capacity(),
                Value::Bytes(b) => b.capacity(),
                Value::Lang { lexical, lang } => lexical.capacity().saturating_add(lang.capacity()),
                Value::Typed { lexical, datatype } => {
                    lexical.capacity().saturating_add(datatype.capacity())
                }
                _ => 0,
            };
            sum.saturating_add(key.capacity().saturating_add(payload).saturating_mul(2))
        },
    )
}
