//! Bounded native observation boundary, without writer SQL or runtime enrollment.
//!
//! Native calls remain unarmed. The current body supplies no authenticated base,
//! preimages, profile or lineage bundle: every captured row explicitly says so.
//! Native RELEASE evidence is not a verified durable receipt or corpus coverage.
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[cfg(test)]
pub(super) mod tests;

const CAPACITY: usize = 64;

/// Native completion evidence, kept separate from client response delivery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Completion {
    /// The outer release succeeded with the already obtained native data tx.
    NativeRelease(i64),
    /// The native primitive settled without a data transaction.
    NoData,
    /// Native rollback succeeded. This is not an accepted write.
    RolledBack,
    /// Settlement could not be established.
    Unknown,
}

/// Value-free partial observation; never a three-way classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observation {
    /// Existing native completion evidence, not authenticated receipt recovery.
    pub completion: Completion,
    /// No native preimage bundle was supplied; no lookup was attempted.
    pub preimages_unknown: bool,
    /// No authenticated historical base was supplied.
    pub base_unknown: bool,
    /// No loaded profile witness was supplied.
    pub profile_unknown: bool,
    /// No store/epoch/principal bundle was supplied.
    pub identity_unknown: bool,
}

/// Independently sampled, saturating loss counters. Process death can lose them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Health {
    /// Queue full, busy, poisoned, or request budget exceeded.
    pub dropped: u64,
    /// An owned stage was dropped without known native settlement.
    pub abandoned: u64,
    /// Native body refused before an observation handoff existed.
    pub before_handoff: u64,
}

/// Nonblocking read verdict; contention and poison never mean an empty queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Read {
    /// One partial native observation.
    Observation(Observation),
    /// The queue was read and was empty.
    Empty,
    /// The queue could not be read.
    Unavailable,
}

/// Fixed-capacity, nonwaiting source-stage sink; not a durable outcome ledger.
///
/// Creating this object does not enroll a store or arm a transport. There is no
/// production setter, environment switch, schema migration or scheduled reader.
#[derive(Default)]
pub struct Observer {
    queue: Mutex<VecDeque<Observation>>,
    dropped: AtomicU64,
    abandoned: AtomicU64,
    before_handoff: AtomicU64,
}

fn increment(value: &AtomicU64) {
    let _ = value.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
        Some(n.saturating_add(1))
    });
}

impl Observer {
    /// Read independent loss counters; zero is not complete process coverage.
    #[must_use]
    pub fn health(&self) -> Health {
        Health {
            dropped: self.dropped.load(Ordering::Relaxed),
            abandoned: self.abandoned.load(Ordering::Relaxed),
            before_handoff: self.before_handoff.load(Ordering::Relaxed),
        }
    }

    /// Consume one observation without waiting; unavailable is not empty.
    #[must_use]
    pub fn try_take(&self) -> Read {
        match self.queue.try_lock() {
            Ok(mut queue) => queue.pop_front().map_or(Read::Empty, Read::Observation),
            Err(_) => Read::Unavailable,
        }
    }

    fn record(&self, observation: Observation) {
        if let Ok(mut queue) = self.queue.try_lock()
            && queue.len() < CAPACITY
        {
            queue.push_back(observation);
            return;
        }
        increment(&self.dropped);
    }
}

/// An owned observation created only after native reservation/body completion.
/// No Store, Connection, SQL callback, terms or request strings can enter it.
pub(super) struct Stage {
    observer: Option<Arc<Observer>>,
}

impl Stage {
    pub(super) fn capture(observer: Option<Arc<Observer>>, datum_count: usize) -> Self {
        // No additional copying, classification or SQL: the native body has no
        // evidence bundle. Count only a bounded unknown request observation.
        let observer = observer.filter(|sink| {
            if datum_count > 64 {
                increment(&sink.dropped);
                false
            } else {
                true
            }
        });
        Self { observer }
    }

    pub(super) fn finish(mut self, completion: Completion) {
        if let Some(observer) = self.observer.take() {
            let completion = match completion {
                Completion::NativeRelease(tx) if tx <= 0 => Completion::Unknown,
                other => other,
            };
            observer.record(Observation {
                completion,
                preimages_unknown: true,
                base_unknown: true,
                profile_unknown: true,
                identity_unknown: true,
            });
        }
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        if let Some(observer) = self.observer.take() {
            increment(&observer.abandoned);
        }
    }
}

// Only disposable controls can arm the normal native path in this source slice.
// No runtime route is registered; a later live rollout needs separate authority.
#[cfg(test)]
thread_local! {
    static TEST_OBSERVER: std::cell::RefCell<Option<Arc<Observer>>> = const { std::cell::RefCell::new(None) };
}

pub(super) fn current() -> Option<Arc<Observer>> {
    #[cfg(test)]
    {
        TEST_OBSERVER.with(|value| value.borrow().clone())
    }
    #[cfg(not(test))]
    {
        None
    }
}

pub(super) fn before_handoff() {
    if let Some(observer) = current() {
        increment(&observer.before_handoff);
    }
}

pub(super) fn write_time() -> String {
    #[cfg(test)]
    if tests::fixed_clock() {
        return "2026-01-01T00:00:00Z".into();
    }
    crate::time::now_iso()
}
