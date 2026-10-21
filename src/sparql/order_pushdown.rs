//! ORDER BY + LIMIT below OPTIONAL hydration (aegis-xacr).
//!
//! A panel that lists the newest closed items reads as
//!
//! ```sparql
//! GRAPH <g> { ?item q:outcome "done" ; s:endTime ?end .
//!             FILTER(?end >= T)
//!             OPTIONAL { ?item s:name ?title } OPTIONAL { ?item s:result ?r } }
//! ORDER BY DESC(?end) LIMIT 101
//! ```
//!
//! SPARQL puts the FILTER above both OPTIONALs, so every one of the ~45k done
//! items was hydrated before the filter and the limit dropped all but 101
//! (0.89 s on the production store; the same answer in 0.43 s with the
//! selection written first).
//!
//! When the order keys and the filter read only variables the left BGP always
//! binds, the selection can run first and only its first `k` rows are hydrated:
//!
//! - `Filter(e, LeftJoin(L, R))` equals `LeftJoin(Filter(e, L), R)`: an
//!   OPTIONAL never changes a variable `L` already bound, so `e` sees the same
//!   values on the joined row as on the `L` row. `EXISTS` sees the whole row
//!   and is never moved.
//! - Every `L` row yields at least one joined row carrying its keys, and the
//!   sort is stable, so the first `k` joined rows always come from the first
//!   `k` rows of the stably sorted `L`, in the same order. Ties need nothing
//!   extra: tied `L` rows keep their original order on both sides.
//!
//! Both hold only when the comparator is a total order over the key values.
//! Mixed kinds are not: an integer and a string compare `Equal`, so `10 = "x"`
//! and `"x" = 1` while `1 < 10`, and a merge sort's output then depends on the
//! arrangement of its input (franklin's counterexample on #700). So while the
//! rewritten pattern runs, the selection's `ORDER BY` checks that every key's
//! values share one comparison class (exact numeric, float, double,
//! lexical, boolean, IRI); if any does not, the result is
//! discarded and the original pattern is evaluated instead.
//!
//! The rewritten pattern is evaluated and then sorted and cut exactly as
//! before; this only decides how many left rows reach the OPTIONALs.

use std::cell::Cell;

use spargebra::algebra::{Expression, Function, GraphPattern, OrderExpression};
use spargebra::term::NamedNodePattern;

use super::Bindings;
use super::pattern_util::triple_pattern_vars;
use crate::store::Store;
use crate::types::Value;

thread_local! {
    /// (armed, declined) for the rewritten evaluation running on this thread.
    static STATE: Cell<(bool, bool)> = const { Cell::new((false, false)) };
}

/// Arms the comparison-class check for one rewritten evaluation; restores the
/// enclosing state when dropped, so nested rewrites stay independent.
pub(super) struct Guard((bool, bool));

impl Guard {
    pub(super) fn arm() -> Self {
        Self(STATE.with(|s| s.replace((true, false))))
    }

    /// Disarm, restoring the enclosing state, and report whether an `ORDER BY`
    /// inside the rewritten pattern saw mixed key kinds.
    pub(super) fn finish(self) -> bool {
        let declined = STATE.with(|s| s.get().1);
        // Restore before the caller's fallback evaluation runs.
        drop(self);
        declined
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        STATE.with(|s| s.set(self.0));
    }
}

