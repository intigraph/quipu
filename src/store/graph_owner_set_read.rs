//! Logical graph selection over one strict snapshot per physical file.
use super::*;
use std::{collections::BTreeMap, time::Instant};

/// Source-only read dataset. No HTTP mount, route publication or read grant.
/// The initializer supplies the sole configured legacy writer and all routed
/// owners; requests select exact logical graphs and retain normal authorization.
pub struct GraphSetDataset {
    catalog: Arc<GraphSetCatalog>,
    legacy: Owner,
    routed: BTreeMap<String, GraphSetOwner>,
}

impl GraphSetDataset {
    /// Bind every physical owner to one immutable graph-set manifest. The same
    /// aggregate allocation must cover legacy, routed and scratch connections.
    pub fn prepare(
        catalog: Arc<GraphSetCatalog>,
        root: &Path,
        legacy: Store,
        owners: Vec<GraphSetOwner>,
        installed_policy: &str,
        allocation: crate::memory_budget::MemoryAllocation,
    ) -> Result<Self> {
        if catalog.physical().legacy_participant().is_none() {
            return Err(refused("explicit legacy participant required"));
        }
        let mut routed = BTreeMap::new();
        for owner in owners {
            let route = owner.owner.catalog_route();
            if !Arc::ptr_eq(&catalog, &owner.catalog)
                || !owner.owner.prepared_for(&catalog.physical_pin())
                || routed.contains_key(&route.graph)
            {
                return Err(refused(
                    "graph-set owner is not this dataset's unique owner",
                ));
            }
            routed.insert(route.graph.clone(), owner);
        }
        if routed.len() != catalog.physical().routes().count() {
            return Err(refused("dataset is missing a configured physical owner"));
        }
        let legacy = Owner::prepare_with_memory(
            catalog.physical_pin(),
            root,
            crate::schema::ROOT_GRAPH_IRI,
            legacy,
            installed_policy,
            allocation,
        )?;
        legacy.read(|store| {
            for graph in catalog.owner_graphs(crate::schema::ROOT_GRAPH_IRI)? {
                if graph != crate::schema::ROOT_GRAPH_IRI
                    && store.committed_graph_id(graph)?.is_none()
                {
                    return Err(refused("legacy logical member is absent or not committed"));
                }
            }
            Ok(())
        })?;
        Ok(Self {
            catalog,
            legacy,
            routed,
        })
    }

    /// Capture once per selected physical owner, then expose ONLY the selected
    /// logical members from those same snapshots. Admission permits and dataset
    /// grants remain the evaluator's responsibility. No partial/vector fallback.
    pub fn read<T>(
        &self,
        graphs: &[&str],
        deadline: Instant,
        query: impl FnOnce(&[CapturedRead<'_>]) -> Result<T>,
    ) -> Result<T> {
        let plans = self.catalog.plan_read(graphs)?;
        let owners = plans
            .iter()
            .map(|plan| {
                if plan.owner.graph == crate::schema::ROOT_GRAPH_IRI {
                    Ok(&self.legacy)
                } else {
                    self.routed
                        .get(&plan.owner.graph)
                        .map(|owner| &owner.owner)
                        .ok_or_else(|| refused("configured dataset owner unavailable"))
                }
            })
            .collect::<Result<Vec<_>>>()?;
        let routes = plans
            .iter()
            .flat_map(|plan| plan.graphs.iter())
            .map(|graph| {
                self.catalog
                    .logical_route(graph)
                    .ok_or_else(|| refused("logical graph left pinned dataset"))
            })
            .collect::<Result<Vec<_>>>()?;
        Owner::read_strict(&owners, deadline, |physical| {
            let mut selected = Vec::with_capacity(routes.len());
            for route in &routes {
                let source = physical
                    .iter()
                    .find(|read| read.route.store_id == route.store_id)
                    .ok_or_else(|| refused("logical member has no captured physical owner"))?;
                selected.push(CapturedRead {
                    route,
                    catalog_revision: source.catalog_revision,
                    local_tx: source.local_tx,
                    store: source.store,
                });
            }
            query(&selected)
        })
    }
}

#[cfg(test)]
#[path = "graph_owner_set_read_tests.rs"]
mod tests;
