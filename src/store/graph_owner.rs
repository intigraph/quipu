//! A bounded native owner used to qualify the per-graph pilot.
//!
//! The writer is supplied fully configured, never reopened with weaker defaults.
//! Work runs on the caller's blocking thread, retaining authentication and write
//! provenance scopes. FIFO writer contention is local to this owner; its read
//! connections never fall back to another writer.
//!
//! This primitive does NOT publish a server route. Its advisory file lease
//! excludes other instances of this primitive, not arbitrary SQLite clients.
//! Durable bypass fencing, aggregate memory allocation and composed-read parity
//! remain mandatory before a server may advertise a routed graph.

use std::{
    fs::{File, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
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
    file: File,
    device: u64,
    inode: u64,
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
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)
            .map_err(refused)?;
        let metadata = file.metadata().map_err(refused)?;
        if !metadata.is_file() {
            return Err(refused("store is not a regular file"));
        }
        rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive)
            .map_err(|_| refused("native inode already has an owner lease"))?;
        let placement = Self {
            path,
            file,
            device: metadata.dev(),
            inode: metadata.ino(),
        };
        placement.verify()?;
        Ok(placement)
    }

    fn verify(&self) -> Result<()> {
        let metadata = std::fs::symlink_metadata(&self.path).map_err(refused)?;
        let held = self.file.metadata().map_err(refused)?;
        if !metadata.is_file()
            || metadata.dev() != self.device
            || metadata.ino() != self.inode
            || held.dev() != self.device
            || held.ino() != self.inode
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
        mut store: Store,
        installed_policy: &str,
    ) -> Result<Self> {
        let route = catalog
            .routes()
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
        let graph_id = store
            .lookup(graph)?
            .ok_or_else(|| refused("graph must already exist in owner"))?;
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
        let mut readers = Vec::with_capacity(budget.readers_per_owner);
        for _ in 0..budget.readers_per_owner {
            let mut reader = Store::open_read_only(path)?;
            reader.adopt_read_config_from(&store);
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
            let result = crate::tool_knot(&mut store, input)?;
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
            let store = self.writer.lock();
            self.placement.verify()?;
            Self::check_identity(&self.route, &store)?;
            store.apply_deferred_embed(&work, &embeddings)?;
        }
        Ok(result)
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

#[cfg(test)]
#[path = "graph_owner_tests.rs"]
mod tests;
