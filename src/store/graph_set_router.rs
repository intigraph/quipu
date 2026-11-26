//! Graph-set write dispatch (aegis-n0wc.5, Phase 1): one graph-set catalog,
//! the legacy writer, and one prepared [`GraphSetOwner`] per physical owner.
//!
//! Admission requires the legacy file's fence to name EVERY logical member of
//! every routed set (not only the owner graph) at the catalog revision and the
//! set's generation, and nothing else; legacy members stay unfenced. A write to
//! any routed member through a path that bypasses this router is therefore
//! refused by the legacy file itself.
//!
//! Dispatch never falls back. A batch whose targets are all routed runs in its
//! one owner; a batch whose targets are all legacy is classified, not executed,
//! so the caller uses its ordinary legacy writer; a mixed or cross-file batch
//! refuses before any writer is touched.
//!
//! This primitive publishes no server route and changes no startup path.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::{GraphSetBatch, GraphSetOwner};
use crate::{
    Error, Result,
    graph_catalog::{GraphSetCatalog, WriteRequest},
    store::Store,
};

fn refused(message: impl std::fmt::Display) -> Error {
    Error::PolicyDenied(format!("graph-set router: {message}"))
}

/// Where a complete batch must run. `Legacy` executes nothing.
#[derive(Debug, PartialEq, Eq)]
pub enum SetTarget<'a> {
    /// Every target is a legacy member (or the default graph).
    Legacy,
    /// Every target is a member of this one routed physical owner.
    Routed(&'a str),
}

/// Dispatches complete batches between the legacy writer and set owners.
pub struct GraphSetRouter {
    catalog: Arc<GraphSetCatalog>,
    owners: BTreeMap<String, GraphSetOwner>,
}

impl GraphSetRouter {
    /// Admit a router. `owners` must be exactly one owner per routed physical
    /// owner of `catalog`, each prepared against this same catalog value.
    pub fn admit(
        catalog: Arc<GraphSetCatalog>,
        legacy: &Store,
        owners: Vec<GraphSetOwner>,
    ) -> Result<Self> {
        let mut by_owner = BTreeMap::new();
        for owner in owners {
            if !Arc::ptr_eq(&owner.catalog, &catalog) {
                return Err(refused("owner was prepared against another catalog"));
            }
            let route = owner.owner.catalog_route();
            if catalog.physical().routes().find(|r| r.graph == route.graph) != Some(route) {
                return Err(refused("owner route is not this catalog's route"));
            }
            if by_owner.insert(route.graph.clone(), owner).is_some() {
                return Err(refused("more than one owner for a physical owner"));
            }
        }
        if by_owner.len() != catalog.physical().routes().count() {
            return Err(refused("a routed physical owner has no prepared owner"));
        }
        Self::check_fence(&catalog, legacy)?;
        Ok(Self {
            catalog,
            owners: by_owner,
        })
    }

    /// The legacy fence must be exactly every routed member at this revision
    /// and its set's generation, with every trigger present.
    fn check_fence(catalog: &GraphSetCatalog, legacy: &Store) -> Result<()> {
        legacy.verify_routed_fence()?;
        let mut fenced = legacy.routed_graph_fence_rows()?;
        fenced.sort();
        let revision = catalog.physical().revision();
        let mut expected = Vec::new();
        for route in catalog.physical().routes() {
            for member in catalog.owner_graphs(&route.graph)? {
                expected.push((member.to_owned(), revision, route.generation));
            }
        }
        expected.sort();
        if fenced != expected {
            return Err(refused(
                "legacy fence does not name exactly every routed graph-set member at this revision and generation",
            ));
        }
        Ok(())
    }

    /// Re-check the legacy fence, e.g. after legacy schema maintenance.
    pub fn recheck(&self, legacy: &Store) -> Result<()> {
        Self::check_fence(&self.catalog, legacy)
    }

    /// A member of a routed (non-ROOT) set. As with the single-graph router,
    /// every other graph, including the default graph, is legacy; the legacy
    /// fence is what keeps routed members out of the legacy file.
    fn routed(&self, graph: Option<&str>) -> bool {
        graph.is_some_and(|graph| {
            self.catalog
                .owner_graphs(graph)
                .is_ok_and(|members| !members.contains(&crate::schema::ROOT_GRAPH_IRI))
        })
    }

    /// Classify a complete batch without executing it or touching a writer.
    /// Mixed legacy/routed, cross-file and stale-pin batches refuse.
    pub fn target(&self, requests: &[WriteRequest<'_>]) -> Result<SetTarget<'_>> {
        if requests.is_empty() {
            return Err(refused("empty batch"));
        }
        let legacy = requests.iter().filter(|r| !self.routed(r.graph)).count();
        if legacy == requests.len() {
            return Ok(SetTarget::Legacy);
        }
        if legacy != 0 {
            return Err(refused(
                "batch mixes legacy and routed graphs; no cross-file write exists",
            ));
        }
        let plan = self.catalog.plan_batch(requests)?;
        let (key, _) = self
            .owners
            .get_key_value(&plan.owner.graph)
            .ok_or_else(|| refused("routed physical owner has no owner"))?;
        Ok(SetTarget::Routed(key))
    }

    /// Execute a routed batch in its owner. A legacy batch is refused here,
    /// never forwarded: the caller must use its ordinary legacy writer.
    pub fn transact(
        &self,
        requests: &[WriteRequest<'_>],
        batch: &GraphSetBatch,
    ) -> Result<Vec<(String, i64)>> {
        match self.target(requests)? {
            SetTarget::Routed(owner) => self.owners[owner].transact(requests, batch),
            SetTarget::Legacy => Err(refused("legacy batch must use the ordinary legacy writer")),
        }
    }

    /// The set owner serving `graph`'s physical owner, if routed.
    pub fn owner(&self, graph: &str) -> Option<&GraphSetOwner> {
        let members = self.catalog.owner_graphs(graph).ok()?;
        self.owners
            .iter()
            .find(|(key, _)| members.contains(&key.as_str()))
            .map(|(_, owner)| owner)
    }

    /// Immutable request revision; grants no graph or writer capability.
    pub fn catalog_revision(&self) -> u64 {
        self.catalog.physical().revision()
    }
}

#[cfg(test)]
#[path = "graph_set_router_tests.rs"]
mod tests;
