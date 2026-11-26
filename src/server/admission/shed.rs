//! Early read shedding with a client priority class (aegis-69x).
//!
//! WHY: an ordinary read that cannot get a permit used to wait on the read
//! semaphore with no admission timeout, spend its whole request budget in the
//! queue and then fail `RequestTimeout` (408, no Retry-After). A caller learned
//! nothing it could act on and every retry joined the same queue. Writes already
//! answer `503 + Retry-After` before they start (`write_not_started`).
//!
//! With `[quipu.server] read_admission_wait_ms > 0` a read that is not admitted
//! within that wait (or the remaining request budget, whichever is shorter) is
//! refused BEFORE it runs: 429 + Retry-After, body `read_not_started`. A read
//! declared `X-Quipu-Priority: background` never queues and never takes the last
//! `read_background_reserve` permits, so non-critical pollers (status segments,
//! dashboards) yield to interactive reads.
//!
//! OFF BY DEFAULT: with `read_admission_wait_ms = 0` nothing here changes
//! admission and the header is ignored. The priority header can only LOWER a
//! request's class; an absent or unknown value is `normal`. It is scheduling
//! metadata, never authorization.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Header a caller sets to declare its read as sheddable background work.
pub(crate) const PRIORITY_HEADER: &str = "x-quipu-priority";

/// The declared scheduling class of the current request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReadPriority {
    Normal,
    Background,
}

impl ReadPriority {
    /// Only the exact value `background` lowers the class. Anything else,
    /// including duplicates, stays `normal`: the header can never elevate.
    pub(crate) fn from_headers(headers: &axum::http::HeaderMap) -> Self {
        let mut values = headers.get_all(PRIORITY_HEADER).iter();
        match (values.next(), values.next()) {
            (Some(value), None) if value.as_bytes().eq_ignore_ascii_case(b"background") => {
                Self::Background
            }
            _ => Self::Normal,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Background => "background",
        }
    }
}

tokio::task_local! {
    static PRIORITY: ReadPriority;
}

/// Run the rest of the request with the priority its headers declare.
pub(crate) async fn run(
    next: axum::middleware::Next,
    req: axum::extract::Request,
) -> axum::response::Response {
    let priority = ReadPriority::from_headers(req.headers());
    scope(priority, next.run(req)).await
}

/// Run a request future with its declared priority visible to admission.
pub(crate) async fn scope<F: std::future::Future>(priority: ReadPriority, fut: F) -> F::Output {
    PRIORITY.scope(priority, fut).await
}

fn current_priority() -> ReadPriority {
    PRIORITY.try_with(|p| *p).unwrap_or(ReadPriority::Normal)
}

/// Process-wide shedding policy, set once from `serve`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ShedPolicy {
    /// Longest a normal read waits for a permit before a 429. Positive.
    pub(crate) wait_ms: u64,
    /// Permits a background read must leave free for normal reads.
    pub(crate) background_reserve: usize,
}

static POLICY: OnceLock<Option<ShedPolicy>> = OnceLock::new();

/// Arm shedding from configuration. `wait_ms == 0` leaves it OFF.
pub(crate) fn init(wait_ms: u64, background_reserve: usize) {
    let policy = (wait_ms > 0).then_some(ShedPolicy {
        wait_ms,
        background_reserve,
    });
    if let Some(p) = policy {
        eprintln!(
            "read shedding: normal reads wait at most {}ms for a permit, background keeps {} permit(s) free; then 429 + Retry-After",
            p.wait_ms, p.background_reserve
        );
    }
    let _ = POLICY.set(policy);
}

#[cfg(test)]
tokio::task_local! { pub(crate) static TEST_POLICY: Option<ShedPolicy>; }

pub(crate) fn policy() -> Option<ShedPolicy> {
    #[cfg(test)]
    if let Ok(p) = TEST_POLICY.try_with(|p| *p) {
        return p;
    }
    POLICY.get().copied().flatten()
}

/// Is a read admission kind subject to shedding? Episode reads are part of a
/// write's resolution and keep their own budget; they are never shed here.
pub(crate) fn sheddable(kind: &str) -> bool {
    matches!(kind, "read" | "point-read" | "heavy-read")
}

/// Readers currently waiting for a permit (all sheddable kinds).
static WAITING: AtomicUsize = AtomicUsize::new(0);
/// Exponentially weighted mean read service time, in milliseconds.
static SERVICE_EWMA_MS: AtomicU64 = AtomicU64::new(0);

/// Counts this process's sheds, by class and reason, for `/metrics`.
static SHED_NORMAL_WAIT: AtomicU64 = AtomicU64::new(0);
static SHED_BACKGROUND_RESERVE: AtomicU64 = AtomicU64::new(0);
static SHED_WORKERS_BUSY: AtomicU64 = AtomicU64::new(0);

