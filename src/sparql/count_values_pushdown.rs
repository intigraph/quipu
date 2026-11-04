//! Evaluate a group FILTER's conjuncts before a constant VALUES join when
//! they cannot see its variables (aegis-xdvg.6).
//!
//! A group's FILTERs apply to the whole group, after any `VALUES` joined into
//! it. The 2fa stats aggregate joins seven `?metric` rows onto every board
//! item, so a conjunct about the item alone (`!BOUND(?inbox) && !REGEX(...)`)
//! was evaluated seven times per item. Such a conjunct gives the same answer
//! for each of an item's joined rows, so judging it once on the item first
//! keeps exactly the same rows in the same order.
//!
//! A conjunct moves only when it is deterministic, mentions no variable the
//! inline table or an intervening `BIND` defines, and the path down is
//! `BIND`s over a `Join` whose other side is the `VALUES` table. Anything else
//! stays where it was. `a && b` passes exactly when both pass, so evaluating
//! the conjuncts at different depths admits the same rows.

use spargebra::algebra::{Expression, Function, GraphPattern};

/// The pattern with movable FILTER conjuncts pushed below a VALUES join, or
/// `None` when nothing moves.
pub(super) fn rewrite(pattern: &GraphPattern) -> Option<GraphPattern> {
    let GraphPattern::Filter { expr, inner } = pattern else {
        return None;
    };
    let mut kept = Vec::new();
    let mut moved = false;
    let mut current = (**inner).clone();
    for conjunct in conjuncts(expr) {
        match push(conjunct, &current) {
            Some(next) => {
                current = next;
                moved = true;
            }
            None => kept.push(conjunct.clone()),
        }
    }
    if !moved {
        return None;
    }
    Some(
        match kept
            .into_iter()
            .reduce(|a, b| Expression::And(Box::new(a), Box::new(b)))
        {
            Some(expr) => GraphPattern::Filter {
                expr,
                inner: Box::new(current),
            },
            None => current,
        },
    )
}

fn conjuncts(expr: &Expression) -> Vec<&Expression> {
    match expr {
        Expression::And(a, b) => {
            let mut out = conjuncts(a);
            out.extend(conjuncts(b));
            out
        }
        other => vec![other],
    }
}

fn push(conjunct: &Expression, pattern: &GraphPattern) -> Option<GraphPattern> {
    let mut vars = Vec::new();
    if !deterministic_vars(conjunct, &mut vars) {
        return None;
    }
    place(conjunct, &vars, pattern)
}

fn place(conjunct: &Expression, vars: &[String], pattern: &GraphPattern) -> Option<GraphPattern> {
    match pattern {
        GraphPattern::Extend {
            inner,
            variable,
            expression,
        } if !vars.iter().any(|v| v == variable.as_str()) => Some(GraphPattern::Extend {
            inner: Box::new(place(conjunct, vars, inner)?),
            variable: variable.clone(),
            expression: expression.clone(),
        }),
        GraphPattern::Join { left, right } => match &**right {
            GraphPattern::Values { variables, .. }
                if !variables
                    .iter()
                    .any(|t| vars.iter().any(|v| v == t.as_str())) =>
            {
                Some(GraphPattern::Join {
                    left: Box::new(GraphPattern::Filter {
                        expr: conjunct.clone(),
                        inner: left.clone(),
                    }),
                    right: right.clone(),
                })
            }
            _ => None,
        },
        _ => None,
    }
}

/// Collect the conjunct's variables; false when it is not a deterministic
/// expression over its row (EXISTS, RAND/NOW/UUID/BNODE, custom functions).
fn deterministic_vars(expr: &Expression, vars: &mut Vec<String>) -> bool {
    match expr {
        Expression::Variable(v) | Expression::Bound(v) => {
            vars.push(v.as_str().to_owned());
            true
        }
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
        | Expression::Or(a, b) => deterministic_vars(a, vars) && deterministic_vars(b, vars),
        Expression::Not(a) | Expression::UnaryPlus(a) | Expression::UnaryMinus(a) => {
            deterministic_vars(a, vars)
        }
        Expression::If(a, b, c) => {
            deterministic_vars(a, vars)
                && deterministic_vars(b, vars)
                && deterministic_vars(c, vars)
        }
        Expression::In(a, list) => {
            deterministic_vars(a, vars) && list.iter().all(|e| deterministic_vars(e, vars))
        }
        Expression::Coalesce(list) => list.iter().all(|e| deterministic_vars(e, vars)),
        Expression::FunctionCall(function, args) => {
            !matches!(
                function,
                Function::Rand
                    | Function::Now
                    | Function::Uuid
                    | Function::StrUuid
                    | Function::BNode
                    | Function::Custom(_)
            ) && args.iter().all(|e| deterministic_vars(e, vars))
        }
        // EXISTS reads the store per row: never moved.
        Expression::Exists(_) => false,
    }
}
