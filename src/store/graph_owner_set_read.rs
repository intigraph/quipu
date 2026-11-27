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
    root: std::path::PathBuf,
    /// Phase 2 read-only ATTACH composition connections (aegis-n0wc.5).
    composition: Arc<crate::store::composition::CompositionPool>,
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
        let legacy_file = &catalog
            .physical()
            .legacy_participant()
            .ok_or_else(|| refused("explicit legacy participant required"))?
            .file_id;
        let legacy_path = root.join(format!("{legacy_file}.db"));
        // ONE composition connection until the memory plan accounts for more
        // (follow-up): it is counted beside the legacy readers it reads.
        let composition = crate::store::composition::CompositionPool::new(
            legacy_path
                .to_str()
                .ok_or_else(|| refused("legacy path is not UTF-8"))?,
            1,
            catalog.physical().budget().max_owners,
        )?;
        Ok(Self {
            catalog,
            legacy,
            routed,
            root: root.to_path_buf(),
            composition,
        })
    }

    /// Phase 2 read (seam A): like [`Self::read`], plus one composition
    /// connection that attaches exactly the selected routed files read-only
    /// and begins its snapshot while every commit fence is held, so it reads
    /// the same strict cut as the captured participants.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn read_attached<T>(
        &self,
        graphs: &[&str],
        deadline: Instant,
        query: impl FnOnce(&[CapturedRead<'_>], crate::store::composition::Checkout) -> Result<T>,
    ) -> Result<T> {
        let checkout = std::cell::RefCell::new(Some(self.composition.checkout()?));
        self.read_inner(
            graphs,
            deadline,
            true,
            |physical| {
                // R1: prove disjoint term spaces BEFORE anything is attached.
                crate::store::composition::check_term_spaces(physical)?;
                let mounts = physical
                    .iter()
                    .filter(|read| !read.legacy)
                    .enumerate()
                    .map(|(i, read)| crate::store::composition::Mount {
                        store_id: read.route.store_id.clone(),
                        alias: format!("set{i}"),
                        path: self
                            .root
                            .join(format!("{}.db", read.route.file_id))
                            .to_string_lossy()
                            .into_owned(),
                    })
                    .collect::<Vec<_>>();
                checkout
                    .borrow_mut()
                    .as_mut()
                    .ok_or_else(|| refused("composition checkout missing"))?
                    .begin(&mounts)
            },
            |selected| {
                let checkout = checkout
                    .borrow_mut()
                    .take()
                    .ok_or_else(|| refused("composition checkout missing"))?;
                query(selected, checkout)
            },
        )
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
        self.read_inner(graphs, deadline, false, |_| Ok(()), query)
    }

    fn read_inner<T>(
        &self,
        graphs: &[&str],
        deadline: Instant,
        cut: bool,
        under_fence: impl FnOnce(&[CapturedRead<'_>]) -> Result<()>,
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
        Owner::read_strict_inner(&owners, deadline, cut, under_fence, |physical| {
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
                    published_tx: source.published_tx,
                    store: source.store,
                    legacy: source.legacy,
                });
            }
            query(&selected)
        })
    }
}

#[cfg(test)]
impl GraphSetDataset {
    pub(crate) fn composition_idle(&self) -> usize {
        self.composition.idle_len()
    }

    pub(crate) fn composition_checkout(&self) -> Result<crate::store::composition::Checkout> {
        self.composition.checkout()
    }

    pub(crate) fn set_composition_attach_limit_for_test(&mut self, limit: usize) {
        self.composition = crate::store::composition::CompositionPool::new(
            &self.composition.legacy_path_for_test(),
            1,
            limit,
        )
        .unwrap();
    }
}

#[cfg(test)]
#[path = "graph_owner_set_read_tests.rs"]
mod tests;
