//! Reject known plain property values before expanding a same-subject star.
//!
//! The owner/status discovery compatibility arm joins every owner's status
//! before discarding its known values. A covering predicate scan can instead
//! find subjects with a value not proven excluded. This is only a candidate
//! filter: the ordinary BGP and full FILTER still decide each subject, so
//! language/typed literals, mixed values and bag multiplicity retain their
//! existing meaning. Missing values belong to the separate absence arm.

use spargebra::algebra::{Expression, Function, GraphPattern};
use spargebra::term::{NamedNodePattern, TermPattern};

use super::{Bindings, GraphScope, TemporalContext};
use crate::{Result, Store, Value};

const MAX_SUBJECTS: usize = 128;

fn excluded(expr: &Expression) -> Option<(&str, Vec<&str>)> {
    let Expression::And(literal, not) = expr else {
        return None;
    };
    let Expression::FunctionCall(Function::IsLiteral, args) = literal.as_ref() else {
        return None;
    };
    let [Expression::Variable(variable)] = args.as_slice() else {
        return None;
    };
    let Expression::Not(not) = not.as_ref() else {
        return None;
    };
    let Expression::In(value, choices) = not.as_ref() else {
        return None;
    };
    let Expression::FunctionCall(Function::Str, args) = value.as_ref() else {
        return None;
    };
    if args.as_slice() != [Expression::Variable(variable.clone())]
        || choices.is_empty()
        || choices.len() > 32
    {
        return None;
    }
    let mut strings = Vec::new();
    for choice in choices {
        let Expression::Literal(value) = choice else {
            return None;
        };
        if value.language().is_some()
            || value.datatype().as_str() != "http://www.w3.org/2001/XMLSchema#string"
        {
            return None;
        }
        strings.push(value.value());
    }
    Some((variable.as_str(), strings))
}

