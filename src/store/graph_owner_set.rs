//! Native, already-planned same-file batches. No HTTP mount or migration.
use super::*;
use crate::{graph_catalog::GraphSetCatalog, store::Datum};
use std::collections::BTreeSet;

/// A native delta planned in this exact owner's term space. Numeric term IDs
/// are file-local; callers must not copy them from another store. This is not
/// an HTTP payload, durable retry ticket or multi-file transaction protocol.
pub struct GraphSetBatch {
    /// Expected physical lineage of every datum.
    pub store_id: String,
    /// Expected change-log epoch, not a scalar global cursor.
    pub log_epoch: String,
    /// Ordered graph-scoped changes; order is preserved in returned local heads.
    pub graphs: Vec<(String, Vec<Datum>)>,
    /// Ordinary transaction valid time.
    pub timestamp: String,
    /// Ordinary declared write actor, not authentication.
    pub actor: Option<String>,
    /// Ordinary source attribution.
    pub source: Option<String>,
}

/// One native Owner for an explicit graph set. It reuses the same file lease,
/// identity checks, bounded FIFO mailbox, read pool and ownership commit seam.
/// The supplied Store must already be configured/admitted by its initializer.
pub struct GraphSetOwner {
    owner: Owner,
    catalog: Arc<GraphSetCatalog>,
}

#[path = "graph_owner_set_read.rs"]
mod dataset;
pub use dataset::GraphSetDataset;

#[path = "graph_set_router.rs"]
mod router;
pub use router::{GraphSetRouter, SetTarget};

impl GraphSetOwner {
    fn check_hooks(store: &Store) -> Result<()> {
        if store.pending_embed.is_some()
            || (store.embedding_config.auto_embed
                && store.vector_delegate.is_none()
                && store.embedding_provider.is_some())
        {
            return Err(refused("graph-set embedding completion is not qualified"));
        }
        #[cfg(feature = "reactive-reasoner")]
        if !store.observers.is_empty() {
            return Err(refused("graph-set reactive completion is not qualified"));
        }
        Ok(())
    }

    /// Prepare exactly one writer; never create/adopt a file or install an S1
    /// pin. Every configured member must already be a committed graph in it.
    pub fn prepare(
        catalog: Arc<GraphSetCatalog>,
        root: &Path,
        graph: &str,
        store: Store,
        installed_policy: &str,
        allocation: crate::memory_budget::MemoryAllocation,
    ) -> Result<Self> {
        Self::check_hooks(&store)?;
        let plan = catalog.plan_read(&[graph])?;
        let canonical = plan[0].owner.graph.clone();
        let owner = Owner::prepare_with_memory(
            catalog.physical_pin(),
            root,
            &canonical,
            store,
            installed_policy,
            allocation,
        )?;
        Self::check_members(&owner, &catalog, &canonical)?;
        Ok(Self { owner, catalog })
    }

    /// Test latch: hold this owner's writer to prove other owners never wait.
    #[cfg(all(test, feature = "ownership-witness-s3", not(target_arch = "wasm32")))]
    pub(crate) fn hold_writer_for_test(&self) -> parking_lot::FairMutexGuard<'_, Store> {
        self.owner.writer.lock()
    }

    /// Every configured member must already be a committed graph in the file.
    fn check_members(owner: &Owner, catalog: &GraphSetCatalog, canonical: &str) -> Result<()> {
        owner.read(|store| {
            for member in catalog.owner_graphs(canonical)? {
                let id = store
                    .committed_graph_id(member)?
                    .ok_or_else(|| refused("graph-set member is absent or not committed"))?;
                if store.graph_class(id)?.as_deref() != Some("committed") {
                    return Err(refused("graph-set member is not committed"));
                }
            }
            Ok(())
        })
    }

    /// Open one routed set owner's EXISTING enrolled file through S1, the same
    /// enrolled path as the ingest pilot: witness configuration is validated
    /// and the policy pinned before credentials resolve; S1 then verifies the
    /// inventory, marker, journal and live witness before an ownership pin.
    /// `configure` must install the served writer policy; unqualified
    /// embedding/reactive hooks then refuse. There is no fallback to an
    /// ordinary open, enrollment or the legacy writer.
    #[cfg(all(feature = "ownership-witness-s3", not(target_arch = "wasm32")))]
    pub fn open_enrolled(
        catalog: Arc<GraphSetCatalog>,
        graph: &str,
        config: &crate::store::graph_router::EnrolledPilotConfig,
        credentials: impl FnOnce() -> Result<(String, String)>,
        configure: impl FnOnce(&mut Store) -> Result<()>,
    ) -> Result<Self> {
        let location = config.location()?;
        let plan = catalog.plan_read(&[graph])?;
        let route = plan[0].owner.clone();
        if route.graph == crate::schema::ROOT_GRAPH_IRI {
            return Err(refused("the legacy participant is not a graph-set owner"));
        }
        if config.installed_policy != route.policy {
            return Err(refused("installed policy differs from graph-set catalog"));
        }
        let owner = config.open_owner(
            location,
            catalog.physical_pin(),
            &route,
            credentials,
            |store| {
                configure(store)?;
                Self::check_hooks(store)
            },
        )?;
        Self::check_members(&owner, &catalog, &route.graph)?;
        Ok(Self { owner, catalog })
    }

    /// Execute one native batch through Store's existing guarded atomic batch
    /// pipeline. Validate ALL targets/pins before queue admission. One ownership
    /// permit covers the physical outer commit; failure returns no local heads.
    /// Signed requests and cross-graph validation remain unsupported. This API
    /// adds no retry/idempotency protocol and no external side-effect callback.
    pub fn transact(
        &self,
        requests: &[WriteRequest<'_>],
        batch: &GraphSetBatch,
    ) -> Result<Vec<(String, i64)>> {
        let plan = self.catalog.plan_batch(requests)?;
        let supplied: BTreeSet<_> = batch.graphs.iter().map(|(g, _)| g.as_str()).collect();
        if plan.owner != self.owner.catalog_route()
            || batch.store_id != plan.owner.store_id
            || batch.log_epoch != plan.owner.log_epoch
            || supplied.len() != batch.graphs.len()
            || supplied != plan.graphs.into_iter().collect()
            || crate::transaction_auth::current_attestation().is_some()
        {
            return Err(refused(
                "batch does not select exactly this unsigned graph-set owner",
            ));
        }
        let _admission = self.owner.admit()?;
        let mut store = self.owner.writer.owned()?;
        self.owner.placement.verify()?;
        Owner::check_identity(self.owner.catalog_route(), &store)?;
        Self::check_hooks(&store)?;
        let native = batch
            .graphs
            .iter()
            .map(|(graph, datums)| {
                let id = store
                    .committed_graph_id(graph)?
                    .ok_or_else(|| refused("graph-set member is absent or not committed"))?;
                Ok((id, datums.clone()))
            })
            .collect::<Result<Vec<_>>>()?;
        let targets = supplied.into_iter().map(str::to_owned).collect();
        let heads = store.ownership_operation(|store| {
            store.with_writer_scope(
                crate::writer_scope::WriteOperation::Update,
                &targets,
                |store| {
                    store.transact_graph_batches_tx(
                        &native,
                        &batch.timestamp,
                        batch.actor.as_deref(),
                        batch.source.as_deref(),
                    )
                },
            )
        })?;
        Ok(batch
            .graphs
            .iter()
            .zip(heads)
            .map(|((graph, _), (_, tx))| (graph.clone(), tx))
            .collect())
    }
}

#[cfg(test)]
#[path = "graph_owner_set_tests.rs"]
mod tests;
