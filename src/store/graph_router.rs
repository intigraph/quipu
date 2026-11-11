//! The pilot write router: one catalog, the legacy writer, and one prepared
//! [`Owner`] per routed graph.
//!
//! A router is built only after the legacy file fences every catalog route at
//! the catalog's revision and that route's generation, and the fence verifies
//! over the live schema. Until then no route is advertised. Each write is then
//! dispatched by the catalog's write target: a routed graph goes to its owner,
//! anything else to the caller's legacy writer. There is no fallback in either
//! direction; the legacy file's own fence still refuses a routed graph if a
//! caller bypasses the router.
//!
//! This primitive publishes no server route and changes no startup path.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;

use super::{Store, graph_owner::Owner};
use crate::{
    Error, Result,
    graph_catalog::{Catalog, WriteRequest, WriteTarget},
};

fn refused(message: impl std::fmt::Display) -> Error {
    Error::PolicyDenied(format!("graph router: {message}"))
}

/// Dispatches writes between the legacy writer and per-graph owners.
pub struct Router {
    catalog: Arc<Catalog>,
    owners: BTreeMap<String, Owner>,
}

impl Router {
    /// Admit a router. `owners` must be exactly one prepared owner per catalog
    /// route, each prepared against this catalog. `legacy` is the legacy
    /// writer's store: its fence must name exactly the catalog's routes at
    /// this revision and their generations, and must verify.
    pub fn admit(catalog: Arc<Catalog>, legacy: &Store, owners: Vec<Owner>) -> Result<Self> {
        let mut by_graph = BTreeMap::new();
        for owner in owners {
            let route = owner.route().clone();
            if catalog.routes().find(|r| r.graph == route.graph) != Some(&route) {
                return Err(refused("owner route is not this catalog's route"));
            }
            if by_graph.insert(route.graph.clone(), owner).is_some() {
                return Err(refused("more than one owner for a graph"));
            }
        }
        if by_graph.len() != catalog.routes().count() {
            return Err(refused("a catalog route has no prepared owner"));
        }
        Self::check_fence(&catalog, legacy)?;
        Ok(Self {
            catalog,
            owners: by_graph,
        })
    }

    /// The legacy fence must match the catalog exactly: every route fenced at
    /// this revision and generation, nothing else, and every trigger present.
    fn check_fence(catalog: &Catalog, legacy: &Store) -> Result<()> {
        legacy.verify_routed_fence()?;
        let fenced = legacy.routed_graph_fence_rows()?;
        let expected: Vec<(String, u64, u64)> = catalog
            .routes()
            .map(|r| (r.graph.clone(), catalog.revision(), r.generation))
            .collect();
        let mut fenced_sorted = fenced;
        fenced_sorted.sort();
        let mut expected_sorted = expected;
        expected_sorted.sort();
        if fenced_sorted != expected_sorted {
            return Err(refused(
                "legacy fence does not name exactly this catalog's routes, revision and generations",
            ));
        }
        Ok(())
    }

    /// Re-check the legacy fence, e.g. before a new catalog pin or after
    /// maintenance that may have changed the legacy schema.
    pub fn recheck(&self, legacy: &Store) -> Result<()> {
        Self::check_fence(&self.catalog, legacy)
    }

    /// Dispatch one guarded knot. A routed graph runs in its owner only; a
    /// legacy target runs the ordinary pipeline on `legacy`. The request's
    /// `graph` must equal the input's `graph`, so the two cannot disagree.
    pub fn knot(
        &self,
        legacy: &mut Store,
        request: &WriteRequest<'_>,
        input: &Value,
    ) -> Result<Value> {
        if request.graph != input.get("graph").and_then(Value::as_str) {
            return Err(refused("request graph differs from the write's graph"));
        }
        match self.catalog.write_target(request)? {
            WriteTarget::Routed(route) => self
                .owners
                .get(&route.graph)
                .ok_or_else(|| refused("routed graph has no owner"))?
                .knot(request, input),
            WriteTarget::Legacy => crate::tool_knot(legacy, input),
        }
    }

    /// The owner serving `graph`, if it is routed.
    pub fn owner(&self, graph: &str) -> Option<&Owner> {
        self.owners.get(graph)
    }
}

#[cfg(test)]
#[path = "graph_router_tests.rs"]
mod tests;
