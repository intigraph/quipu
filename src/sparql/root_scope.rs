//! ROOT is a default graph, not a named graph. Reject its GRAPH spelling before
//! evaluation can skip an empty branch or turn an expression error into no rows.

use spargebra::Query;
use spargebra::algebra::{AggregateExpression, Expression, GraphPattern, OrderExpression};
use spargebra::term::NamedNodePattern;

use crate::error::{Error, Result};

enum Node<'a> {
    Pattern(&'a GraphPattern),
    Expression(&'a Expression),
}

// The LATERAL variant exists only when dependency features are unified by SHACL.
#[allow(clippy::match_wildcard_for_single_variants)]
pub(super) fn check(query: &Query) -> Result<()> {
    let pattern = match query {
        Query::Select { pattern, .. }
        | Query::Ask { pattern, .. }
        | Query::Construct { pattern, .. }
        | Query::Describe { pattern, .. } => pattern,
    };
    let mut pending = vec![Node::Pattern(pattern)];
    while let Some(node) = pending.pop() {
        match node {
            Node::Pattern(pattern) => match pattern {
                GraphPattern::Graph { name, inner } => {
                    if matches!(name, NamedNodePattern::NamedNode(iri)
                        if iri.as_str() == crate::schema::ROOT_GRAPH_IRI)
                    {
                        return Err(Error::InvalidValue(
                            "urn:quipu:graph:root is the ROOT/default graph, not a named graph; \
                             omit the GRAPH clause to read ROOT, or use FROM <urn:quipu:graph:root>."
                                .into(),
                        ));
                    }
                    pending.push(Node::Pattern(inner));
                }
                GraphPattern::Join { left, right }
                | GraphPattern::Union { left, right }
                | GraphPattern::Minus { left, right } => {
                    pending.extend([Node::Pattern(left), Node::Pattern(right)]);
                }
                GraphPattern::LeftJoin {
                    left,
                    right,
                    expression,
                } => {
                    pending.extend([Node::Pattern(left), Node::Pattern(right)]);
                    if let Some(expression) = expression {
                        pending.push(Node::Expression(expression));
                    }
                }
                GraphPattern::Filter { inner, expr }
                | GraphPattern::Extend {
                    inner,
                    expression: expr,
                    ..
                } => {
                    pending.extend([Node::Pattern(inner), Node::Expression(expr)]);
                }
                GraphPattern::OrderBy { inner, expression } => {
                    pending.push(Node::Pattern(inner));
                    for OrderExpression::Asc(expr) | OrderExpression::Desc(expr) in expression {
                        pending.push(Node::Expression(expr));
                    }
                }
                GraphPattern::Group {
                    inner, aggregates, ..
                } => {
                    pending.push(Node::Pattern(inner));
                    for (_, aggregate) in aggregates {
                        if let AggregateExpression::FunctionCall { expr, .. } = aggregate {
                            pending.push(Node::Expression(expr));
                        }
                    }
                }
                GraphPattern::Project { inner, .. }
                | GraphPattern::Distinct { inner }
                | GraphPattern::Reduced { inner }
                | GraphPattern::Slice { inner, .. }
                | GraphPattern::Service { inner, .. } => pending.push(Node::Pattern(inner)),
                GraphPattern::Bgp { .. }
                | GraphPattern::Path { .. }
                | GraphPattern::Values { .. } => {}
                // Dependency feature unification can add LATERAL, which the
                // evaluator does not support. Refuse unknown algebra before
                // an empty branch can hide it, on every feature surface.
                #[allow(unreachable_patterns)]
                other => {
                    return Err(Error::InvalidValue(format!(
                        "unsupported graph pattern: {other:?}"
                    )));
                }
            },
            Node::Expression(expression) => match expression {
                Expression::Exists(pattern) => pending.push(Node::Pattern(pattern)),
                Expression::Or(left, right)
                | Expression::And(left, right)
                | Expression::Equal(left, right)
                | Expression::SameTerm(left, right)
                | Expression::Greater(left, right)
                | Expression::GreaterOrEqual(left, right)
                | Expression::Less(left, right)
                | Expression::LessOrEqual(left, right)
                | Expression::Add(left, right)
                | Expression::Subtract(left, right)
                | Expression::Multiply(left, right)
                | Expression::Divide(left, right) => {
                    pending.extend([Node::Expression(left), Node::Expression(right)]);
                }
                Expression::UnaryPlus(inner)
                | Expression::UnaryMinus(inner)
                | Expression::Not(inner) => {
                    pending.push(Node::Expression(inner));
                }
                Expression::In(left, candidates) => {
                    pending.push(Node::Expression(left));
                    pending.extend(candidates.iter().map(Node::Expression));
                }
                Expression::If(condition, yes, no) => {
                    pending.extend([
                        Node::Expression(condition),
                        Node::Expression(yes),
                        Node::Expression(no),
                    ]);
                }
                Expression::Coalesce(args) | Expression::FunctionCall(_, args) => {
                    pending.extend(args.iter().map(Node::Expression));
                }
                Expression::NamedNode(_)
                | Expression::Literal(_)
                | Expression::Variable(_)
                | Expression::Bound(_) => {}
            },
        }
    }
    Ok(())
}
