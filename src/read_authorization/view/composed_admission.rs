//! Request scratch admission sized from the same catalog/service allocation.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use super::denied;
use crate::{Result, graph_catalog::Catalog, memory_budget::MemoryAllocation};

struct Shared {
    catalog: Arc<Catalog>,
    allocation: MemoryAllocation,
    limit: usize,
    active: AtomicUsize,
}

/// ONE trusted-initializer-created handle, cloned across requests. Counts live
/// scratch connections within the service allocation, not threads or processes.
/// Initializers must separately drain overlapping catalog generations and reserve
/// evaluator/RSS capacity under the service cgroup; this is not a heap meter.
#[derive(Clone)]
pub struct ComposedReadAdmission(Arc<Shared>);

/// A non-clonable live scratch reservation. Retained by the actual view owner.
pub struct ComposedReadPermit(Arc<Shared>);

impl ComposedReadAdmission {
    /// Count legacy, all owner/fence/read-pool and maximum concurrent scratch
    /// connections together. No default/unlimited mode or fallback admission.
    pub fn new(
        catalog: Arc<Catalog>,
        mib: u64,
        legacy_connections: u64,
        scratch_limit: usize,
    ) -> Result<Self> {
        if scratch_limit == 0 {
            return Err(denied());
        }
        let count = u64::try_from(scratch_limit).map_err(|_| denied())?;
        let allocation = catalog.memory_allocation(mib, legacy_connections, count)?;
        Ok(Self(Arc::new(Shared {
            catalog,
            allocation,
            limit: scratch_limit,
            active: AtomicUsize::new(0),
        })))
    }

    /// The SAME allocation applies to each native owner, legacy and scratch
    /// connection. Creating a handle does not change existing connections.
    pub fn allocation(&self) -> MemoryAllocation {
        self.0.allocation
    }

    /// Reserve BEFORE strict capture. Exhaustion refuses immediately without
    /// queueing on a writer or borrowing another owner's request reservation.
    pub fn try_reserve(&self) -> Result<ComposedReadPermit> {
        self.0
            .active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < self.0.limit).then(|| n + 1)
            })
            .map_err(|_| denied())?;
        Ok(ComposedReadPermit(self.0.clone()))
    }
}

impl ComposedReadPermit {
    pub(super) fn allocation(&self) -> MemoryAllocation {
        self.0.allocation
    }

    pub(super) fn matches(&self, read: &crate::store::graph_owner::CapturedRead<'_>) -> bool {
        self.0.catalog.revision() == read.catalog_revision
            && self.0.catalog.routes().any(|route| route == read.route)
    }
}

impl Drop for ComposedReadPermit {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::AcqRel);
    }
}
