//! What a migration template may contain, and how much it may touch.
//!
//! - Templates read and write the ROOT graph only: `GRAPH` and `SERVICE` are
//!   refused anywhere in a `WHERE` pattern, including inside `EXISTS`.
//! - A typed operation's expression is never pasted into a template as text.
//!   It is parsed on its own as one SPARQL expression, refused if it holds a
//!   graph pattern (`EXISTS`), and re-serialized from the parsed tree.
//! - A migration is bounded by `server.update_full_copy_max_facts`, the
//!   ceiling a whole-store `/update` already honours: an operation matching
//!   more solutions, or a program planning more facts, is refused.

use std::sync::atomic::{AtomicUsize, Ordering};

use spargebra::algebra::{AggregateExpression, Expression, GraphPattern, OrderExpression};

use crate::error::{Error, Result};

/// Default ceiling, matching `server.update_full_copy_max_facts`.
const DEFAULT_MAX_FACTS: usize = 250_000;

static MAX_FACTS: AtomicUsize = AtomicUsize::new(DEFAULT_MAX_FACTS);

#[cfg(test)]
thread_local! {
    static TEST_MAX_FACTS: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Set the migration ceiling (the server passes
/// `server.update_full_copy_max_facts` at startup).
pub fn set_max_facts(max: usize) {
    MAX_FACTS.store(max, Ordering::Relaxed);
}

/// The largest number of solutions one operation, or of facts one program,
/// may touch.
#[must_use]
pub fn max_facts() -> usize {
    #[cfg(test)]
    if let Some(max) = TEST_MAX_FACTS.with(std::cell::Cell::get) {
        return max;
    }
    MAX_FACTS.load(Ordering::Relaxed)
}

/// Run `f` with a different ceiling on this thread only.
#[cfg(all(test, feature = "shacl"))]
pub(super) fn with_max_facts<T>(max: usize, f: impl FnOnce() -> T) -> T {
    TEST_MAX_FACTS.with(|c| c.set(Some(max)));
    let out = f();
    TEST_MAX_FACTS.with(|c| c.set(None));
    out
}

/// The refusal for a migration over the ceiling.
pub(super) fn too_large(what: &str) -> Error {
    Error::InvalidValue(format!(
        "{what} more than {} facts; split the migration or raise \
         server.update_full_copy_max_facts. No facts were written",
        max_facts()
    ))
}

/// Apply the migration ceiling to intermediate WHERE bindings as well as
/// final solutions. LIMIT alone cannot bound a FILTER, join or DISTINCT input.
pub(super) fn bounded_query(
    store: &crate::store::Store,
    query: &str,
) -> Result<crate::sparql::QueryResult> {
    let migration_limit = max_facts();
    let configured = store.search_config().max_join_rows;
    let row_cap = if configured == 0 {
        migration_limit
    } else {
        configured.min(migration_limit)
    };
    let context = crate::sparql::TemporalContext {
        row_cap: Some(row_cap),
        ..crate::sparql::TemporalContext::default()
    };
    crate::sparql::query_temporal(store, query, &context).map_err(|error| match error {
        Error::QueryComplexity { limit } if limit == migration_limit => {
            too_large("the WHERE evaluation materializes")
        }
        other => other,
    })
}

fn refuse(message: &str) -> Error {
    Error::InvalidValue(format!("migration template: {message}"))
}

/// Refuse a `WHERE` pattern that reads beyond the ROOT graph.
#[allow(clippy::match_wildcard_for_single_variants)]
pub(super) fn check_pattern(pattern: &GraphPattern) -> Result<()> {
    match pattern {
        GraphPattern::Bgp { .. } | GraphPattern::Path { .. } | GraphPattern::Values { .. } => {
            Ok(())
        }
        GraphPattern::Graph { .. } => Err(refuse(
            "GRAPH is not supported; templates read the ROOT graph only",
        )),
        GraphPattern::Service { .. } => Err(refuse("SERVICE is not supported")),
        GraphPattern::Join { left, right }
        | GraphPattern::Union { left, right }
        | GraphPattern::Minus { left, right } => {
            check_pattern(left)?;
            check_pattern(right)
        }
        GraphPattern::LeftJoin {
            left,
            right,
            expression,
        } => {
            check_pattern(left)?;
            check_pattern(right)?;
            expression.as_ref().map_or(Ok(()), check_expression)
        }
        GraphPattern::Filter { expr, inner } => {
            check_expression(expr)?;
            check_pattern(inner)
        }
        GraphPattern::Extend {
            inner, expression, ..
        } => {
            check_expression(expression)?;
            check_pattern(inner)
        }
        GraphPattern::OrderBy { inner, expression } => {
            for order in expression {
                match order {
                    OrderExpression::Asc(e) | OrderExpression::Desc(e) => check_expression(e)?,
                }
            }
            check_pattern(inner)
        }
        GraphPattern::Project { inner, .. }
        | GraphPattern::Distinct { inner }
        | GraphPattern::Reduced { inner }
        | GraphPattern::Slice { inner, .. } => check_pattern(inner),
        GraphPattern::Group {
            inner, aggregates, ..
        } => {
            for (_, aggregate) in aggregates {
                if let AggregateExpression::FunctionCall { expr, .. } = aggregate {
                    check_expression(expr)?;
                }
            }
            check_pattern(inner)
        }
        // Anything newer (e.g. LATERAL, present only with some spargebra
        // features) fails closed.
        #[allow(unreachable_patterns)]
        _ => Err(refuse("unsupported graph pattern")),
    }
}

/// Every sub-expression, and the pattern of any `EXISTS`.
fn check_expression(expression: &Expression) -> Result<()> {
    match expression {
        Expression::NamedNode(_)
        | Expression::Literal(_)
        | Expression::Variable(_)
        | Expression::Bound(_) => Ok(()),
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
        | Expression::Divide(a, b) => {
            check_expression(a)?;
            check_expression(b)
        }
        Expression::UnaryPlus(a) | Expression::UnaryMinus(a) | Expression::Not(a) => {
            check_expression(a)
        }
        Expression::If(a, b, c) => {
            check_expression(a)?;
            check_expression(b)?;
            check_expression(c)
        }
        Expression::In(a, list) => {
            check_expression(a)?;
            list.iter().try_for_each(check_expression)
        }
        Expression::Coalesce(list) | Expression::FunctionCall(_, list) => {
            list.iter().try_for_each(check_expression)
        }
        Expression::Exists(pattern) => check_pattern(pattern),
    }
}

/// Does `expression` contain a graph pattern?
fn has_exists(expression: &Expression) -> bool {
    match expression {
        Expression::Exists(_) => true,
        Expression::NamedNode(_)
        | Expression::Literal(_)
        | Expression::Variable(_)
        | Expression::Bound(_) => false,
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
        | Expression::Divide(a, b) => has_exists(a) || has_exists(b),
        Expression::UnaryPlus(a) | Expression::UnaryMinus(a) | Expression::Not(a) => has_exists(a),
        Expression::If(a, b, c) => has_exists(a) || has_exists(b) || has_exists(c),
        Expression::In(a, list) => has_exists(a) || list.iter().any(has_exists),
        Expression::Coalesce(list) | Expression::FunctionCall(_, list) => {
            list.iter().any(has_exists)
        }
    }
}

/// Parse a typed operation's `expression` as exactly ONE SPARQL expression
/// and return it re-serialized from the parse tree, so no text the caller
/// wrote reaches the template.
pub(super) fn expression(text: &str) -> Result<String> {
    let bad = |why: &str| Error::InvalidValue(format!("expression {text:?} {why}"));
    let query = format!("SELECT ((\n{text}\n) AS ?quipu_expression) WHERE {{}}");
    let parsed = crate::sparql::sparql_parser()
        .parse_query(&query)
        .map_err(|_| bad("is not a single SPARQL expression"))?;
    let spargebra::Query::Select { pattern, .. } = parsed else {
        return Err(bad("is not a single SPARQL expression"));
    };
    let GraphPattern::Project { inner, variables } = pattern else {
        return Err(bad("is not a single SPARQL expression"));
    };
    let GraphPattern::Extend {
        inner,
        variable,
        expression,
    } = *inner
    else {
        return Err(bad("is not a single SPARQL expression"));
    };
    let empty = matches!(*inner, GraphPattern::Bgp { ref patterns } if patterns.is_empty());
    if !empty || variables.len() != 1 || variable.as_str() != "quipu_expression" {
        return Err(bad("is not a single SPARQL expression"));
    }
    if has_exists(&expression) {
        return Err(bad("may not contain a graph pattern (EXISTS)"));
    }
    Ok(expression.to_string())
}
