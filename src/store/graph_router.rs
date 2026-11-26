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

#[cfg(all(feature = "ownership-witness-s3", not(target_arch = "wasm32")))]
#[path = "graph_router_enrolled.rs"]
mod enrolled;
#[cfg(all(feature = "ownership-witness-s3", not(target_arch = "wasm32")))]
pub use enrolled::EnrolledPilotConfig;

fn refused(message: impl std::fmt::Display) -> Error {
    Error::PolicyDenied(format!("graph router: {message}"))
}

/// Dispatches writes between the legacy writer and per-graph owners.
pub struct Router {
    catalog: Arc<Catalog>,
    owners: BTreeMap<String, Owner>,
}

/// Source-pilot admission only; not a server configuration or cutover command.
/// Default OFF avoids preparing an owner. The caller must inventory whether
/// its selected graph is regenerable; a graph never determines a file path.
#[derive(Debug, Clone, Default)]
pub struct IngestPilot {
    /// Explicit opt-in for a disposable, single-graph qualification.
    pub enabled: bool,
    /// Exact graph already pinned in the supplied catalog.
    pub graph: Option<String>,
}

impl IngestPilot {
    /// Admit one graph through existing owner/fence checks. Disabled admission
    /// does not call `prepare`, open a file or install a fence. Preparation
    /// supplies an already configured owner, never weaker writer defaults.
    pub fn admit(
        &self,
        catalog: Arc<Catalog>,
        legacy: &Store,
        prepare: impl FnOnce() -> Result<Owner>,
    ) -> Result<Option<Router>> {
        if !self.enabled {
            return Ok(None);
        }
        let mut routes = catalog.routes();
        let route = routes
            .next()
            .ok_or_else(|| refused("pilot requires one route"))?;
        if routes.next().is_some() || self.graph.as_deref() != Some(route.graph.as_str()) {
            return Err(refused(
                "pilot requires exactly its explicitly selected graph",
            ));
        }
        drop(routes);
        Router::check_fence(&catalog, legacy)?;
        let owner = prepare()?;
        Router::admit(catalog, legacy, vec![owner]).map(Some)
    }
}

impl Router {
    /// Admit a router. `owners` must be exactly one prepared owner per catalog
    /// route, each prepared against this catalog. `legacy` is the legacy
    /// writer's store: its fence must name exactly the catalog's routes at
    /// this revision and their generations, and must verify.
    pub fn admit(catalog: Arc<Catalog>, legacy: &Store, owners: Vec<Owner>) -> Result<Self> {
        let mut by_graph = BTreeMap::new();
        for owner in owners {
            let route = owner.catalog_route().clone();
            if !owner.prepared_for(&catalog)
                || catalog.routes().find(|r| r.graph == route.graph) != Some(&route)
            {
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
        match self.routed_knot(request, input)? {
            Some(result) => Ok(result),
            None => crate::tool_knot(legacy, input),
        }
    }

    /// Dispatch a routed knot without borrowing or locking the legacy writer.
    /// `None` selects legacy without executing it; only then should a caller
    /// acquire its legacy writer. An error is a refusal, never legacy fallback.
    /// The caller's authentication/provenance scopes are retained as in `knot`.
    pub fn routed_knot(&self, request: &WriteRequest<'_>, input: &Value) -> Result<Option<Value>> {
        if request.graph != input.get("graph").and_then(Value::as_str) {
            return Err(refused("request graph differs from the write's graph"));
        }
        match self.catalog.write_target(request)? {
            WriteTarget::Routed(route) => self
                .owners
                .get(&route.graph)
                .ok_or_else(|| refused("routed graph has no owner"))?
                .knot(request, input)
                .map(Some),
            WriteTarget::Legacy => Ok(None),
        }
    }

    /// The owner serving `graph`, if it is routed.
    pub fn owner(&self, graph: &str) -> Option<&Owner> {
        self.owners.get(graph)
    }

    /// Immutable request revision; does not grant a graph or writer capability.
    pub fn catalog_revision(&self) -> u64 {
        self.catalog.revision()
    }

    /// The source pilot admits one explicit graph, never an inferred default.
    pub fn single_routed_graph(&self) -> Option<&str> {
        (self.owners.len() == 1).then(|| self.owners.keys().next().unwrap().as_str())
    }
}

#[cfg(test)]
#[path = "graph_router_tests.rs"]
mod tests;
