//! Place a FILTER next to the basic graph pattern it can judge.
//!
//! A group's FILTER applies to the whole group, so spargebra puts it above the
//! group's BINDs, its UNION and its OPTIONALs:
//! `Filter(?stamp in month, Extend(Union(Graph(g1, LeftJoin(Bgp, ..)), ..)))`.
//! Evaluated as written, every branch joins every row through every OPTIONAL
//! before the month filter drops most of them. The BGP-level pushdowns
//! (`filter_drive`, the status and string pushdowns) never fire, because the
//! filter does not wrap the BGP. Measured on a copy of a 1.7M-fact board: the
//! month's closed-item statistics took 5-13 s, for any month.
//!
//! This copies a leading run of the filter's conjuncts down to the pattern
//! that binds their variables, and leaves the original filter where it is:
//!
//! - **Only through row-preserving operators.** UNION, `GRAPH <iri>`, BIND
//!   (when it does not bind a filtered variable) and the left side of OPTIONAL
//!   when every filtered variable is certainly bound there. None of these drops
//!   a row, so every row the copied filter sees also reaches the original one
//!   with the same values for the variables it reads.
//! - **Only a prefix of the conjuncts, with stable variables** (the
//!   [`super::filter_drive`] rule). `&&` evaluates left to right and stops at
//!   `false`, so a hard error in a later conjunct is raised by the copy only on
//!   a row where the original also reaches it. `EXISTS` and volatile functions
//!   are never copied.
//! - A copied conjunct only drops rows the original would drop (`a` not true
//!   makes `a && b` not true), and the original still runs on top, so the
//!   result is unchanged.

use std::collections::HashSet;

use spargebra::algebra::{Expression, GraphPattern};
use spargebra::term::NamedNodePattern;

use super::filter_drive::stable_vars;
use super::pattern_util::triple_pattern_vars;

/// `inner` with a prefix of `expr`'s conjuncts copied down beside the pattern
/// that binds them, or `None` when nothing can move (the caller then evaluates
/// the original unchanged).
pub(super) fn placed(expr: &Expression, inner: &GraphPattern) -> Option<GraphPattern> {
    #[cfg(test)]
    if tests::disabled() {
        return None;
    }
    let mut conjuncts = Vec::new();
    collect(expr, &mut conjuncts);
    let mut vars = HashSet::new();
    let prefix: Vec<&Expression> = conjuncts
        .into_iter()
        .take_while(|c| {
            let mut own = HashSet::new();
            let ok = stable_vars(c, &mut own) && !own.is_empty();
            if ok {
                vars.extend(own);
            }
            ok
        })
        .collect();
    if prefix.is_empty() {
        return None;
    }
    let copy = prefix
        .into_iter()
        .cloned()
        .reduce(|a, b| Expression::And(Box::new(a), Box::new(b)))?;
    let placed = push(&copy, &vars, inner, false);
    #[cfg(test)]
    if placed.is_some() {
        tests::placed_once();
    }
    placed
}

fn collect<'a>(expr: &'a Expression, out: &mut Vec<&'a Expression>) {
    if let Expression::And(left, right) = expr {
        collect(left, out);
        collect(right, out);
    } else {
        out.push(expr);
    }
}

/// Descend through row-preserving operators; wrap the first pattern that is
/// not one. `moved` records whether any descent happened: wrapping in place
/// would only duplicate the original filter.
fn push(
    copy: &Expression,
    vars: &HashSet<String>,
    pattern: &GraphPattern,
    moved: bool,
) -> Option<GraphPattern> {
    match pattern {
        GraphPattern::Union { left, right } => Some(GraphPattern::Union {
            left: Box::new(push_or_wrap(copy, vars, left)?),
            right: Box::new(push_or_wrap(copy, vars, right)?),
        }),
        GraphPattern::Graph {
            name: NamedNodePattern::NamedNode(name),
            inner,
        } => Some(GraphPattern::Graph {
            name: NamedNodePattern::NamedNode(name.clone()),
            inner: Box::new(push_or_wrap(copy, vars, inner)?),
        }),
        GraphPattern::Extend {
            inner,
            variable,
            expression,
        } if !vars.contains(variable.as_str()) => Some(GraphPattern::Extend {
            inner: Box::new(push_or_wrap(copy, vars, inner)?),
            variable: variable.clone(),
            expression: expression.clone(),
        }),
        GraphPattern::LeftJoin {
            left,
            right,
            expression,
        } if vars.is_subset(&certain(left)) => Some(GraphPattern::LeftJoin {
            left: Box::new(push_or_wrap(copy, vars, left)?),
            right: right.clone(),
            expression: expression.clone(),
        }),
        _ if moved => (vars.is_subset(&certain(pattern))).then(|| GraphPattern::Filter {
            expr: copy.clone(),
            inner: Box::new(pattern.clone()),
        }),
        _ => None,
    }
}

/// One level below a row-preserving operator: descend further, or wrap here
/// when this pattern certainly binds every filtered variable. `None` aborts
/// the whole placement (some branch cannot judge the copy).
fn push_or_wrap(
    copy: &Expression,
    vars: &HashSet<String>,
    pattern: &GraphPattern,
) -> Option<GraphPattern> {
    push(copy, vars, pattern, true)
}

/// Variables bound in EVERY solution of `pattern` (conservative: an operator
/// not listed binds nothing certainly).
pub(super) fn certain(pattern: &GraphPattern) -> HashSet<String> {
    match pattern {
        GraphPattern::Bgp { patterns } => patterns.iter().flat_map(triple_pattern_vars).collect(),
        GraphPattern::Join { left, right } => {
            let mut vars = certain(left);
            vars.extend(certain(right));
            vars
        }
        GraphPattern::LeftJoin { left, .. }
        | GraphPattern::Filter { inner: left, .. }
        | GraphPattern::Extend { inner: left, .. } => certain(left),
        GraphPattern::Union { left, right } => {
            let right = certain(right);
            certain(left)
                .into_iter()
                .filter(|v| right.contains(v))
                .collect()
        }
        GraphPattern::Graph { name, inner } => {
            let mut vars = certain(inner);
            if let NamedNodePattern::Variable(v) = name {
                vars.insert(v.as_str().to_string());
            }
            vars
        }
        _ => HashSet::new(),
    }
}

#[cfg(test)]
#[path = "filter_place_tests.rs"]
mod tests;
