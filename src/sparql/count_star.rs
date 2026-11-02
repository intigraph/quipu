//! A typed subject with single-pattern OPTIONALs, read in one ordered pass
//! (aegis-xdvg.6).
//!
//! `GRAPH <g> { ?s a <T> . OPTIONAL { ?s <p1> ?o1 } ... }` used to cost one
//! scan for the type and one per OPTIONAL, each walking an index and looking
//! up every row in the table, then sorting for its GROUP BY: 69% of the 2fa
//! stats aggregate's star was SQLite reads. The partial index
//! `idx_current_gea (g, e, a, v) WHERE op = 1 AND valid_to IS NULL` already
//! holds every current fact of a graph in subject order, so one covering scan
//! fills each subject's slots: is it a `<T>`, and which objects does each
//! OPTIONAL see.
//!
//! The rows are exactly those of the nested left joins: subjects in ascending
//! id (the order the type scan emits), each OPTIONAL's objects in term-key
//! order with identical values once (the order and dedup of the per-property
//! scans' `GROUP BY e, a, quipu_term_key(v)`), the first OPTIONAL outermost,
//! and a row kept once when an OPTIONAL matches nothing. A subject holding two
//! DIFFERENT encodings of one term (`"1"^^xsd:integer` and `"01"^^xsd:integer`)
//! is where SQL keeps one representative of its choosing; the pass declines
//! before emitting anything and the existing scans run instead.
//!
//! Admitted only where the existing streaming OPTIONAL path is: one named
//! graph at the current time, no attachments, no inference or string narrows,
//! nothing pre-bound.

use super::filter::eval_filter;
use super::pattern_util::check_eval_budget;
use super::{Bindings, GraphScope, TemporalContext};
use crate::{Result, Store, Value};
use rusqlite::OptionalExtension;
use spargebra::algebra::{Expression, GraphPattern};
use spargebra::term::{NamedNodePattern, TermPattern};

#[cfg(test)]
thread_local! {
    /// How many stars took the single pass (tests prove the path ran).
    pub(super) static PASSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

struct Optional<'a> {
    predicate: &'a str,
    object: &'a str,
    expression: Option<&'a Expression>,
}

struct Star<'a> {
    subject: &'a str,
    class: &'a str,
    /// At most one `?s <p> ?o` beside the type in the base BGP: every value
    /// is a row and a subject without one has none (an inner join).
    required: Option<(&'a str, &'a str)>,
    optionals: Vec<Optional<'a>>,
}

fn shape(pattern: &GraphPattern) -> Option<Star<'_>> {
    match pattern {
        GraphPattern::Bgp { patterns } => {
            // The SQL BGP keeps written order when a type pattern is present;
            // with one other pattern either order yields subjects ascending
            // and that pattern's values in term-key order.
            let (mut class, mut required, mut subject_var) = (None, None, None);
            if patterns.is_empty() || patterns.len() > 2 {
                return None;
            }
            for tp in patterns {
                let (TermPattern::Variable(subject), NamedNodePattern::NamedNode(predicate)) =
                    (&tp.subject, &tp.predicate)
                else {
                    return None;
                };
                if subject_var.is_some_and(|s: &str| s != subject.as_str()) {
                    return None;
                }
                subject_var = Some(subject.as_str());
                match &tp.object {
                    TermPattern::NamedNode(c)
                        if predicate.as_str() == crate::namespace::RDF_TYPE && class.is_none() =>
                    {
                        class = Some(c.as_str());
                    }
                    TermPattern::Variable(o)
                        if predicate.as_str() != crate::namespace::RDF_TYPE
                            && o.as_str() != subject.as_str()
                            && required.is_none() =>
                    {
                        required = Some((predicate.as_str(), o.as_str()));
                    }
                    _ => return None,
                }
            }
            Some(Star {
                subject: subject_var?,
                class: class?,
                required,
                optionals: Vec::new(),
            })
        }
        GraphPattern::LeftJoin {
            left,
            right,
            expression,
        } => {
            let mut star = shape(left)?;
            let GraphPattern::Bgp { patterns } = &**right else {
                return None;
            };
            let [tp] = patterns.as_slice() else {
                return None;
            };
            let (
                TermPattern::Variable(subject),
                NamedNodePattern::NamedNode(predicate),
                TermPattern::Variable(object),
            ) = (&tp.subject, &tp.predicate, &tp.object)
            else {
                return None;
            };
            let object = object.as_str();
            if subject.as_str() != star.subject
                || object == star.subject
                || predicate.as_str() == crate::namespace::RDF_TYPE
                || star.optionals.iter().any(|o| o.object == object)
                || star.required.is_some_and(|(_, r)| r == object)
            {
                return None;
            }
            star.optionals.push(Optional {
                predicate: predicate.as_str(),
                object,
                expression: expression.as_ref(),
            });
            Some(star)
        }
        _ => None,
    }
}

