//! Native, already-planned same-file batches. No HTTP mount or migration.
use super::*;
use crate::{graph_catalog::GraphSetCatalog, store::Datum};
use std::collections::BTreeSet;

/// Where one routed update spent its time (aegis-n0wc.5 H2a).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OwnerHold {
    /// From the call until this owner's writer was held (admission and
    /// writer queue), or until the refusal when it never was.
    pub wait: std::time::Duration,
    /// How long the writer was held; `None` when refused before holding it.
    pub held: Option<std::time::Duration>,
}

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

#[path = "mini_fork.rs"]
pub mod mini_fork;

#[path = "participant_commit.rs"]
pub mod participant;

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
        owner.read_metadata(|store| {
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
        let supplied = self.check_batch(requests, batch)?;
        let _admission = self.owner.admit()?;
        let mut store = self.owner.writer.owned()?;
        self.owner.placement.verify()?;
        Owner::check_identity(self.owner.catalog_route(), &store)?;
        Self::check_hooks(&store)?;
        participant::check_write_fence(&store)?;
        participant::check_pinned_write(&store, &batch.graphs)?;
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

    /// Run one routed SPARQL update in this owner (aegis-n0wc.5 phase 4 H1).
    /// `members` are the graphs the update names; every one must be a member
    /// of this owner. The gates are [`Self::transact`]'s: unsigned, admission,
    /// placement, identity, hooks and the participant write fence. A file
    /// that records dependency pins refuses, because their guard needs the
    /// batch's datums and an update only knows them after evaluation.
    pub fn update<T>(
        &self,
        members: &BTreeSet<String>,
        work: impl FnOnce(&mut Store) -> Result<T>,
    ) -> Result<T> {
        self.update_observed(members, work).0
    }

    /// [`Self::update`], also reporting where it spent its time (aegis-n0wc.5
    /// H2a), so a routed write is as visible to write telemetry as a legacy
    /// one. The hold is returned on refusal too: a refused write still waited.
    pub fn update_observed<T>(
        &self,
        members: &BTreeSet<String>,
        work: impl FnOnce(&mut Store) -> Result<T>,
    ) -> (Result<T>, OwnerHold) {
        self.update_observed_ticketed(members, None, work)
    }

    /// [`Self::update_observed`] with a durable write-ID ticket (aegis-n0wc.5
    /// H3): the update's multi-graph commit records the ticket's witness in
    /// its own savepoint.
    #[cfg(unix)]
    pub fn update_observed_ticketed<T>(
        &self,
        members: &BTreeSet<String>,
        ticket: Option<&crate::store::knot_completion::update::UpdateTicket>,
        work: impl FnOnce(&mut Store) -> Result<T>,
    ) -> (Result<T>, OwnerHold) {
        let started = std::time::Instant::now();
        let mut hold = OwnerHold::default();
        let result = match ticket {
            Some(ticket) => {
                crate::store::knot_completion::update::with_update_ticket(ticket, || {
                    self.update_timed(members, work, started, &mut hold)
                })
                .0
            }
            None => self.update_timed(members, work, started, &mut hold),
        };
        if hold.held.is_none() {
            hold.wait = started.elapsed();
        }
        (result, hold)
    }

    fn update_timed<T>(
        &self,
        members: &BTreeSet<String>,
        work: impl FnOnce(&mut Store) -> Result<T>,
        started: std::time::Instant,
        hold: &mut OwnerHold,
    ) -> Result<T> {
        let owned = self
            .catalog
            .owner_graphs(&self.owner.catalog_route().graph)?;
        if members.is_empty()
            || members.iter().any(|g| !owned.contains(&g.as_str()))
            || crate::transaction_auth::current_attestation().is_some()
        {
            return Err(refused(
                "update does not select exactly this unsigned graph-set owner",
            ));
        }
        let _admission = self.owner.admit()?;
        let mut store = self.owner.writer.owned()?;
        // The writer is ours from here: `held` covers every check and the
        // commit made while holding it, as the legacy writer's does.
        hold.wait = started.elapsed();
        let acquired = std::time::Instant::now();
        let result = (|| {
            self.owner.placement.verify()?;
            Owner::check_identity(self.owner.catalog_route(), &store)?;
            Self::check_hooks(&store)?;
            participant::check_write_fence(&store)?;
            if participant::has_dependency_pins(&store)? {
                return Err(refused(
                    "a routed update into a file with dependency pins is not qualified",
                ));
            }
            store.ownership_operation(|store| {
                store.with_writer_scope(crate::writer_scope::WriteOperation::Update, members, work)
            })
        })();
        hold.held = Some(acquired.elapsed());
        result
    }

    /// Run one write-ID ledger operation on this owner's file under the same
    /// admission, placement, identity and participant-fence gates as an
    /// update, holding the writer only for the ledger write (aegis-n0wc.5 H3).
    #[cfg(unix)]
    fn ledger<T>(&self, op: impl FnOnce(&mut Store) -> Result<T>) -> Result<T> {
        let _admission = self.owner.admit()?;
        let mut store = self.owner.writer.owned()?;
        self.owner.placement.verify()?;
        Owner::check_identity(self.owner.catalog_route(), &store)?;
        participant::check_write_fence(&store)?;
        op(&mut store)
    }

    /// Admit a routed update's write ID in this owner's file BEFORE its effect.
    #[cfg(unix)]
    pub fn admit_update(
        &self,
        binding: crate::store::knot_completion::Binding,
    ) -> Result<crate::store::knot_completion::update::UpdateAdmission> {
        self.ledger(|store| store.admit_update_attempt(binding))
    }

    /// Record the complete receipt (or `None` on failure) after the effect.
    #[cfg(unix)]
    pub fn finish_update(
        &self,
        ticket: &crate::store::knot_completion::update::UpdateTicket,
        receipt: Option<&serde_json::Value>,
    ) -> Result<()> {
        self.ledger(|store| store.finish_update_attempt(ticket, receipt))
    }

    /// Seal an UNKNOWN (admitted, uncommitted) attempt.
    #[cfg(unix)]
    pub fn seal_update(
        &self,
        binding: &crate::store::knot_completion::Binding,
    ) -> Result<serde_json::Value> {
        self.ledger(|store| store.seal_update_binding(binding))
    }

    /// Exact recorded status of one binding in this owner's file.
    #[cfg(unix)]
    pub fn update_status(
        &self,
        binding: &crate::store::knot_completion::Binding,
    ) -> Result<serde_json::Value> {
        self.ledger(|store| store.update_attempt_status(binding))
    }

    /// A strict composition cut of `members` in this owner, at its published
    /// head (aegis-n0wc.5 H2b): what a routed `/update` receipt hands back so
    /// the writer can later read its own write at a defined point. Captured
    /// AFTER the write committed and its writer was released, so every member
    /// is at or after that write. Refuses (never queues) when a fence or reader
    /// is occupied; the caller decides what that means for the receipt.
    pub fn capture_cut(
        &self,
        members: &BTreeSet<String>,
        deadline: std::time::Instant,
    ) -> Result<crate::store::composition_cut::CompositionCut> {
        self.read_members(members, deadline, |selected| {
            crate::store::composition_cut::CompositionCut::capture(selected)
        })
    }

    /// One strict read of `members` in this owner (aegis-n0wc.5.2): a single
    /// captured snapshot of the owner file, exposed as one [`CapturedRead`]
    /// per selected member, all at the same coordinate. Every member must be
    /// a graph of THIS owner. Refuses (never queues) when a fence or reader is
    /// occupied. Authorization is the caller's: this grants nothing.
    pub fn read_members<T>(
        &self,
        members: &BTreeSet<String>,
        deadline: std::time::Instant,
        query: impl FnOnce(&[CapturedRead<'_>]) -> Result<T>,
    ) -> Result<T> {
        if members.is_empty() {
            return Err(refused("read selects no graph-set member"));
        }
        let routes = members
            .iter()
            .map(|graph| {
                self.catalog
                    .logical_route(graph)
                    .filter(|route| route.store_id == self.owner.catalog_route().store_id)
                    .ok_or_else(|| refused("read member is not a graph of this owner"))
            })
            .collect::<Result<Vec<_>>>()?;
        Owner::read_strict(&[&self.owner], deadline, |physical| {
            let source = physical
                .first()
                .ok_or_else(|| refused("strict capture returned no owner"))?;
            let selected: Vec<CapturedRead<'_>> = routes
                .iter()
                .map(|route| CapturedRead {
                    route,
                    catalog_revision: source.catalog_revision,
                    local_tx: source.local_tx,
                    published_tx: source.published_tx,
                    store: source.store,
                    legacy: source.legacy,
                })
                .collect();
            query(&selected)
        })
    }

    /// Pre-queue validation shared by local and participant batches: the
    /// requests select exactly this unsigned owner and the batch its members.
    fn check_batch<'b>(
        &self,
        requests: &[WriteRequest<'_>],
        batch: &'b GraphSetBatch,
    ) -> Result<BTreeSet<&'b str>> {
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
        Ok(supplied)
    }
}

#[cfg(test)]
#[path = "graph_owner_set_tests.rs"]
mod tests;
