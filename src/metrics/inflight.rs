//! Pending HTTP requests and admitted store work have different lifetimes.
//!
//! HTTP cancellation drops its guard. Store work moves its guard into the
//! blocking closure, so an abandoned client cannot conceal a running write.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

/// Closed lane vocabulary; never derived from caller-controlled labels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    /// Mutating request or write admission.
    Write,
    /// Ordinary read.
    Read,
    /// Reserved point-read admission (store scope only).
    PointRead,
    /// Unclassified HTTP route.
    Other,
}

impl Lane {
    const ALL: [Self; 4] = [Self::Write, Self::Read, Self::PointRead, Self::Other];

    const fn index(self) -> usize {
        self as usize
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Read => "read",
            Self::PointRead => "point-read",
            Self::Other => "other",
        }
    }
}

/// Waiting includes admission and blocking-pool queueing; active means executing.
#[derive(Clone, Copy)]
pub enum Phase {
    /// Waiting for admission or for a blocking thread.
    Waiting,
    /// Executing within a handler or blocking closure.
    Active,
}

struct Entry {
    lane: Lane,
    phase: Phase,
    started: Instant,
}

/// Registry bounded by actual concurrent work, not the historical request count.
#[derive(Default)]
pub struct Registry {
    next: AtomicU64,
    entries: Mutex<BTreeMap<u64, Entry>>,
}

/// Owning RAII token: removes its entry on success, error, cancellation or unwind.
#[must_use = "keep the guard alive for the full tracked work lifetime"]
pub struct Guard {
    registry: Arc<Registry>,
    id: u64,
}

impl Registry {
    /// Register work before it can wait. The token may move across thread hops.
    pub fn start(self: &Arc<Self>, lane: Lane, phase: Phase) -> Guard {
        self.start_at(lane, phase, Instant::now())
    }

    fn start_at(self: &Arc<Self>, lane: Lane, phase: Phase, started: Instant) -> Guard {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.entries.lock().unwrap().insert(
            id,
            Entry {
                lane,
                phase,
                started,
            },
        );
        Guard {
            registry: self.clone(),
            id,
        }
    }

    fn snapshot(&self, now: Instant) -> ([[usize; 2]; 4], f64) {
        let entries = self.entries.lock().unwrap();
        let mut counts = [[0; 2]; 4];
        let mut oldest = 0.0_f64;
        for entry in entries.values() {
            counts[entry.lane.index()][entry.phase as usize] += 1;
            if entry.lane == Lane::Write {
                oldest = oldest.max(now.saturating_duration_since(entry.started).as_secs_f64());
            }
        }
        (counts, oldest)
    }

    /// Render one consistent registry snapshot without acquiring a store lock.
    pub fn render(&self, out: &mut String, store: bool) {
        let (counts, oldest) = self.snapshot(Instant::now());
        let scope = if store { "store" } else { "http" };
        let _ = writeln!(
            out,
            "# HELP quipu_{scope}_requests_in_flight Pending work by closed lane{}; process-local, reset on restart.",
            if store { " and phase" } else { "" }
        );
        let _ = writeln!(out, "# TYPE quipu_{scope}_requests_in_flight gauge");
        for lane in Lane::ALL {
            if store {
                if lane == Lane::Other {
                    continue;
                }
                for (phase, count) in ["waiting", "active"].into_iter().zip(counts[lane.index()]) {
                    let _ = writeln!(
                        out,
                        "quipu_store_requests_in_flight{{lane=\"{}\",phase=\"{phase}\"}} {count}",
                        lane.label()
                    );
                }
            } else {
                if lane == Lane::PointRead {
                    continue;
                }
                let count: usize = counts[lane.index()].iter().sum();
                let _ = writeln!(
                    out,
                    "quipu_http_requests_in_flight{{lane=\"{}\"}} {count}",
                    lane.label()
                );
            }
        }
        let _ = writeln!(
            out,
            "# HELP quipu_{scope}_oldest_write_age_seconds Age since the oldest pending write began in this scope; zero when idle, not writer-lock hold time."
        );
        let _ = writeln!(out, "# TYPE quipu_{scope}_oldest_write_age_seconds gauge");
        let _ = writeln!(out, "quipu_{scope}_oldest_write_age_seconds {oldest}");
    }
}

impl Guard {
    /// Set only inside the executing blocking closure, not on permit acquisition.
    pub fn activate(&self) {
        if let Some(entry) = self.registry.entries.lock().unwrap().get_mut(&self.id) {
            entry.phase = Phase::Active;
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.registry.entries.lock().unwrap().remove(&self.id);
    }
}

/// HTTP handler lifetime until response production, not streaming-body lifetime.
/// Includes admission wait but excludes detached blocking work.
pub fn http() -> &'static Arc<Registry> {
    static REGISTRY: OnceLock<Arc<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}

/// Admission queue plus actual blocking work, even after the client disconnects.
pub fn store() -> &'static Arc<Registry> {
    static REGISTRY: OnceLock<Arc<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}

#[cfg(test)]
#[path = "inflight_tests.rs"]
mod tests;