/// Under an armed guard, decline the rewrite unless `rows` order totally
/// under every key in `expression`. A no-op otherwise.
pub(super) fn check(store: &Store, rows: &[Bindings], expression: &[OrderExpression]) {
    if !STATE.with(|s| s.get().0) {
        return;
    }
    let total = expression.iter().all(|order| {
        let (OrderExpression::Asc(e) | OrderExpression::Desc(e)) = order;
        let mut class = None;
        rows.iter().all(|row| {
            let Some(kind) = kind(super::filter::eval_expr(store, e, row).as_ref()) else {
                return false;
            };
            // Unbound sorts first against everything, consistently.
            kind == Kind::Unbound || *class.get_or_insert(kind) == kind
        })
    });
    if !total {
        STATE.with(|s| s.set((true, true)));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Unbound,
    /// Integer and decimal: compared exactly (`BigDecimal`).
    Exact,
    /// `xsd:float` only: compared as `f32`.
    Float,
    /// `xsd:double` (and `Value::Float`): compared as `f64`.
    Double,
    Lexical,
    Bool,
    Iri,
}

/// The comparison class `compare_option_values` and the IRI key give a value,
/// or `None` where it is not totally ordered (NaN, bytes, an unparsable
/// numeric literal).
fn kind(v: Option<&Value>) -> Option<Kind> {
    let Some(v) = v else {
        return Some(Kind::Unbound);
    };
    if v.datatype()
        .is_some_and(crate::namespace::is_numeric_datatype)
    {
        // Numeric pairs go through `numeric_value::compare`, which is exact
        // between integers and decimals but casts to f32 or f64 as soon as a
        // float or double is involved. Each domain alone is a total order;
        // mixing them is not (franklin, #700: 9007199254740993 vs
        // 9.007199254740992E15 vs 9007199254740992). A value that does not
        // compare equal to itself (unparsable lexical, NaN) compares `Equal`
        // to everything and breaks transitivity on its own.
        if crate::numeric_value::compare(v, v) != Some(std::cmp::Ordering::Equal) {
            return None;
        }
        return Some(match v.datatype() {
            Some(crate::namespace::XSD_FLOAT) => Kind::Float,
            Some(crate::namespace::XSD_DOUBLE) => Kind::Double,
            _ => Kind::Exact,
        });
    }
    match v {
        Value::Ref(_) => Some(Kind::Iri),
        Value::Bool(_) => Some(Kind::Bool),
        Value::Str(_) | Value::Lang { .. } | Value::Typed { .. } => Some(Kind::Lexical),
        _ => None,
    }
}

/// The pattern to evaluate under `ORDER BY expression` with a `k`-row limit,
/// or `None` when the shape does not qualify (evaluate `inner` unchanged).
pub(super) fn rewrite(
    inner: &GraphPattern,
    expression: &[OrderExpression],
    k: usize,
) -> Option<GraphPattern> {
    match inner {
        GraphPattern::Graph {
            name: name @ NamedNodePattern::NamedNode(_),
            inner,
        } => Some(GraphPattern::Graph {
            name: name.clone(),
            inner: Box::new(rewrite(inner, expression, k)?),
        }),
        GraphPattern::Filter { expr, inner } => {
            let selection = |left: &GraphPattern| GraphPattern::Filter {
                expr: expr.clone(),
                inner: Box::new(left.clone()),
            };
            push_left(inner, expression, k, Some(expr), &selection)
        }
        GraphPattern::LeftJoin { .. } => {
            push_left(inner, expression, k, None, &|left: &GraphPattern| {
                left.clone()
            })
        }
        _ => None,
    }
}

/// Replace the left spine's leaf `L` of a `LeftJoin` chain by the top `k` of
/// `selection(L)`, when the keys and `filter` read only variables `L` binds.
fn push_left(
    chain: &GraphPattern,
    expression: &[OrderExpression],
    k: usize,
    filter: Option<&Expression>,
    selection: &dyn Fn(&GraphPattern) -> GraphPattern,
) -> Option<GraphPattern> {
    let GraphPattern::LeftJoin { .. } = chain else {
        return None;
    };
    let leaf = leftmost(chain);
    let GraphPattern::Bgp { patterns } = leaf else {
        return None;
    };
    let mut bound = Vec::new();
    for tp in patterns {
        bound.extend(triple_pattern_vars(tp));
    }
    let mut read = Vec::new();
    for order in expression {
        let (OrderExpression::Asc(e) | OrderExpression::Desc(e)) = order;
        if !reads(e, &mut read) {
            return None;
        }
    }
    if let Some(f) = filter
        && !reads(f, &mut read)
    {
        return None;
    }
    if !read.iter().all(|v| bound.contains(v)) {
        return None;
    }
    let top = GraphPattern::Slice {
        inner: Box::new(GraphPattern::OrderBy {
            inner: Box::new(selection(leaf)),
            expression: expression.to_vec(),
        }),
        start: 0,
        length: Some(k),
    };
    Some(replace_leftmost(chain, top))
}

fn leftmost(p: &GraphPattern) -> &GraphPattern {
    match p {
        GraphPattern::LeftJoin { left, .. } => leftmost(left),
        other => other,
    }
}

fn replace_leftmost(p: &GraphPattern, leaf: GraphPattern) -> GraphPattern {
    match p {
        GraphPattern::LeftJoin {
            left,
            right,
            expression,
        } => GraphPattern::LeftJoin {
            left: Box::new(replace_leftmost(left, leaf)),
            right: right.clone(),
            expression: expression.clone(),
        },
        _ => leaf,
    }
}

/// Collect the variables `e` reads, or `false` when it can observe more than
/// those values: `EXISTS`, and the functions whose result is not a function of
/// their arguments (`RAND`, `UUID`, `STRUUID`, `BNODE`, extension functions).
fn reads(e: &Expression, out: &mut Vec<String>) -> bool {
    match e {
        Expression::NamedNode(_) | Expression::Literal(_) => true,
        Expression::Variable(v) | Expression::Bound(v) => {
            out.push(v.as_str().to_string());
            true
        }
        Expression::Or(a, b)
        | Expression::And(a, b)
        | Expression::Equal(a, b)
        | Expression::SameTerm(a, b)
        | Expression::Greater(a, b)
        | Expression::GreaterOrEqual(a, b)
        | Expression::Less(a, b)
        | Expression::LessOrEqual(a, b)
        | Expression::Add(a, b)
        | Expression::Subtract(a, b)
        | Expression::Multiply(a, b)
        | Expression::Divide(a, b) => reads(a, out) && reads(b, out),
        Expression::UnaryPlus(a) | Expression::UnaryMinus(a) | Expression::Not(a) => reads(a, out),
        Expression::If(a, b, c) => reads(a, out) && reads(b, out) && reads(c, out),
        Expression::In(a, list) => reads(a, out) && list.iter().all(|x| reads(x, out)),
        Expression::Coalesce(list) => list.iter().all(|x| reads(x, out)),
        Expression::FunctionCall(f, args) => {
            !matches!(
                f,
                Function::Rand
                    | Function::Uuid
                    | Function::StrUuid
                    | Function::BNode
                    | Function::Custom(_)
            ) && args.iter().all(|x| reads(x, out))
        }
        Expression::Exists(_) => false,
    }
}
