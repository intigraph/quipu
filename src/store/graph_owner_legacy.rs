//! The borrowed legacy read participant (aegis-4qv option A).
//!
//! The server keeps its configured legacy writer. A pilot dataset borrows it
//! read-only: it fences commits for a strict cut and never writes or claims
//! the legacy file's lease.

use super::*;
use parking_lot::FairMutexGuard;

/// The server's configured legacy writer, BORROWED by a pilot dataset.
///
/// A borrowed participant never writes through this handle and never claims
/// the legacy file's writer lease: the server keeps its writer and its write
/// path unchanged. `try_writer` is used only as the strict cut's commit fence
/// (the same non-blocking barrier an owned participant's writer provides);
/// `writer` is taken once at preparation to read identity and configuration.
pub trait LegacyWriterFence: Send + Sync {
    /// Non-blocking commit fence; `None` when the writer is busy.
    fn try_writer(&self) -> Option<FairMutexGuard<'_, Store>>;
    /// Blocking access for read-only preparation checks.
    fn writer(&self) -> FairMutexGuard<'_, Store>;
}

impl LegacyWriterFence for FairMutex<Store> {
    fn try_writer(&self) -> Option<FairMutexGuard<'_, Store>> {
        self.try_lock()
    }

    fn writer(&self) -> FairMutexGuard<'_, Store> {
        self.lock()
    }
}

/// An owner's writer: its own (the pilot file), or the borrowed legacy fence.
pub(super) enum Writer {
    // Boxed: a Store is large and the borrowed variant is one pointer.
    Owned(Box<FairMutex<Store>>),
    Borrowed(Arc<dyn LegacyWriterFence>),
}

impl Writer {
    /// Strict-cut commit fence for either kind; never used to write.
    pub(super) fn fence(&self) -> Option<FairMutexGuard<'_, Store>> {
        match self {
            Self::Owned(writer) => writer.try_lock(),
            Self::Borrowed(fence) => fence.try_writer(),
        }
    }

    /// The only write access. A borrowed legacy participant refuses, so no
    /// owner operation can write the legacy file through the pilot.
    pub(super) fn owned(&self) -> Result<FairMutexGuard<'_, Store>> {
        match self {
            Self::Owned(writer) => Ok(writer.lock()),
            Self::Borrowed(_) => Err(refused("legacy read participant is read-only")),
        }
    }

    #[cfg(test)]
    pub(super) fn lock(&self) -> FairMutexGuard<'_, Store> {
        match self {
            Self::Owned(writer) => writer.lock(),
            Self::Borrowed(fence) => fence.writer(),
        }
    }

    #[cfg(test)]
    pub(super) fn try_lock(&self) -> Option<FairMutexGuard<'_, Store>> {
        self.fence()
    }
}

impl Owner {
    /// Prepare the catalog's legacy participant as a READ-ONLY borrower of
    /// the server's configured legacy writer.
    ///
    /// The server keeps that writer and its write path: this claims no lease,
    /// applies no memory ceiling or embed deferral to the writer, and every
    /// owner write operation refuses. Only the dedicated read pool (opened
    /// read-only on the same file, sharing the writer's admission and read
    /// configuration) receives `allocation`. Strict cuts fence legacy commits
    /// through `LegacyWriterFence::try_writer`, never waiting on it.
    pub fn borrow_legacy(
        catalog: Arc<Catalog>,
        root: &Path,
        fence: Arc<dyn LegacyWriterFence>,
        installed_policy: &str,
        allocation: Option<crate::memory_budget::MemoryAllocation>,
    ) -> Result<Self> {
        let route = catalog
            .legacy_participant()
            .cloned()
            .ok_or_else(|| refused("explicit legacy read participant required"))?;
        if route.graph != crate::schema::ROOT_GRAPH_IRI {
            return Err(refused("legacy read participant must be ROOT"));
        }
        if installed_policy != route.policy {
            return Err(refused("installed policy differs from catalog"));
        }
        let (placement, readers) = {
            let writer = fence.writer();
            writer.require_ownership()?;
            let placement = Placement::observe(&writer)?;
            if !root.is_absolute()
                || root.canonicalize().map_err(refused)? != root
                || placement.path != root.join(format!("{}.db", route.file_id))
            {
                return Err(refused("owner file does not match catalog placement"));
            }
            Self::check_identity(&route, &writer)?;
            if writer.graph_class(0)?.as_deref() != Some("committed") {
                return Err(refused("owner graph must be committed"));
            }
            let path = placement
                .path
                .to_str()
                .ok_or_else(|| refused("native store path must be UTF-8"))?;
            let budget = catalog.budget();
            let mut readers = Vec::with_capacity(budget.readers_per_owner);
            for _ in 0..budget.readers_per_owner {
                let mut reader = Store::open_read_only(path)?;
                writer.share_admission_with(&mut reader)?;
                reader.adopt_read_config_from(&writer);
                if let Some(allocation) = allocation {
                    reader.apply_memory_allocation(allocation)?;
                }
                Self::check_identity(&route, &reader)?;
                readers.push(FairMutex::new(reader));
            }
            (placement, readers)
        };
        if readers.is_empty() {
            return Err(refused("legacy read participant needs a read pool"));
        }
        placement.verify()?;
        let capacity = catalog
            .budget()
            .queued_per_owner
            .checked_add(1)
            .ok_or_else(|| refused("mailbox bound overflow"))?;
        Ok(Self {
            catalog,
            route,
            writer: Writer::Borrowed(fence),
            readers,
            next_reader: AtomicUsize::new(0),
            admitted: AtomicUsize::new(0),
            capacity,
            placement,
        })
    }

    /// True for a borrowed legacy participant, which never writes.
    pub fn is_read_only(&self) -> bool {
        matches!(self.writer, Writer::Borrowed(_))
    }
}