pub(crate) struct Waiting;
impl Waiting {
    pub(crate) fn start() -> Self {
        WAITING.fetch_add(1, Ordering::Relaxed);
        Self
    }
}
impl Drop for Waiting {
    fn drop(&mut self) {
        WAITING.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Fold one completed read's service time into the mean (weight 1/8).
pub(crate) fn record_service(elapsed: std::time::Duration) {
    let sample = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
    // A plain CAS loop: current stable deprecates `fetch_update` (renamed
    // `try_update`), which older toolchains do not have.
    let mut old = SERVICE_EWMA_MS.load(Ordering::Relaxed);
    loop {
        let next = if old == 0 {
            sample
        } else {
            old.saturating_mul(7).saturating_add(sample) / 8
        };
        match SERVICE_EWMA_MS.compare_exchange_weak(old, next, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => return,
            Err(actual) => old = actual,
        }
    }
}

/// Seconds until a retry is likely to be admitted: the queue ahead drained at
/// the measured service rate across the permits, clamped to [1, 30].
pub(crate) fn retry_after_secs(permits: usize) -> u64 {
    let ewma = SERVICE_EWMA_MS.load(Ordering::Relaxed).max(1);
    let ahead = u64::try_from(WAITING.load(Ordering::Relaxed) + 1).unwrap_or(u64::MAX);
    let permits = u64::try_from(permits.max(1)).unwrap_or(1);
    let ms = ewma.saturating_mul(ahead) / permits;
    ms.div_ceil(1000).clamp(1, 30)
}

/// Why a read was refused before it started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShedReason {
    /// A normal read waited `wait_ms` (or its remaining budget) without a permit.
    NormalWait,
    /// A background read found no permit above the reserve.
    BackgroundReserve,
    /// Every read worker process was busy (process pool).
    WorkersBusy,
}

impl ShedReason {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::NormalWait => "wait_exceeded",
            Self::BackgroundReserve => "background_reserve",
            Self::WorkersBusy => "workers_busy",
        }
    }
    fn counter(self) -> &'static AtomicU64 {
        match self {
            Self::NormalWait => &SHED_NORMAL_WAIT,
            Self::BackgroundReserve => &SHED_BACKGROUND_RESERVE,
            Self::WorkersBusy => &SHED_WORKERS_BUSY,
        }
    }
}

/// Build the refusal and count it. The read never ran.
pub(crate) fn shed(
    reason: ShedReason,
    priority: ReadPriority,
    waited: std::time::Duration,
    permits: usize,
) -> quipu::Error {
    reason.counter().fetch_add(1, Ordering::Relaxed);
    quipu::Error::ReadAdmissionShed {
        waited_ms: waited.as_millis(),
        class: priority.label(),
        reason: reason.label(),
        retry_after_s: retry_after_secs(permits),
    }
}

/// Acquire a read permit under the shedding policy.
///
/// Returns `Ok(None)` when shedding is OFF so the caller keeps its existing
/// acquisition path unchanged.
pub(crate) async fn acquire(
    policy: Option<ShedPolicy>,
    admission: &'static tokio::sync::Semaphore,
    deadline: Option<quipu::time::Deadline>,
    started: std::time::Instant,
) -> Result<Option<tokio::sync::SemaphorePermit<'static>>, quipu::Error> {
    let Some(policy) = policy else {
        return Ok(None);
    };
    let priority = current_priority();
    let permits = admission.available_permits();
    if priority == ReadPriority::Background {
        if permits <= policy.background_reserve {
            return Err(shed(
                ShedReason::BackgroundReserve,
                priority,
                started.elapsed(),
                permits,
            ));
        }
        return match admission.try_acquire() {
            Ok(permit) => Ok(Some(permit)),
            Err(tokio::sync::TryAcquireError::NoPermits) => Err(shed(
                ShedReason::BackgroundReserve,
                priority,
                started.elapsed(),
                permits,
            )),
            Err(tokio::sync::TryAcquireError::Closed) => Err(closed()),
        };
    }
    let mut wait = std::time::Duration::from_millis(policy.wait_ms);
    if let Some(deadline) = deadline {
        wait = wait.min(std::time::Duration::from_millis(
            deadline.remaining_millis(),
        ));
    }
    let _waiting = Waiting::start();
    match tokio::time::timeout(wait, admission.acquire()).await {
        Ok(Ok(permit)) => Ok(Some(permit)),
        Ok(Err(_)) => Err(closed()),
        Err(_) => Err(shed(
            ShedReason::NormalWait,
            priority,
            started.elapsed(),
            admission.available_permits().max(1),
        )),
    }
}

fn closed() -> quipu::Error {
    quipu::Error::InvalidValue("read admission is closed".into())
}

/// `quipu_read_shed_total{reason}` for the metrics page.
pub(crate) fn render_metrics(out: &mut String) {
    use std::fmt::Write as _;
    out.push_str(
        "# HELP quipu_read_shed_total Reads refused with 429 before they started (aegis-69x).\n",
    );
    out.push_str("# TYPE quipu_read_shed_total counter\n");
    for reason in [
        ShedReason::NormalWait,
        ShedReason::BackgroundReserve,
        ShedReason::WorkersBusy,
    ] {
        let _ = writeln!(
            out,
            "quipu_read_shed_total{{reason=\"{}\"}} {}",
            reason.label(),
            reason.counter().load(Ordering::Relaxed)
        );
    }
}
