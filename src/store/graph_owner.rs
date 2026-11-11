//! A bounded native owner used to qualify the per-graph pilot.
//!
//! The writer is supplied fully configured, never reopened with weaker defaults.
//! Work runs on the caller's blocking thread, retaining authentication and write
//! provenance scopes. FIFO writer contention is local to this owner; its read
//! connections never fall back to another writer.
//!
//! This primitive does NOT publish a server route. Its advisory file lease
//! excludes cooperating writable Store handles, not arbitrary SQLite clients.
//! Facts and deferred-embedding writes reuse Store's ownership operation seam;
//! inference runs outside that lineage latch. An enrolled owner file opens only
//! through S1 admission (`Owner::open_enrolled`), which installs the generation
//! pin; this adapter sets no pin of its own. Durable bypass fencing, aggregate
//! memory allocation and composed-read parity remain mandatory before a server
//! advertises a route.

use std::{
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use parking_lot::FairMutex;
use serde_json::Value;

use super::Store;
use crate::{
    Error, Result,
    graph_catalog::{Catalog, Route, WriteRequest, WriteTarget},
};

/// One prepared native writer and its dedicated, bounded read pool.
pub struct Owner {
    catalog: Arc<Catalog>,
    route: Route,
    writer: FairMutex<Store>,
    readers: Vec<FairMutex<Store>>,
    next_reader: AtomicUsize,
    admitted: AtomicUsize,
    capacity: usize,
    // Drop the store connections before releasing the inode lease.
    placement: Placement,
}

struct Placement {
    path: PathBuf,
    lease: Arc<super::writer_lease::Lease>,
    device: u64,
    inode: u64,
    lease_device: u64,
    lease_inode: u64,
}

fn refused(message: impl std::fmt::Display) -> Error {
    Error::PolicyDenied(format!("graph owner: {message}"))
}

impl Placement {
    fn claim(store: &Store) -> Result<Self> {
        let path = store
            .conn
            .path()
            .filter(|path| !path.is_empty())
            .ok_or_else(|| refused("native file placement required"))?;
        let path = PathBuf::from(path);
        if !path.is_absolute() || path.canonicalize().map_err(refused)? != path {
            return Err(refused("canonical path without symlinks required"));
        }
        let lease = store
            .writer_lease
            .as_ref()
            .ok_or_else(|| refused("writable Store admission required"))?
            .clone();
        lease.claim_exclusive()?;
        // Identify the database by path metadata (a stat, no descriptor): the
        // lease is held on a sibling file, never on the database itself.
        let metadata = std::fs::symlink_metadata(&path).map_err(refused)?;
        if !metadata.is_file() {
            return Err(refused("store is not a regular file"));
        }
        let held = lease.file.metadata().map_err(refused)?;
        let placement = Self {
            path,
            lease,
            device: metadata.dev(),
            inode: metadata.ino(),
            lease_device: held.dev(),
            lease_inode: held.ino(),
        };
        placement.verify()?;
        Ok(placement)
    }

    fn verify(&self) -> Result<()> {
        let metadata = std::fs::symlink_metadata(&self.path).map_err(refused)?;
        let lease_path = super::ownership_latch::sibling(&self.path, ".writer-lease")?;
        let lease = std::fs::symlink_metadata(&lease_path).map_err(refused)?;
        let held = self.lease.file.metadata().map_err(refused)?;
        if !metadata.is_file()
            || metadata.dev() != self.device
            || metadata.ino() != self.inode
            || lease.dev() != self.lease_device
            || lease.ino() != self.lease_inode
            || held.dev() != self.lease_device
            || held.ino() != self.lease_inode
            || self.path.canonicalize().map_err(refused)? != self.path
        {
            return Err(refused("native file placement changed"));
        }
        Ok(())
    }
}

struct Admission<'a>(&'a AtomicUsize);

impl Drop for Admission<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Owner {
    /// Prepare an already configured and explicitly owned native store.
    /// `installed_policy` comes from the trusted initializer, never a request.
    /// Opens read-only connections but never creates/adopts/migrates a store.
    pub fn prepare(
        catalog: Arc<Catalog>,
        root: &Path,
        graph: &str,
        store: Store,
        installed_policy: &str,
    ) -> Result<Self> {
        Self::prepare_inner(catalog, root, graph, store, installed_policy, None)
    }

