//! Payload-free stage observations for selected synchronous writer paths.
//!
//! This is log-only: it grants no admission, changes no deadline and never
//! catches a store error. A missing end event is incomplete evidence.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::time::Instant;

/// Closed observation paths; caller-controlled strings are never logged.
#[derive(Clone, Copy, Debug)]
pub enum Path {
    Knot,
    OwlPage,
    OwlFinish,
}

/// Closed stage names, with overlapping totals intentionally not additive.
#[derive(Clone, Copy, Debug)]
pub enum Stage {
    Dispatch,
    QueuedDispatch,
    LockWait,
    LockHeld,
    Tool,
    Deferred,
    Validate,
    Prepare,
    Remap,
    Transaction,
    Acknowledge,
    Guards,
    Release,
    Hooks,
}
impl Stage {
    fn name(self) -> &'static str {
        match self {
            Self::Dispatch => "dispatch",
            Self::QueuedDispatch => "admission_and_dispatch",
            Self::LockWait => "store_wait",
            Self::LockHeld => "store_held",
            Self::Tool => "tool",
            Self::Deferred => "deferred",
            Self::Validate => "validate",
            Self::Prepare => "private_prepare",
            Self::Remap => "remap",
            Self::Transaction => "transaction",
            Self::Acknowledge => "private_ack",
            Self::Guards => "guards",
            Self::Release => "release",
            Self::Hooks => "hooks",
        }
    }
}
#[derive(Clone)]
pub struct Observation(Arc<Inner>);
struct Inner {
    id: u64,
    path: Path,
    request: Option<u64>,
    started: Instant,
    durations: Mutex<BTreeMap<&'static str, f64>>,
    page: Mutex<Option<serde_json::Value>>,
}
thread_local! { static CURRENT: RefCell<Option<Observation>> = const { RefCell::new(None) }; }
static NEXT: AtomicU64 = AtomicU64::new(1);

impl Observation {
    /// Start before scheduling, preserving the server's own request ID if known.
    #[must_use]
    pub fn new(path: Path, request: Option<u64>) -> Self {
        Self(Arc::new(Inner {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            path,
            request,
            started: Instant::now(),
            durations: Mutex::new(BTreeMap::new()),
            page: Mutex::new(None),
        }))
    }
    fn record(&self, event: &str, stage: Option<Stage>, outcome: &str) -> serde_json::Value {
        serde_json::json!({"event": event, "observation_id": self.0.id,
            "path": format!("{:?}", self.0.path), "request_id": self.0.request,
            "server_process_id": std::process::id(), "timestamp": crate::time::now_iso(),
            "elapsed_ms": self.0.started.elapsed().as_secs_f64()*1000.0,
            "stage": stage.map(Stage::name), "outcome": outcome,
            "durations_ms": *self.0.durations.lock().unwrap_or_else(|e| e.into_inner()),
            "page": *self.0.page.lock().unwrap_or_else(|e| e.into_inner())})
    }
    /// Separate admission from blocking dispatch using its observed grant.
    pub fn admission_granted(&self, granted: Option<Instant>) {
        if let Some(granted) = granted {
            let mut d = self.0.durations.lock().unwrap_or_else(|e| e.into_inner());
            d.insert(
                "admission",
                granted
                    .saturating_duration_since(self.0.started)
                    .as_secs_f64()
                    * 1000.0,
            );
            d.insert("dispatch", granted.elapsed().as_secs_f64() * 1000.0);
        }
    }
    /// Run on the actual blocking thread. Nested scopes restore their parent.
    pub fn run<T, E>(&self, f: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
        let previous = CURRENT.with(|c| c.replace(Some(self.clone())));
        let mut scope = Scope {
            current: self.clone(),
            previous,
            outcome: "interrupted",
        };
        let result = f();
        scope.outcome = if result.is_ok() { "ok" } else { "error" };
        result
    }
    /// A stage can cross scheduling boundaries without losing its correlation.
    #[must_use]
    pub fn stage(&self, stage: Stage) -> Span {
        eprintln!(
            "{}",
            self.record("write_stage_start", Some(stage), "started")
        );
        Span {
            observation: Some(self.clone()),
            stage,
            started: Instant::now(),
            outcome: "interrupted",
        }
    }
}
struct Scope {
    current: Observation,
    previous: Option<Observation>,
    outcome: &'static str,
}
impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.with(|c| {
            c.replace(self.previous.take());
        });
        eprintln!(
            "{}",
            self.current
                .record("write_observation_end", None, self.outcome)
        );
    }
}
/// Error-safe stage timer. An unfinished scope is explicitly interrupted.
pub struct Span {
    observation: Option<Observation>,
    stage: Stage,
    started: Instant,
    outcome: &'static str,
}
impl Span {
    /// End only after the named operation (and, for held time, the guard drop).
    pub fn finish(mut self, success: bool) {
        self.outcome = if success { "ok" } else { "error" };
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        if let Some(o) = &self.observation {
            *o.0.durations
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entry(self.stage.name())
                .or_default() += self.started.elapsed().as_secs_f64() * 1000.0;
            eprintln!(
                "{}",
                o.record("write_stage_end", Some(self.stage), self.outcome)
            );
        }
    }
}
/// Observe only an explicitly scoped operation; unrelated writers do no logging.
#[must_use]
pub fn stage(stage: Stage) -> Span {
    CURRENT
        .with(|c| c.borrow().as_ref().map(|o| o.stage(stage)))
        .unwrap_or(Span {
            observation: None,
            stage,
            started: Instant::now(),
            outcome: "interrupted",
        })
}
/// Preserve the original result, including every early error.
pub fn observe<T, E>(name: Stage, f: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
    let span = stage(name);
    let result = f();
    span.finish(result.is_ok());
    result
}

