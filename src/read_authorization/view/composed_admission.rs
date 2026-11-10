//! Request scratch admission sized from the same catalog/service allocation.

use std::sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

use super::denied;
use crate::{Result, graph_catalog::Catalog, memory_budget::MemoryAllocation};

/// Explicit trusted-initializer sizing, qualified separately against the actual
/// workload and service cgroup. Counts reservations, not allocator/RSS usage.
#[derive(Debug, Clone, Copy)]
pub struct ComposedReadReservation {
    /// Runtime/other-evaluator share kept outside this admission handle.
    pub runtime_bytes: u64,
    /// Scratch construction, dictionaries and resident models held by a view.
    pub view_bytes: u64,
    /// Evaluator plus serialized output share for each outstanding query.
    pub query_bytes: u64,
    /// Hard serialized-body ceiling, enforced by the output writer.
    pub max_response_bytes: usize,
}

struct Shared {
    catalog: Arc<Catalog>,
    allocation: MemoryAllocation,
    limit: usize,
    active: AtomicUsize,
    reservation: ComposedReadReservation,
    heap_capacity: u64,
    heap_reserved: AtomicU64,
}

/// ONE trusted-initializer-created handle, cloned across requests. Counts live
/// scratch connections within the service allocation, not threads or processes.
/// Initializers must separately drain overlapping catalog generations and qualify
/// reservation sizes under the service cgroup; this is not an allocator/RSS meter.
#[derive(Clone)]
pub struct ComposedReadAdmission(Arc<Shared>);

/// A non-clonable live scratch reservation. Retained by the actual view owner.
pub struct ComposedReadPermit {
    shared: Arc<Shared>,
    _view: HeapLease,
}

pub(super) struct HeapLease(Arc<Shared>, u64);

/// Buffered complete response. Retains its query reservation after the view is
/// dropped, until the last owned output is consumed. Copies made by consumers
/// require their own accounting; the raw Vec cannot be extracted here.
pub struct ComposedQueryOutput {
    bytes: Vec<u8>,
    content_type: &'static str,
    _lease: HeapLease,
}

impl ComposedQueryOutput {
    pub(super) fn new(response: (&'static str, Vec<u8>), lease: HeapLease) -> Self {
        Self {
            content_type: response.0,
            bytes: response.1,
            _lease: lease,
        }
    }
    /// Borrow the complete serialized response without relinquishing its lease.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Serializer-selected media type; adapters must not sniff response bytes.
    pub fn content_type(&self) -> &'static str {
        self.content_type
    }
}
impl std::ops::Deref for ComposedQueryOutput {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        self.as_bytes()
    }
}

impl Shared {
    fn reserve(self: &Arc<Self>, bytes: u64) -> Result<HeapLease> {
        self.heap_reserved
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                active
                    .checked_add(bytes)
                    .filter(|n| *n <= self.heap_capacity)
            })
            .map_err(|_| denied())?;
        Ok(HeapLease(self.clone(), bytes))
    }
}
impl Drop for HeapLease {
    fn drop(&mut self) {
        self.0.heap_reserved.fetch_sub(self.1, Ordering::AcqRel);
    }
}

impl ComposedReadAdmission {
    /// Count legacy, all owner/fence/read-pool and maximum concurrent scratch
    /// connections together. No default/unlimited mode or fallback admission.
    pub fn new(
        catalog: Arc<Catalog>,
        mib: u64,
        legacy_connections: u64,
        scratch_limit: usize,
        reservation: ComposedReadReservation,
    ) -> Result<Self> {
        if scratch_limit == 0 {
            return Err(denied());
        }
        let count = u64::try_from(scratch_limit).map_err(|_| denied())?;
        let allocation = catalog.memory_allocation(mib, legacy_connections, count)?;
        let heap_capacity = (allocation.total_bytes / 2)
            .checked_sub(reservation.runtime_bytes)
            .ok_or_else(denied)?;
        let demand = reservation
            .view_bytes
            .checked_add(reservation.query_bytes)
            .and_then(|n| n.checked_mul(count))
            .ok_or_else(denied)?;
        if reservation.runtime_bytes == 0
            || reservation.view_bytes == 0
            || reservation.query_bytes == 0
            || reservation.max_response_bytes == 0
            || u64::try_from(reservation.max_response_bytes).map_err(|_| denied())?
                > reservation.query_bytes
            || demand > heap_capacity
        {
            return Err(denied());
        }
        Ok(Self(Arc::new(Shared {
            catalog,
            allocation,
            limit: scratch_limit,
            active: AtomicUsize::new(0),
            reservation,
            heap_capacity,
            heap_reserved: AtomicU64::new(0),
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
        let view = self.0.reserve(self.0.reservation.view_bytes)?;
        self.0
            .active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < self.0.limit).then(|| n + 1)
            })
            .map_err(|_| denied())?;
        Ok(ComposedReadPermit {
            shared: self.0.clone(),
            _view: view,
        })
    }
}

impl ComposedReadPermit {
    pub(super) fn reserve_query(&self) -> Result<HeapLease> {
        self.shared.reserve(self.shared.reservation.query_bytes)
    }
    pub(super) fn max_response_bytes(&self) -> usize {
        self.shared.reservation.max_response_bytes
    }

    pub(super) fn allocation(&self) -> MemoryAllocation {
        self.shared.allocation
    }

    pub(super) fn matches(&self, read: &crate::store::graph_owner::CapturedRead<'_>) -> bool {
        self.shared.catalog.revision() == read.catalog_revision
            && self
                .shared
                .catalog
                .routes()
                .chain(self.shared.catalog.legacy_participant())
                .any(|route| route == read.route)
    }
}

impl Drop for ComposedReadPermit {
    fn drop(&mut self) {
        self.shared.active.fetch_sub(1, Ordering::AcqRel);
    }
}
