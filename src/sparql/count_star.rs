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

/// Every buffered subject's slots, flat: one value arena and, per subject,
/// where each of its `width` slots starts. No allocation per subject.
struct Buffered {
    width: usize,
    subjects: Vec<i64>,
    starts: Vec<usize>,
    values: Vec<Value>,
}

impl Buffered {
    fn push(&mut self, subject: i64, slots: &mut [Vec<Value>]) {
        self.subjects.push(subject);
        for slot in slots {
            self.starts.push(self.values.len());
            self.values.append(slot);
        }
    }

    /// Subject `k`'s slot `j`.
    fn slot(&self, k: usize, j: usize) -> &[Value] {
        let i = k * self.width + j;
        let end = self.starts.get(i + 1).copied().unwrap_or(self.values.len());
        &self.values[self.starts[i]..end]
    }
}

/// Put each slot in the per-property scans' order: term-key order, identical
/// values once. `false` when a slot holds two encodings of one term, where
/// SQL keeps a representative of its choosing.
///
/// Rows arrive in index (byte) order and the sort is stable. Two encodings
/// that decode to one value write back identically, and any representative
/// would decode to that same value, so comparing written-back bytes is enough.
/// Single values, the common case, need no key at all.
fn settle(values: &mut [Vec<Value>]) -> bool {
    for slot in values.iter_mut() {
        if slot.len() < 2 {
            continue;
        }
        let mut keyed: Vec<(Vec<u8>, Value)> = slot.drain(..).map(|v| (v.term_key(), v)).collect();
        keyed.sort_by(|a, b| a.0.cmp(&b.0));
        let mut previous: Option<(Vec<u8>, Vec<u8>)> = None;
        for (key, value) in keyed {
            let bytes = value.to_bytes();
            if let Some((last_key, last_bytes)) = &previous
                && *last_key == key
            {
                if *last_bytes == bytes {
                    continue;
                }
                return false;
            }
            previous = Some((key, bytes));
            slot.push(value);
        }
    }
    true
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
    let slot_of: Vec<(i64, usize)> = predicates
        .iter()
        .enumerate()
        .filter_map(|(slot, predicate)| predicate.map(|p| (p, slot)))
        .collect();
    let mut buffered = Buffered {
        width: predicates.len(),
        subjects: Vec::new(),
        starts: Vec::new(),
        values: Vec::new(),
    };
    {
        let mut statement = store.conn.prepare_cached(&sql)?;
        let mut rows = statement.query(rusqlite::params_from_iter(params.iter()))?;
        let mut current: Option<i64> = None;
        let mut typed = false;
        // Reused for every subject; `push` moves the values out.
        let mut values: Vec<Vec<Value>> = vec![Vec::new(); predicates.len()];
        let mut seen = 0usize;
        loop {
            let row = rows.next()?;
            let e = match row {
                Some(row) => Some(row.get::<_, i64>(0)?),
                None => None,
            };
            if let Some(done) = current
                && e != current
            {
                if typed {
                    if !settle(&mut values) {
                        return Ok(false);
                    }
                    buffered.push(done, &mut values);
                } else {
                    values.iter_mut().for_each(Vec::clear);
                }
                typed = false;
                seen = 0;
            }
            let Some(row) = row else { break };
            current = e;
            seen += 1;
            check_eval_budget(ctx, seen, 0)?;
            let a: i64 = row.get(1)?;
            // Borrowed, decoded once into its slot; never copied.
            let rusqlite::types::ValueRef::Blob(v) = row.get_ref(2)? else {
                return Ok(false);
            };
            if a == type_id && v == class.as_slice() {
                typed = true;
            }
            for &(predicate, slot) in &slot_of {
                if predicate == a {
                    values[slot].push(Value::from_bytes(v)?);
                }
            }
        }
    }

    #[cfg(test)]
    PASSES.with(|n| n.set(n.get() + 1));
    let mut emitted = 0usize;
    let mut slots: Vec<&[Value]> = Vec::with_capacity(buffered.width);
    for (k, subject) in buffered.subjects.iter().enumerate() {
        slots.clear();
        slots.extend((0..buffered.width).map(|j| buffered.slot(k, j)));
        let mut row = seed.clone();
        row.insert(star.subject.to_owned(), Value::Ref(*subject));
        match star.required {
            Some((_, object)) => {
                let (values, optional_values) = slots.split_first().expect("required slot");
                for value in *values {
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
                &slots,
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
    values: &[&[Value]],
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
    for value in values[i] {
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
