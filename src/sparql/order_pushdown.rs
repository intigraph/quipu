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
//! The rewritten pattern is evaluated and then sorted and cut exactly as
//! before; this only decides how many left rows reach the OPTIONALs.

use spargebra::algebra::{Expression, Function, GraphPattern, OrderExpression};
use spargebra::term::NamedNodePattern;

use super::pattern_util::triple_pattern_vars;

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
