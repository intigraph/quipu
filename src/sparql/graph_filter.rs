//! Narrow only a direct GRAPH-variable/BGP scope, retaining the real FILTER.
use super::TemporalContext;
use crate::{error::Result, store::Store};
use spargebra::{
    algebra::{Expression, GraphPattern},
    term::NamedNodePattern,
};

/// A dataset restriction cannot escape this GRAPH into nested GRAPH/subqueries:
/// only a plain BGP is admitted. Other shapes retain the original evaluator.
pub(super) fn narrowed(
    store: &Store,
    expr: &Expression,
    inner: &GraphPattern,
    ctx: &TemporalContext,
) -> Result<Option<TemporalContext>> {
    let GraphPattern::Graph {
        name: NamedNodePattern::Variable(var),
        inner,
    } = inner
    else {
        return Ok(None);
    };
    if !matches!(inner.as_ref(), GraphPattern::Bgp { .. }) {
        return Ok(None);
    }
    let Some(narrow) = super::string_pushdown::graph_narrow(expr, var.as_str()) else {
        return Ok(None);
    };
    let graphs = store
        .list_graphs(None, None)?
        .into_iter()
        .filter(|g| {
            g.g != 0
                && ctx
                    .named_dataset
                    .as_ref()
                    .is_none_or(|ids| ids.contains(&g.g))
                && narrow.keeps_text(&g.iri)
        })
        .map(|g| g.g)
        .collect();
    Ok(Some(TemporalContext {
        named_dataset: Some(graphs),
        ..ctx.clone()
    }))
}

#[cfg(test)]
thread_local! { static SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
#[cfg(test)]
pub(super) fn record_scan() {
    SCANS.with(|n| n.set(n.get() + 1));
}
#[cfg(test)]
pub(super) fn scans(reset: bool) -> usize {
    SCANS.with(|n| if reset { n.replace(0) } else { n.get() })
}