fn covering_index(store: &Store) -> Result<bool> {
    let definition: Option<String> = store
        .conn
        .query_row(
            "SELECT sql FROM main.sqlite_master WHERE type='index' AND name='idx_current_gea'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(definition.is_some_and(|sql| {
        sql.chars()
            .filter(|c| !c.is_ascii_whitespace())
            .flat_map(char::to_lowercase)
            .collect::<String>()
            == "createindexidx_current_geaonfacts(g,e,a,v)whereop=1andvalid_toisnull"
    }))
}

/// One subject's objects per OPTIONAL, each in term-key order.
struct Slots {
    subject: i64,
    values: Vec<Vec<Value>>,
}

/// Read the subject's raw `(a, v)` rows into per-OPTIONAL slots. `None` when
/// a slot holds two encodings of one term.
fn fill(
    rows: &mut Vec<(i64, Vec<u8>)>,
    predicates: &[Option<i64>],
    type_id: i64,
    class: &[u8],
) -> Result<Option<Option<Vec<Vec<Value>>>>> {
    let typed = rows
        .iter()
        .any(|(a, v)| *a == type_id && v.as_slice() == class);
    if !typed {
        rows.clear();
        return Ok(Some(None));
    }
    let mut slots = Vec::with_capacity(predicates.len());
    for predicate in predicates {
        let mut keyed: Vec<(Vec<u8>, &[u8])> = Vec::new();
        if let Some(predicate) = predicate {
            for (a, v) in rows.iter() {
                if a == predicate {
                    keyed.push((Value::from_bytes(v)?.term_key(), v.as_slice()));
                }
            }
        }
        // Stable: equal keys keep index (byte) order, so identical bytes sit
        // together and are kept once.
        keyed.sort_by(|a, b| a.0.cmp(&b.0));
        let mut values = Vec::with_capacity(keyed.len());
        for (i, (key, bytes)) in keyed.iter().enumerate() {
            if i > 0 && keyed[i - 1].0 == *key {
                if keyed[i - 1].1 == *bytes {
                    continue;
                }
                return Ok(None);
            }
            values.push(Value::from_bytes(bytes)?);
        }
        slots.push(values);
    }
    rows.clear();
    Ok(Some(Some(slots)))
}