pub(super) fn candidates(
    store: &Store,
    expr: &Expression,
    inner: &GraphPattern,
    ctx: &TemporalContext,
    seed: &Bindings,
) -> Result<Option<(Vec<Bindings>, Vec<String>)>> {
    #[cfg(test)]
    if tests::disabled() {
        return Ok(None);
    }
    if !seed.is_empty()
        || store.has_attachments()
        || ctx.valid_at.is_some()
        || ctx.as_of_tx.is_some()
        || ctx.row_limit.is_some()
        || ctx.entails_rdfs
        || ctx.string_narrows.is_some()
    {
        return Ok(None);
    }
    let GraphScope::Named(graphs) = &ctx.graph else {
        return Ok(None);
    };
    let [graph] = graphs.as_slice() else {
        return Ok(None);
    };
    if *graph <= 0 {
        return Ok(None);
    }
    let Some((variable, choices)) = excluded(expr) else {
        return Ok(None);
    };
    let GraphPattern::Bgp { patterns } = inner else {
        return Ok(None);
    };
    if !(2..=8).contains(&patterns.len()) {
        return Ok(None);
    }
    let Some(property) = patterns
        .iter()
        .find(|p| matches!(&p.object, TermPattern::Variable(v) if v.as_str() == variable))
    else {
        return Ok(None);
    };
    let TermPattern::Variable(subject) = &property.subject else {
        return Ok(None);
    };
    if subject.as_str() == variable
        || patterns.iter().any(|p| {
            p.subject != property.subject
                || !matches!(p.predicate, NamedNodePattern::NamedNode(_))
                || !(std::ptr::eq(p, property)
                    || matches!(
                        p.object,
                        TermPattern::Literal(_) | TermPattern::NamedNode(_)
                    ))
        })
    {
        return Ok(None);
    }
    let NamedNodePattern::NamedNode(predicate) = &property.predicate else {
        return Ok(None);
    };
    // A global property scan must not replace a cheap, selective owner lookup.
    // Prove a non-type constant anchor is already large using a bounded ordinary
    // lookup; small/empty owners retain their existing plan.
    let Some(anchor) = patterns.iter().find(|p| {
        !std::ptr::eq(*p, property)
            && matches!(&p.predicate, NamedNodePattern::NamedNode(n)
                if n.as_str() != crate::namespace::RDF_TYPE)
    }) else {
        return Ok(None);
    };
    if super::triple::eval_triple_pattern_limited(
        store,
        anchor,
        seed,
        ctx,
        Some(super::filter_drive::MIN_DRIVER_ROWS + 1),
    )?
    .len()
        <= super::filter_drive::MIN_DRIVER_ROWS
    {
        return Ok(None);
    }
    let ids = store.lookup_all(predicate.as_str())?;
    let [predicate] = ids.as_slice() else {
        return Ok(None);
    };
    let indexes: i64 = store.conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name IN ('idx_current_aev','idx_current_g')", [], |r|r.get(0))?;
    if indexes != 2 {
        return Ok(None);
    }
    let mut parameters: Vec<Box<dyn rusqlite::types::ToSql>> =
        vec![Box::new(*predicate), Box::new(*graph)];
    let mut placeholders = Vec::new();
    for choice in choices {
        // Only bytes whose RDF term is certainly excluded are removed. Other
        // encodings and literal kinds survive for the full expression.
        for value in store.literal_aliases(&Value::Str(choice.into()), false)? {
            placeholders.push(format!("?{}", parameters.len() + 1));
            parameters.push(Box::new(value));
        }
    }
    let sql = format!(
        "SELECT DISTINCT p.e FROM facts AS p INDEXED BY idx_current_aev
        WHERE p.a=?1 AND p.op=1 AND p.valid_to IS NULL AND p.v NOT IN ({})
        AND EXISTS (SELECT 1 FROM facts AS gp INDEXED BY idx_current_g
            WHERE gp.g=?2 AND gp.rowid=p.rowid AND gp.op=1 AND gp.valid_to IS NULL)
        LIMIT {}",
        placeholders.join(","),
        MAX_SUBJECTS + 1
    );
    let mut statement = store.prepare(&sql)?;
    let mut cursor = statement.query(rusqlite::params_from_iter(
        parameters.iter().map(std::convert::AsRef::as_ref),
    ))?;
    let mut subjects = Vec::new();
    while let Some(row) = cursor.next()? {
        super::pattern_util::check_eval_budget(ctx, subjects.len(), 0)?;
        let subject = row.get::<_, i64>(0)?;
        // A seed would canonicalize legacy dictionary aliases. Repeating it
        // once per physical alias could multiply the original solution bag.
        if subject <= 0 {
            return Ok(None);
        }
        let name = match store.resolve(subject) {
            Ok(name) => name,
            // A speculative candidate outside the owner relation must not
            // introduce a decoder error the ordinary BGP never encounters.
            Err(crate::Error::UnknownTerm(_) | crate::Error::InvalidValue(_)) => return Ok(None),
            Err(error) => return Err(error),
        };
        if store.lookup_all(&name)?.len() != 1 {
            return Ok(None);
        }
        subjects.push(subject);
        if subjects.len() > MAX_SUBJECTS {
            return Ok(None);
        }
    }
    #[cfg(test)]
    tests::hit();
    let variables = patterns
        .iter()
        .flat_map(super::pattern_util::triple_pattern_vars)
        .fold(Vec::new(), |mut vars, var| {
            if !vars.contains(&var) {
                vars.push(var)
            };
            vars
        });
    let mut rows = Vec::new();
    for (i, id) in subjects.into_iter().enumerate() {
        super::pattern_util::check_eval_budget(ctx, i, rows.len())?;
        let mut bound = Bindings::new();
        bound.insert(subject.as_str().into(), Value::Ref(id));
        rows.extend(super::triple::eval_bgp(store, patterns, ctx, &bound)?.0);
    }
    Ok(Some((rows, variables)))
}

#[cfg(test)]
mod tests;
