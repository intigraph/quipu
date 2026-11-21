//! Completed explicit-writer holds, including cancelled HTTP/detached work.
//! Uses existing write provenance carried to blocking/deep-stack/job threads.
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::ops::{Deref, DerefMut};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

type Key = (String, String);
type Totals = BTreeMap<Key, (u64, f64)>;
const MAX_SERIES: usize = 256;

#[derive(Default)]
struct Registry(Mutex<Totals>);

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Registry::default)
}

impl Registry {
    fn observe(&self, key: Key, seconds: f64) {
        // Caller labels are cooperative attribution, never authentication.
        // Bound series even when clients supply endlessly distinct labels.
        let mut entries = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = if entries.contains_key(&key) || entries.len() < MAX_SERIES {
            key
        } else {
            ("overflow".into(), "other".into())
        };
        let entry = entries.entry(key).or_default();
        entry.0 += 1;
        entry.1 += seconds;
    }

    fn render(&self, out: &mut String) {
        out.push_str("# HELP quipu_writer_lock_held_seconds_total Completed explicit writer holds; excludes acquisition wait, pooled reads and post-unlock work.\n# TYPE quipu_writer_lock_held_seconds_total counter\n# HELP quipu_writer_lock_acquisitions_total Completed explicit writer acquisitions.\n# TYPE quipu_writer_lock_acquisitions_total counter\n");
        let entries = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for ((endpoint, client), (count, seconds)) in entries.iter() {
            let _ = writeln!(
                out,
                "quipu_writer_lock_held_seconds_total{{endpoint=\"{endpoint}\",client=\"{client}\"}} {seconds}"
            );
            let _ = writeln!(
                out,
                "quipu_writer_lock_acquisitions_total{{endpoint=\"{endpoint}\",client=\"{client}\"}} {count}"
            );
        }
    }
}

pub(crate) fn render(out: &mut String) {
    registry().render(out);
}

fn labels() -> Key {
    quipu::write_provenance::current().map_or_else(
        || ("internal".into(), "unattributed".into()),
        |p| {
            // Only fixed route names, never payload paths or dynamic identifiers.
            let route = match p.endpoint.as_str() {
                "/knot" => "/knot",
                "/knot/promote" => "/knot/promote",
                "/import/promote" => "/import/promote",
                _ => "other",
            };
            (
                route.into(),
                quipu::metrics::normalize_client(Some(&p.client), None),
            )
        },
    )
}

/// Own the actual guard so unlock always precedes registry aggregation.
/// Early returns and unwinds report holds too; acquisition wait is excluded.
pub(crate) struct StoreGuard<'a> {
    guard: Option<parking_lot::FairMutexGuard<'a, quipu::Store>>,
    started: Instant,
    key: Key,
}
impl<'a> StoreGuard<'a> {
    pub(crate) fn new(guard: parking_lot::FairMutexGuard<'a, quipu::Store>) -> Self {
        let started = Instant::now();
        Self {
            guard: Some(guard),
            started,
            key: labels(),
        }
    }
}
impl Deref for StoreGuard<'_> {
    type Target = quipu::Store;
    fn deref(&self) -> &Self::Target {
        self.guard.as_ref().unwrap()
    }
}
impl DerefMut for StoreGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard.as_mut().unwrap()
    }
}
impl Drop for StoreGuard<'_> {
    fn drop(&mut self) {
        let seconds = self.started.elapsed().as_secs_f64();
        drop(self.guard.take());
        registry().observe(std::mem::take(&mut self.key), seconds);
    }
}

#[cfg(test)]
#[path = "writer_timing_tests.rs"]
mod tests;