/// Attach bounded internal cursor metadata, never semantic strings or paths.
pub fn page(generation: &str, start: i64, end: i64, count: usize) {
    let generation = generation.strip_prefix("urn:uuid:").unwrap_or(generation);
    if generation.len() != 36
        || !generation.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
    {
        return;
    }
    if start < 0 || end < start || count > 64 {
        return;
    }
    CURRENT.with(|c| {
        if let Some(o) = c.borrow().as_ref() {
            *o.0.page.lock().unwrap_or_else(|e| e.into_inner()) = Some(serde_json::json!({
                "generation": generation.to_ascii_lowercase(), "cursor_start": start,
                "cursor_end": end, "keys": count,
            }));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_is_bounded_and_queue_grant_is_measured_separately() {
        let o = Observation::new(Path::OwlPage, Some(3));
        o.admission_granted(None);
        assert!(o.0.durations.lock().unwrap().is_empty());
        o.admission_granted(Some(Instant::now()));
        o.run(|| -> Result<(), ()> {
            page("secret-payload", 0, 64, 64);
            assert!(o.record("test", None, "ok")["page"].is_null());
            page("urn:uuid:79aa55ce-68c9-49e5-96b7-43bb457aa33d", 0, 64, 64);
            Ok(())
        })
        .unwrap();
        let r = o.record("test", None, "ok");
        assert_eq!(r["page"]["keys"], 64);
        assert!(r["durations_ms"]["admission"].as_f64().unwrap() >= 0.0);
        assert!(r["durations_ms"]["dispatch"].as_f64().unwrap() >= 0.0);
        assert!(!r.to_string().contains("secret-payload"));
    }
    #[test]
    fn real_normal_transaction_records_guards_release_and_hooks() {
        let mut store = crate::Store::open_in_memory_test_ns().unwrap();
        let o = Observation::new(Path::Knot, Some(11));
        o.run(|| {
            crate::tool_knot(
                &mut store,
                &serde_json::json!({
                    "turtle": "<urn:observation:e> <urn:observation:p> \"value\" ."
                }),
            )
        })
        .unwrap();
        let r = o.record("test", None, "ok");
        for name in ["guards", "release", "hooks"] {
            assert!(r["durations_ms"][name].as_f64().unwrap() >= 0.0);
        }
        assert!(!r.to_string().contains("urn:observation"));
    }
    #[test]
    fn errors_keep_stage_and_restore_nested_context() {
        let outer = Observation::new(Path::Knot, Some(7));
        let inner = Observation::new(Path::OwlPage, None);
        outer
            .run(|| -> Result<(), ()> {
                assert!(
                    inner
                        .run(|| observe(Stage::Release, || Err::<(), _>(())))
                        .is_err()
                );
                observe(Stage::Guards, || Ok::<(), ()>(()))?;
                Ok(())
            })
            .unwrap();
        assert!(outer.0.durations.lock().unwrap().contains_key("guards"));
        assert!(!outer.0.durations.lock().unwrap().contains_key("release"));
        assert!(inner.0.durations.lock().unwrap().contains_key("release"));
        assert!(CURRENT.with(|c| c.borrow().is_none()));
    }
    #[test]
    fn unwind_restores_context_and_no_payload_fields_exist() {
        let o = Observation::new(Path::Knot, None);
        let _ = std::panic::catch_unwind(|| {
            o.run(|| -> Result<(), ()> {
                let _span = stage(Stage::LockHeld);
                panic!("fixture");
            })
        });
        assert!(CURRENT.with(|c| c.borrow().is_none()));
        let record = o.record("write_observation_end", None, "interrupted");
        assert!(record["request_id"].is_null());
        assert!(record["durations_ms"]["store_held"].as_f64().unwrap() >= 0.0);
        assert!(record.get("input").is_none());
        assert!(record.get("authorization").is_none());
    }
}
