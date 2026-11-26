//! Per-owner async admission for the inert graph-owner adapter. Construction
//! requires an initializer's already reserved catalog budget; this module
//! creates no owner, runtime route or resource allocation policy.
#![cfg_attr(not(test), allow(dead_code))]
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Instant;
use tokio::sync::Semaphore;

use super::{
    AppError, request_budget,
    tracked::{admit_tracked_owned, admit_tracked_permit},
};

/// One actor's single writer and bounded active+waiting population. The
/// initializer must create exactly one of these per frozen owner route.
pub(crate) struct OwnerWriteAdmission {
    semaphore: Arc<Semaphore>,
    reserved: AtomicUsize,
    capacity: usize,
}

struct Reservation(Arc<OwnerWriteAdmission>);

impl Drop for Reservation {
    fn drop(&mut self) {
        self.0.reserved.fetch_sub(1, Ordering::AcqRel);
    }
}

impl OwnerWriteAdmission {
    pub(crate) fn new(queued_per_owner: usize) -> Result<Self, AppError> {
        let capacity = queued_per_owner
            .checked_add(1)
            .filter(|_| queued_per_owner > 0)
            .ok_or_else(|| quipu::Error::InvalidValue("invalid owner queue reservation".into()))?;
        Ok(Self {
            semaphore: Arc::new(Semaphore::new(1)),
            reserved: AtomicUsize::new(0),
            capacity,
        })
    }

    fn reserve(self: &Arc<Self>) -> Result<Reservation, AppError> {
        let mut current = self.reserved.load(Ordering::Acquire);
        loop {
            if current >= self.capacity {
                return Err(quipu::Error::WriteAdmissionTimeout { waited_ms: 0 }.into());
            }
            match self.reserved.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(Reservation(self.clone())),
                Err(actual) => current = actual,
            }
        }
    }

    /// Cancellation while queued drops the reservation. After dispatch both
    /// reservation and semaphore permit belong to the actual blocking worker,
    /// including all deferred work and receipt finalization inside `work`.
    pub(crate) async fn write<T, F>(self: &Arc<Self>, work: F) -> Result<T, AppError>
    where
        F: FnOnce() -> Result<T, AppError> + Send + 'static,
        T: Send + 'static,
    {
        let started = Instant::now();
        self.write_with_deadline(request_budget(), started, work)
            .await
    }

    async fn write_with_deadline<T, F>(
        self: &Arc<Self>,
        deadline: Option<quipu::time::Deadline>,
        started: Instant,
        work: F,
    ) -> Result<T, AppError>
    where
        F: FnOnce() -> Result<T, AppError> + Send + 'static,
        T: Send + 'static,
    {
        let reservation = self.reserve()?;
        admit_tracked_owned(
            self.semaphore.clone(),
            "write",
            deadline,
            started,
            None,
            move || {
                let _reservation = reservation;
                work()
            },
            quipu::metrics::inflight::store().clone(),
        )
        .await
    }
}

/// Owner status reads reserve an existing reader before spawning. There is
/// no unbounded queue and no borrowing of the legacy point-read semaphore.
pub(crate) async fn read_now<T, F>(readers: Arc<Semaphore>, work: F) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    let started = Instant::now();
    let deadline = request_budget();
    let permit = readers
        .try_acquire_owned()
        .map_err(|_| quipu::Error::InvalidValue("owner status read pool busy or closed".into()))?;
    admit_tracked_permit(
        std::future::ready(Ok(permit)),
        "point-read",
        deadline,
        started,
        None,
        work,
        quipu::metrics::inflight::store().clone(),
    )
    .await
}

#[cfg(test)]
#[path = "owner_tests.rs"]
mod tests;