/// Stream the star's rows if its shape and store admit the single pass;
/// `Ok(false)` (nothing emitted) otherwise.
pub(super) fn try_visit(
    store: &Store,
    pattern: &GraphPattern,
    ctx: &TemporalContext,
    seed: &Bindings,
    emit: super::count_stream::Emit<'_>,
) -> Result<bool> {
    let Some(star) = shape(pattern) else {
        return Ok(false);
    };
    let GraphScope::Named(graphs) = &ctx.graph else {
        return Ok(false);
    };
    let [graph] = graphs.as_slice() else {
        return Ok(false);
    };
    if *graph <= 0
        || !seed.is_empty()
        || store.has_attachments()
        || ctx.valid_at.is_some()
        || ctx.as_of_tx.is_some()
        || ctx.string_narrows.is_some()
        || ctx.entails_rdfs
        || !covering_index(store)?
    {
        return Ok(false);
    }
    let (Some(type_id), Some(class_id)) = (
        store.lookup(crate::namespace::RDF_TYPE)?,
        store.lookup(star.class)?,
    ) else {
        // No such class or no type facts: the type scan finds nothing either.
        #[cfg(test)]
        PASSES.with(|n| n.set(n.get() + 1));
        return Ok(true);
    };
    let class = Value::Ref(class_id).to_bytes();
    // Slot 0 is the required pattern's, when there is one.
    let predicates = star
        .required
        .iter()
        .map(|(p, _)| *p)
        .chain(star.optionals.iter().map(|o| o.predicate))
        .map(|p| store.lookup(p))
        .collect::<Result<Vec<_>>>()?;
    let mut wanted: Vec<i64> = predicates.iter().flatten().copied().collect();
    wanted.push(type_id);
    wanted.sort_unstable();
    wanted.dedup();
    let placeholders = (0..wanted.len())
        .map(|i| format!("?{}", i + 2))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT e, a, v FROM facts INDEXED BY idx_current_gea \
         WHERE g = ?1 AND op = 1 AND valid_to IS NULL AND a IN ({placeholders})"
    );
    let mut params: Vec<i64> = vec![*graph];
    params.extend(&wanted);

    // Read every subject before emitting any, so a decline emits nothing.
    let mut subjects: Vec<Slots> = Vec::new();
    {
        let mut statement = store.conn.prepare_cached(&sql)?;
        let mut rows = statement.query(rusqlite::params_from_iter(params.iter()))?;
        let mut current: Option<i64> = None;
        let mut pending: Vec<(i64, Vec<u8>)> = Vec::new();
        while let Some(row) = rows.next()? {
            let e: i64 = row.get(0)?;
            if current.is_some_and(|c| c != e) {
                match fill(&mut pending, &predicates, type_id, &class)? {
                    None => return Ok(false),
                    Some(Some(values)) => subjects.push(Slots {
                        subject: current.expect("checked"),
                        values,
                    }),
                    Some(None) => {}
                }
            }
            current = Some(e);
            pending.push((row.get(1)?, row.get(2)?));
            check_eval_budget(ctx, pending.len(), 0)?;
        }
        if let Some(e) = current {
            match fill(&mut pending, &predicates, type_id, &class)? {
                None => return Ok(false),
                Some(Some(values)) => subjects.push(Slots { subject: e, values }),
                Some(None) => {}
            }
        }
    }

    #[cfg(test)]
    PASSES.with(|n| n.set(n.get() + 1));
    let mut emitted = 0usize;
    for slots in subjects {
        let mut row = seed.clone();
        row.insert(star.subject.to_owned(), Value::Ref(slots.subject));
        match star.required {
            Some((_, object)) => {
                let (values, optional_values) = slots.values.split_first().expect("required slot");
                for value in values {
                    row.insert(object.to_owned(), value.clone());
                    expand(
                        store,
                        ctx,
                        &star.optionals,
                        optional_values,
                        0,
                        &mut row,
                        &mut emitted,
                        emit,
                    )?;
                }
            }
            None => expand(
                store,
                ctx,
                &star.optionals,
                &slots.values,
                0,
                &mut row,
                &mut emitted,
                emit,
            )?,
        }
    }
    Ok(true)
}

/// The rows of OPTIONALs `i..` over `row`, in nested left-join order.
#[allow(clippy::too_many_arguments)]
fn expand(
    store: &Store,
    ctx: &TemporalContext,
    optionals: &[Optional<'_>],
    values: &[Vec<Value>],
    i: usize,
    row: &mut Bindings,
    emitted: &mut usize,
    emit: super::count_stream::Emit<'_>,
) -> Result<()> {
    let Some(optional) = optionals.get(i) else {
        check_eval_budget(ctx, *emitted, *emitted)?;
        *emitted += 1;
        return emit(row);
    };
    let mut matched = false;
    for value in &values[i] {
        row.insert(optional.object.to_owned(), value.clone());
        let keep = match optional.expression {
            Some(e) => eval_filter(store, e, row, ctx)?,
            None => true,
        };
        if keep {
            matched = true;
            expand(store, ctx, optionals, values, i + 1, row, emitted, emit)?;
        }
        row.remove(optional.object);
    }
    if !matched {
        expand(store, ctx, optionals, values, i + 1, row, emitted, emit)?;
    }
    Ok(())
}
