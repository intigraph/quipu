//! Actual writer-connection occupancy, independent of admission or HTTP lifetime.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

type Totals = BTreeMap<(String, &'static str), (u64, f64)>;

#[derive(Default)]
pub(crate) struct Registry(Mutex<Totals>);

pub(crate) fn registry() -> &'static Arc<Registry> {
    static REGISTRY: OnceLock<Arc<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Arc::new(Registry::default()))
}

impl Registry {
    pub(crate) fn render(&self, out: &mut String) {
        out.push_str("# HELP quipu_writer_lock_held_seconds_total Actual writer connection occupancy; excludes acquisition wait and pooled readers.\n# TYPE quipu_writer_lock_held_seconds_total counter\n# HELP quipu_writer_lock_acquisitions_total Actual writer connection acquisitions.\n# TYPE quipu_writer_lock_acquisitions_total counter\n");
        for ((endpoint, access), (count, seconds)) in self.0.lock().unwrap().iter() {
            let _ = writeln!(
                out,
                "quipu_writer_lock_held_seconds_total{{endpoint=\"{endpoint}\",access=\"{access}\"}} {seconds}"
            );
            let _ = writeln!(
                out,
                "quipu_writer_lock_acquisitions_total{{endpoint=\"{endpoint}\",access=\"{access}\"}} {count}"
            );
        }
    }
}

#[derive(Clone)]
pub(crate) struct Context {
    endpoint: Arc<str>,
    registry: Arc<Registry>,
}

impl Context {
    /// Only a server-owned matched route template is passed here, never a URI.
    pub(crate) fn route(endpoint: &str) -> Self {
        Self {
            endpoint: endpoint.into(),
            registry: registry().clone(),
        }
    }
}

thread_local! { static CONTEXT: RefCell<Option<Context>> = const { RefCell::new(None) }; }

pub(crate) fn scoped<T>(context: Option<Context>, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<Context>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CONTEXT.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(CONTEXT.with(|slot| slot.replace(context)));
    f()
}

/// Capture HTTP attribution before detaching a blocking job from its handler.
pub(crate) fn spawn_blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> tokio::task::JoinHandle<T> {
    let context = crate::request_middleware::writer_context();
    tokio::task::spawn_blocking(move || scoped(context, f))
}

/// The raw guard unlocks before metrics aggregation, including on unwind.
pub(crate) struct StoreGuard<'a> {
    guard: Option<parking_lot::FairMutexGuard<'a, quipu::Store>>,
    timing: Option<(Instant, Context, &'static str)>,
}

impl<'a> StoreGuard<'a> {
    pub(crate) fn writer(
        guard: parking_lot::FairMutexGuard<'a, quipu::Store>,
        access: &'static str,
    ) -> Self {
        let started = Instant::now();
        let context = CONTEXT
            .with(|slot| slot.borrow().clone())
            .unwrap_or_else(|| Context::route("internal"));
        Self {
            guard: Some(guard),
            timing: Some((started, context, access)),
        }
    }
    pub(crate) fn read_fallback(mut self) -> Self {
        if let Some((_, _, access)) = self.timing.as_mut() {
            *access = "read-fallback";
        }
        self
    }
    pub(crate) fn reader(guard: parking_lot::FairMutexGuard<'a, quipu::Store>) -> Self {
        Self {
            guard: Some(guard),
            timing: None,
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
        // Measure before unlock; the registry mutex never extends the store hold.
        let elapsed = self
            .timing
            .as_ref()
            .map(|(started, _, _)| started.elapsed().as_secs_f64());
        drop(self.guard.take());
        if let Some((_, context, access)) = self.timing.take() {
            let mut entries = context.registry.0.lock().unwrap();
            let entry = entries
                .entry((context.endpoint.to_string(), access))
                .or_default();
            entry.0 += 1;
            entry.1 += elapsed.unwrap();
        }
    }
}

#[cfg(test)]
#[path = "writer_timing_tests.rs"]
mod tests;
