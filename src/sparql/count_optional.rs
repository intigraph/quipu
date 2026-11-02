//! Narrow streaming OPTIONAL admission; all other left joins keep the old path.
use super::filter::eval_filter;
use super::pattern_util::{check_eval_budget, triple_pattern_vars};
use super::{Bindings, GraphScope, TemporalContext};
use crate::{Result, Store, Value};
use spargebra::algebra::{Expression, Function, GraphPattern};
use spargebra::term::{NamedNodePattern, TermPattern};

fn local(expr: &Expression, subject: &str, object: &str) -> bool {
    match expr {
        Expression::Variable(v) => v.as_str() == subject || v.as_str() == object,
        Expression::Literal(_) | Expression::NamedNode(_) => true,
        Expression::Add(a, b)
        | Expression::Subtract(a, b)
        | Expression::Multiply(a, b)
        | Expression::Divide(a, b)
        | Expression::Equal(a, b)
        | Expression::SameTerm(a, b)
        | Expression::Less(a, b)
        | Expression::LessOrEqual(a, b)
        | Expression::Greater(a, b)
        | Expression::GreaterOrEqual(a, b)
        | Expression::And(a, b)
        | Expression::Or(a, b) => local(a, subject, object) && local(b, subject, object),
        Expression::Not(a) | Expression::UnaryPlus(a) | Expression::UnaryMinus(a) => {
            local(a, subject, object)
        }
        Expression::Bound(v) => v.as_str() == subject || v.as_str() == object,
        Expression::If(a, b, c) => {
            local(a, subject, object) && local(b, subject, object) && local(c, subject, object)
        }
        Expression::In(a, values) => {
            local(a, subject, object) && values.iter().all(|a| local(a, subject, object))
        }
        Expression::Coalesce(values) => values.iter().all(|a| local(a, subject, object)),
        Expression::FunctionCall(function, args) if pure(function) => {
            args.iter().all(|a| local(a, subject, object))
        }
        _ => false,
    }
}
// Explicit deterministic builtins: volatile, custom and future functions
// remain unsupported rather than acquiring substitution semantics silently.
fn pure(function: &Function) -> bool {
    matches!(
        function,
        Function::Str
            | Function::Lang
            | Function::LangMatches
            | Function::Datatype
            | Function::Iri
            | Function::Abs
            | Function::Ceil
            | Function::Floor
            | Function::Round
            | Function::Concat
            | Function::SubStr
            | Function::StrLen
            | Function::Replace
            | Function::UCase
            | Function::LCase
            | Function::EncodeForUri
            | Function::Contains
            | Function::StrStarts
            | Function::StrEnds
            | Function::StrBefore
            | Function::StrAfter
            | Function::Year
            | Function::Month
            | Function::Day
            | Function::Hours
            | Function::Minutes
            | Function::Seconds
            | Function::Timezone
            | Function::Tz
            | Function::Md5
            | Function::Sha1
            | Function::Sha256
            | Function::Sha384
            | Function::Sha512
            | Function::StrLang
            | Function::StrDt
            | Function::IsIri
            | Function::IsBlank
            | Function::IsLiteral
            | Function::IsNumeric
            | Function::Regex
    )
}
fn binds(pattern: &GraphPattern, variable: &str, mandatory: bool) -> bool {
    match pattern {
        GraphPattern::Bgp { patterns } => patterns.iter().any(|p| {
            if mandatory {
                matches!(&p.subject,TermPattern::Variable(v) if v.as_str()==variable)
            } else {
                triple_pattern_vars(p).iter().any(|v| v == variable)
            }
        }),
        GraphPattern::Graph { inner, .. } | GraphPattern::Filter { inner, .. } => {
            binds(inner, variable, mandatory)
        }
        GraphPattern::Extend {
            inner, variable: v, ..
        } => {
            if mandatory && v.as_str() == variable {
                false
            } else {
                (!mandatory && v.as_str() == variable) || binds(inner, variable, mandatory)
            }
        }
        GraphPattern::Join { left, right } => {
            binds(left, variable, mandatory) || binds(right, variable, mandatory)
        }
        GraphPattern::Union { left, right } => {
            if mandatory {
                binds(left, variable, true) && binds(right, variable, true)
            } else {
                binds(left, variable, false) || binds(right, variable, false)
            }
        }
        GraphPattern::LeftJoin { left, right, .. } => {
            binds(left, variable, mandatory) || (!mandatory && binds(right, variable, false))
        }
        _ => false,
    }
}
fn shape<'a>(
    left: &GraphPattern,
    right: &'a GraphPattern,
    expression: Option<&Expression>,
) -> Option<(&'a str, &'a str)> {
    // A join-condition FILTER sees compatible rows, just as the old left
    // join does. An embedded right FILTER is evaluated independently and can
    // fail on unrelated rows; retain that error path by declining it. A plain
    // OPTIONAL has no condition at all (aegis-xdvg.6).
    let GraphPattern::Bgp { patterns } = right else {
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
    let (subject, object) = (subject.as_str(), object.as_str());
    if subject == object
        || predicate.as_str() == crate::namespace::RDF_TYPE
        || !binds(left, subject, true)
        || binds(left, object, false)
        || expression.is_some_and(|e| !local(e, subject, object))
    {
        return None;
    }
    Some((subject, object))
}
pub(super) fn recognized(
    left: &GraphPattern,
    right: &GraphPattern,
    expression: Option<&Expression>,
) -> bool {
    shape(left, right, expression).is_some()
}
/// Preflight the complete tree before any input is visited: no partial fallback.
pub(super) fn allowed(
    store: &Store,
    pattern: &GraphPattern,
    ctx: &TemporalContext,
    seed: &Bindings,
) -> Result<bool> {
    match pattern {
        GraphPattern::Graph {
            name: NamedNodePattern::NamedNode(name),
            inner,
        } => allowed(
            store,
            inner,
            &super::count_stream::graph_context(store, name.as_str(), ctx)?,
            seed,
        ),
        GraphPattern::Filter { inner, .. } | GraphPattern::Extend { inner, .. } => {
            allowed(store, inner, ctx, seed)
        }
        GraphPattern::Union { left, right } | GraphPattern::Join { left, right } => {
            Ok(allowed(store, left, ctx, seed)? && allowed(store, right, ctx, seed)?)
        }
        GraphPattern::LeftJoin {
            left,
            right,
            expression,
        } => {
            let Some((_, object)) = shape(left, right, expression.as_ref()) else {
                return Ok(false);
            };
            let GraphScope::Named(graphs) = &ctx.graph else {
                return Ok(false);
            };
            if graphs.len() != 1
                || graphs[0] == 0
                || store.has_attachments()
                || ctx.valid_at.is_some()
                || ctx.as_of_tx.is_some()
                || seed.contains_key(object)
            {
                return Ok(false);
            }
            allowed(store, left, ctx, seed)
        }
        _ => Ok(true),
    }
}
/// Parents probed one by one before a property is read whole. Tests read it
/// whole from the second parent, so every exact comparison covers both paths.
#[cfg(not(test))]
const PROBE_PARENTS: usize = 64;
#[cfg(test)]
const PROBE_PARENTS: usize = 1;
/// The most property rows held for probing; larger properties keep per-parent
/// probes.
const PROBE_ROWS: usize = 1 << 20;

/// One OPTIONAL's property read whole, once enough parents asked for it.
///
/// Every parent otherwise issues its own indexed read of `?s <p> ?o`: a board
/// aggregate with seven OPTIONALs over ~70k items made ~500k of them, the
/// largest remaining cost of its ~4.4s (aegis-xdvg.6). The admitted shape is
/// a single current named graph without attachments, so subjects are their
/// own canonical ids and a whole read keeps each subject's objects in the
/// order its own read returns them. Anything else (a failed read, a subject
/// that is not a reference, too many rows) keeps per-parent probes, which
/// report any real failure themselves.
#[derive(Default)]
pub(super) struct Probe {
    parents: usize,
    table: Option<Option<std::collections::HashMap<i64, Vec<Value>>>>,
}

fn whole(
    store: &Store,
    right: &GraphPattern,
    ctx: &TemporalContext,
) -> Option<std::collections::HashMap<i64, Vec<Value>>> {
    let GraphPattern::Bgp { patterns } = right else {
        return None;
    };
    let [tp] = patterns.as_slice() else {
        return None;
    };
    let (TermPattern::Variable(subject), TermPattern::Variable(object)) = (&tp.subject, &tp.object)
    else {
        return None;
    };
    let mut table: std::collections::HashMap<i64, Vec<Value>> = std::collections::HashMap::new();
    let mut rows = 0usize;
    let mut admitted = true;
    super::count_stream::visit(store, right, ctx, &Bindings::new(), &mut |mut row| {
        rows += 1;
        match (row.remove(subject.as_str()), row.remove(object.as_str())) {
            (Some(Value::Ref(id)), Some(value)) if rows <= PROBE_ROWS => {
                table.entry(id).or_default().push(value);
                Ok(())
            }
            _ => {
                admitted = false;
                Err(crate::Error::InvalidValue("OPTIONAL probe declined".into()))
            }
        }
    })
    .ok()?;
    admitted.then_some(table)
}

/// Match one parent's fresh property, retaining that parent once when unmatched.
#[allow(clippy::too_many_arguments)]
pub(super) fn visit(
    store: &Store,
    right: &GraphPattern,
    expression: Option<&Expression>,
    ctx: &TemporalContext,
    parent: &Bindings,
    probe: &mut Probe,
    emitted: &mut usize,
    emit: &mut dyn FnMut(Bindings) -> Result<()>,
) -> Result<()> {
    probe.parents += 1;
    if probe.table.is_none() && probe.parents > PROBE_PARENTS {
        probe.table = Some(whole(store, right, ctx));
    }
    let mut matched = false;
    let mut accept = |row: Bindings| -> Result<()> {
        if match expression {
            Some(e) => eval_filter(store, e, &row, ctx)?,
            None => true,
        } {
            check_eval_budget(ctx, *emitted, *emitted)?;
            *emitted += 1;
            matched = true;
            emit(row)?;
        }
        Ok(())
    };
    let probed = match (&probe.table, right) {
        (Some(Some(table)), GraphPattern::Bgp { patterns }) => match patterns.as_slice() {
            [tp] => match (&tp.subject, &tp.object) {
                (TermPattern::Variable(subject), TermPattern::Variable(object)) => {
                    match parent.get(subject.as_str()) {
                        Some(Value::Ref(id)) => {
                            for value in table.get(id).map(Vec::as_slice).unwrap_or_default() {
                                let mut row = parent.clone();
                                row.insert(object.as_str().to_owned(), value.clone());
                                accept(row)?;
                            }
                            true
                        }
                        _ => false,
                    }
                }
                _ => false,
            },
            _ => false,
        },
        _ => false,
    };
    if !probed {
        super::count_stream::visit(store, right, ctx, parent, &mut accept)?;
    }
    if !matched {
        check_eval_budget(ctx, *emitted, *emitted)?;
        *emitted += 1;
        emit(parent.clone())?;
    }
    Ok(())
}