    /// Prepare with a share of the caller's one aggregate memory plan. Applies
    /// SQLite ceilings to writer, ownership-fence reader and every read-pool
    /// connection before returning the owner. Does not reserve evaluator heaps
    /// or replace service admission/cgroup enforcement.
    pub fn prepare_with_memory(
        catalog: Arc<Catalog>,
        root: &Path,
        graph: &str,
        store: Store,
        installed_policy: &str,
        allocation: crate::memory_budget::MemoryAllocation,
    ) -> Result<Self> {
        Self::prepare_inner(
            catalog,
            root,
            graph,
            store,
            installed_policy,
            Some(allocation),
        )
    }

    /// Open an enrolled owner file through S1 (`Store::open_enrolled`), then
    /// prepare it exactly as [`Owner::prepare_with_memory`] does.
    ///
    /// The route and its canonical placement are checked BEFORE admission, so
    /// a catalog that does not name this file never writes a startup receipt.
    /// `configure` installs the same served writer configuration the trusted
    /// initializer applies to any writer. It runs on the admitted handle before
    /// readers adopt that configuration. An admission or configuration error
    /// returns no owner, and dropping the handle releases its lease.
    #[cfg(not(target_arch = "wasm32"))]
    // Dormant like `Store::open_enrolled`: no production caller until the
    // router integration and a witness-profile resolver exist.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn open_enrolled(
        catalog: Arc<Catalog>,
        root: &Path,
        graph: &str,
        startup: &super::ownership_admission::EnrolledStartup<'_>,
        installed_policy: &str,
        configure: impl FnOnce(&mut Store) -> Result<()>,
        allocation: Option<crate::memory_budget::MemoryAllocation>,
    ) -> Result<Self> {
        let route = catalog
            .routes()
            .chain(catalog.legacy_participant())
            .find(|route| route.graph == graph)
            .ok_or_else(|| refused("unregistered graph owner"))?;
        if installed_policy != route.policy {
            return Err(refused("installed policy differs from catalog"));
        }
        if !root.is_absolute() || root.canonicalize().map_err(refused)? != root {
            return Err(refused("owner file does not match catalog placement"));
        }
        let path = root.join(format!("{}.db", route.file_id));
        let path = path
            .to_str()
            .ok_or_else(|| refused("native store path must be UTF-8"))?;
        let mut store = Store::open_enrolled(path, startup)?;
        configure(&mut store)?;
        Self::prepare_inner(catalog, root, graph, store, installed_policy, allocation)
    }

    fn prepare_inner(
        catalog: Arc<Catalog>,
        root: &Path,
        graph: &str,
        mut store: Store,
        installed_policy: &str,
        allocation: Option<crate::memory_budget::MemoryAllocation>,
    ) -> Result<Self> {
        let route = catalog
            .routes()
            .chain(catalog.legacy_participant())
            .find(|route| route.graph == graph)
            .cloned()
            .ok_or_else(|| refused("unregistered graph owner"))?;
        if installed_policy != route.policy {
            return Err(refused("installed policy differs from catalog"));
        }
        store.require_ownership()?;
        let placement = Placement::claim(&store)?;
        if !root.is_absolute()
            || root.canonicalize().map_err(refused)? != root
            || placement.path != root.join(format!("{}.db", route.file_id))
        {
            return Err(refused("owner file does not match catalog placement"));
        }
        Self::check_identity(&route, &store)?;
        let graph_id = if graph == crate::schema::ROOT_GRAPH_IRI {
            0
        } else {
            store
                .lookup(graph)?
                .ok_or_else(|| refused("graph must already exist in owner"))?
        };
        if store.graph_class(graph_id)?.as_deref() != Some("committed") {
            return Err(refused("owner graph must be committed"));
        }
        let budget = catalog.budget();
        let capacity = budget
            .queued_per_owner
            .checked_add(1)
            .ok_or_else(|| refused("mailbox bound overflow"))?;
        let path = placement
            .path
            .to_str()
            .ok_or_else(|| refused("native store path must be UTF-8"))?;
        if let Some(allocation) = allocation {
            store.apply_memory_allocation(allocation)?;
            store
                .ownership_fence
                .as_ref()
                .ok_or_else(|| refused("ownership reader missing from memory plan"))?
                .apply_memory_allocation(allocation)?;
        }
        let mut readers = Vec::with_capacity(budget.readers_per_owner);
        for _ in 0..budget.readers_per_owner {
            let mut reader = Store::open_read_only(path)?;
            // An S1-admitted writer's readers share its admission; ordinary
            // owners are unchanged.
            store.share_admission_with(&mut reader)?;
            reader.adopt_read_config_from(&store);
            if let Some(allocation) = allocation {
                reader.apply_memory_allocation(allocation)?;
            }
            Self::check_identity(&route, &reader)?;
            readers.push(FairMutex::new(reader));
        }
        placement.verify()?;
        store.set_defer_auto_embed(true);
        Ok(Self {
            catalog,
            route,
            writer: FairMutex::new(store),
            readers,
            next_reader: AtomicUsize::new(0),
            admitted: AtomicUsize::new(0),
            capacity,
            placement,
        })
    }

    /// The immutable catalog route this owner serves.
    pub fn route(&self) -> &Route {
        &self.route
    }

    fn check_identity(route: &Route, store: &Store) -> Result<()> {
        store.require_ownership()?;
        if store.store_id()? != route.store_id || store.log_epoch()? != route.log_epoch {
            return Err(refused("store identity or log epoch differs from catalog"));
        }
        Ok(())
    }

    fn admit(&self) -> Result<Admission<'_>> {
        self.admitted
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < self.capacity).then(|| n + 1)
            })
            .map_err(|_| refused("writer mailbox full; nothing queued"))?;
        Ok(Admission(&self.admitted))
    }

    /// Execute the ordinary guarded knot pipeline in this owner only.
    /// No signed replay or cross-file synchronous validation is admitted.
    /// A post-commit embedding failure remains an error, never an implicit retry.
    pub fn knot(&self, request: &WriteRequest<'_>, input: &Value) -> Result<Value> {
        if self.catalog.write_target(request)? != WriteTarget::Routed(&self.route)
            || input.get("graph").and_then(Value::as_str) != Some(self.route.graph.as_str())
            || crate::transaction_auth::current_attestation().is_some()
        {
            return Err(refused("request does not select this unsigned owner"));
        }
        let _admission = self.admit()?;
        let (result, work, provider, batch_size) = {
            let mut store = self.writer.lock();
            self.placement.verify()?;
            Self::check_identity(&self.route, &store)?;
            let result = store.ownership_operation(|store| crate::tool_knot(store, input))?;
            (
                result,
                store.take_deferred_embed(),
                store.embedding_provider(),
                match store.embedding_config().embed_batch_size {
                    0 => 32,
                    size => size,
                },
            )
        };
        if let (Some(work), Some(provider)) = (work, provider) {
            let texts = work.texts();
            let mut embeddings = Vec::with_capacity(texts.len());
            for chunk in texts.chunks(batch_size) {
                // Owner admission remains reserved; inference holds no writer.
                embeddings.extend(provider.embed_batch(chunk)?);
            }
            let mut store = self.writer.lock();
            self.placement.verify()?;
            Self::check_identity(&self.route, &store)?;
            store.ownership_operation(|store| store.apply_deferred_embed(&work, &embeddings))?;
        }
        Ok(result)
    }

    /// Seed this owner's graph with a producer slice exported from another
    /// file, under the same `snapshot:<key>` tag, through the ownership seam.
    /// Refuses if the tag already owns current facts here.
    pub fn import_slice(
        &self,
        slice: &[super::routed_slice::SliceFact],
        source_tag: &str,
        timestamp: &str,
    ) -> Result<Option<i64>> {
        let _admission = self.admit()?;
        let mut store = self.writer.lock();
        self.placement.verify()?;
        Self::check_identity(&self.route, &store)?;
        let graph = self.route.graph.clone();
        store.ownership_operation(|store| {
            store.import_routed_slice(slice, source_tag, &graph, timestamp)
        })
    }

    /// Read on a free dedicated read-only connection or refuse immediately.
    /// Identity checks and callback reads share one SQLite snapshot. Callbacks
    /// run inside that read transaction; nested BEGIN operations are refused.
    /// No global writer, writer fallback or unbounded reader queue is acquired.
    pub fn read<T>(&self, query: impl FnOnce(&Store) -> Result<T>) -> Result<T> {
        self.placement.verify()?;
        let start = self.next_reader.fetch_add(1, Ordering::Relaxed) % self.readers.len();
        for offset in 0..self.readers.len() {
            let index = (start + offset) % self.readers.len();
            if let Some(reader) = self.readers[index].try_lock() {
                let snapshot = reader.conn.unchecked_transaction()?;
                Self::check_identity(&self.route, &reader)?;
                let result = query(&reader)?;
                snapshot.commit()?;
                return Ok(result);
            }
        }
        Err(refused("owner read pool full; no writer fallback"))
    }
}

#[path = "graph_owner_read.rs"]
mod strict_read;
pub use strict_read::CapturedRead;

#[cfg(test)]
#[path = "graph_owner_tests.rs"]
mod tests;
