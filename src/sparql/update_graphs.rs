//! Which graphs a SPARQL Update reads and writes (aegis-n0wc.5 phase 4 H1).
//!
//! A routed update runs over ONE owner file's dataset, so routing must see
//! every graph it names, in WHERE as well as in the templates: a CAS that
//! reads a graph from another file would otherwise evaluate over a dataset
//! that lacks it and give a different answer without an error. The walk is
//! fail-closed: anything whose graph reach it cannot state (a variable
//! `GRAPH ?g`, a default-graph pattern, `USING`/`WITH`, `SERVICE`, graph
//! management, unknown algebra) is reported, and the router refuses such an
//! update whenever a routed graph is also involved.
use std::collections::BTreeSet;

use spargebra::algebra::{
    AggregateExpression, Expression, GraphPattern, GraphTarget, OrderExpression,
};
use spargebra::term::{GraphName, GraphNamePattern, NamedNodePattern};
use spargebra::{GraphUpdateOperation, Update};

/// The graph reach of one update.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct UpdateGraphs {
    /// Every named graph an operation writes or a pattern reads.
    pub named: BTreeSet<String>,
    /// A quad or pattern in the default graph (ROOT).
    pub default: bool,
    /// Something whose graph reach is not a fixed list: a variable graph,
    /// `USING`/`WITH`, `SERVICE`, LOAD/CLEAR/CREATE/DROP or unknown algebra.
    pub open: bool,
}

/// The graph reach of `update`.
#[must_use]
pub fn update_graphs(update: &Update) -> UpdateGraphs {
    let mut out = UpdateGraphs::default();
    for op in &update.operations {
        match op {
            GraphUpdateOperation::InsertData { data } => {
                for q in data {
                    out.graph_name(&q.graph_name);
                }
            }
            GraphUpdateOperation::DeleteData { data } => {
                for q in data {
                    out.graph_name(&q.graph_name);
                }
            }
            GraphUpdateOperation::DeleteInsert {
                delete,
                insert,
                using,
                pattern,
            } => {
                for g in delete
                    .iter()
                    .map(|q| &q.graph_name)
                    .chain(insert.iter().map(|q| &q.graph_name))
                {
                    match g {
                        GraphNamePattern::NamedNode(n) => {
                            out.named.insert(n.as_str().into());
                        }
                        GraphNamePattern::DefaultGraph => out.default = true,
                        GraphNamePattern::Variable(_) => out.open = true,
                    }
                }
                if using.is_some() {
                    out.open = true;
                }
                out.pattern(pattern, false);
            }
            // Management reach is never a fixed fact list: always open, and
            // the named target is reported so a refusal can name it.
            GraphUpdateOperation::Load { destination, .. } => {
                out.open = true;
                out.graph_name(destination);
            }
            GraphUpdateOperation::Create { graph, .. } => {
                out.open = true;
                out.named.insert(graph.as_str().into());
            }
            GraphUpdateOperation::Clear { graph, .. }
            | GraphUpdateOperation::Drop { graph, .. } => {
                out.open = true;
                if let GraphTarget::NamedNode(n) = graph {
                    out.named.insert(n.as_str().into());
                }
            }
        }
    }
    out
}

impl UpdateGraphs {
    fn graph_name(&mut self, g: &GraphName) {
        match g {
            GraphName::NamedNode(n) => {
                self.named.insert(n.as_str().into());
            }
            GraphName::DefaultGraph => self.default = true,
        }
    }

    #[allow(clippy::match_wildcard_for_single_variants)]
    fn pattern(&mut self, p: &GraphPattern, in_graph: bool) {
        match p {
            GraphPattern::Bgp { patterns } => {
                if !in_graph && !patterns.is_empty() {
                    self.default = true;
                }
            }
            GraphPattern::Path { .. } => {
                if !in_graph {
                    self.default = true;
                }
            }
            GraphPattern::Graph { name, inner } => {
                match name {
                    NamedNodePattern::NamedNode(n) => {
                        self.named.insert(n.as_str().into());
                    }
                    NamedNodePattern::Variable(_) => self.open = true,
                }
                self.pattern(inner, true);
            }
            GraphPattern::Join { left, right }
            | GraphPattern::Union { left, right }
            | GraphPattern::Minus { left, right } => {
                self.pattern(left, in_graph);
                self.pattern(right, in_graph);
            }
            GraphPattern::LeftJoin {
                left,
                right,
                expression,
            } => {
                self.pattern(left, in_graph);
                self.pattern(right, in_graph);
                if let Some(e) = expression {
                    self.expression(e, in_graph);
                }
            }
            GraphPattern::Filter { inner, expr }
            | GraphPattern::Extend {
                inner,
                expression: expr,
                ..
            } => {
                self.pattern(inner, in_graph);
                self.expression(expr, in_graph);
            }
            GraphPattern::OrderBy { inner, expression } => {
                self.pattern(inner, in_graph);
                for OrderExpression::Asc(e) | OrderExpression::Desc(e) in expression {
                    self.expression(e, in_graph);
                }
            }
            GraphPattern::Group {
                inner, aggregates, ..
            } => {
                self.pattern(inner, in_graph);
                for (_, aggregate) in aggregates {
                    if let AggregateExpression::FunctionCall { expr, .. } = aggregate {
                        self.expression(expr, in_graph);
                    }
                }
            }
            GraphPattern::Project { inner, .. }
            | GraphPattern::Distinct { inner }
            | GraphPattern::Reduced { inner }
            | GraphPattern::Slice { inner, .. } => self.pattern(inner, in_graph),
            GraphPattern::Values { .. } => {}
            // SERVICE reads another endpoint; LATERAL and anything newer is
            // not stated here. Neither is a fixed list of this store's graphs.
            #[allow(unreachable_patterns)]
            _ => self.open = true,
        }
    }

    fn expression(&mut self, e: &Expression, in_graph: bool) {
        match e {
            Expression::Exists(p) => self.pattern(p, in_graph),
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
                self.expression(a, in_graph);
                self.expression(b, in_graph);
            }
            Expression::UnaryPlus(a) | Expression::UnaryMinus(a) | Expression::Not(a) => {
                self.expression(a, in_graph);
            }
            Expression::In(a, list) => {
                self.expression(a, in_graph);
                for x in list {
                    self.expression(x, in_graph);
                }
            }
            Expression::If(a, b, c) => {
                self.expression(a, in_graph);
                self.expression(b, in_graph);
                self.expression(c, in_graph);
            }
            Expression::FunctionCall(_, args) | Expression::Coalesce(args) => {
                for x in args {
                    self.expression(x, in_graph);
                }
            }
            Expression::NamedNode(_)
            | Expression::Literal(_)
            | Expression::Variable(_)
            | Expression::Bound(_) => {}
        }
    }
}

#[cfg(test)]
#[path = "update_graphs_tests.rs"]
mod tests;
