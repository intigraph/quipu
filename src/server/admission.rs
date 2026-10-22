//! Cancellable admission for synchronous store handlers.
//!
//! Writes have been admitted here since this module was written; reads were
//! added after aegis-raq1ok, where the missing read half wedged the server.

use super::base::AppError;

#[path = "admission/tracked.rs"]
mod tracked;
use tracked::admit_tracked;

/// Admit one write before moving it onto Tokio's uncancellable blocking pool.
///
/// A request future can be cancelled while it waits for this permit, so an
/// abandoned client never becomes another `spawn_blocking` task queued on the
/// writer mutex. The permit moves into the blocking closure and is held across
/// deferred embedding as well as the transaction: dropping the HTTP future
/// cannot release admission while its blocking work is still running.
static WRITE_ADMISSION: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

/// One FIFO permit paired with the opt-in reserved reader. Ordinary reads
/// cannot consume it; point floods cannot consume ordinary admission either.
static POINT_READ_ADMISSION: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

/// Long episode resolution may occupy only one ordinary read slot. Acquire
/// this before read admission, so queued episodes cannot reserve the pool.
static EPISODE_READ_ADMISSION: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

/// Single-assignment, log-only observation of a granted admission permit.
pub(super) type AdmissionStamp = std::sync::Arc<std::sync::OnceLock<std::time::Instant>>;

pub(crate) async fn query_blocking<T, F>(
    point: bool,
    stamp: Option<AdmissionStamp>,
    f: F,
) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    let (admission, kind) = if point {
        (&POINT_READ_ADMISSION, "point-read")
    } else {
        (read_admission(), "read")
    };
    let started = std::time::Instant::now();
    let deadline = request_budget();
    admit_with_deadline_observed(admission, kind, deadline, started, stamp, f).await
}

pub(crate) async fn write_blocking<T, F>(f: F) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    write_blocking_with(&WRITE_ADMISSION, f).await
}

pub(crate) async fn write_blocking_with<T, F>(
    admission: &'static tokio::sync::Semaphore,
    f: F,
) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    admit_blocking_with(admission, "write", f).await
}

/// The shared acquire-then-block step behind both admission paths.
///
/// Both halves must acquire in ASYNC context and hold the permit across the
/// blocking closure; writing that twice invites the two from drifting, and the
/// read half exists precisely because one half of this codebase had the
/// property and the other did not (aegis-raq1ok).
async fn admit_blocking_with<T, F>(
    admission: &'static tokio::sync::Semaphore,
    kind: &'static str,
    f: F,
) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    // Stamped BEFORE the permit is acquired, which is the whole point: the
    // budget has to cover the queue, because the queue is where a wedged store
    // spends a request's life (aegis-raq1ok). Stamping it after `acquire()`
    // would reproduce the per-query default's blind spot one layer up.
    let started = std::time::Instant::now();
    let deadline = request_budget();
    admit_with_deadline(admission, kind, deadline, started, f).await
}

async fn admit_with_deadline<T, F>(
    admission: &'static tokio::sync::Semaphore,
    kind: &'static str,
    deadline: Option<quipu::time::Deadline>,
    started: std::time::Instant,
    f: F,
) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    admit_with_deadline_observed(admission, kind, deadline, started, None, f).await
}

async fn admit_with_deadline_observed<T, F>(
    admission: &'static tokio::sync::Semaphore,
    kind: &'static str,
    deadline: Option<quipu::time::Deadline>,
    started: std::time::Instant,
    stamp: Option<AdmissionStamp>,
    f: F,
) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    admit_tracked(
        admission,
        kind,
        deadline,
        started,
        stamp,
        f,
        quipu::metrics::inflight::store().clone(),
    )
    .await
}

thread_local! {
    static WRITE_WAIT_START: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
}

struct WriteWaitGuard(Option<std::time::Instant>);
impl WriteWaitGuard {
    fn start(started: Option<std::time::Instant>) -> Self {
        Self(WRITE_WAIT_START.with(|cell| cell.replace(started)))
    }
}
impl Drop for WriteWaitGuard {
    fn drop(&mut self) {
        WRITE_WAIT_START.with(|cell| cell.set(self.0));
    }
}

/// Call only before the first write: this error guarantees the tool did not run.
/// Checking again after a commit would mislabel an indeterminate result as safe.
pub(crate) fn reject_expired_write() -> Result<(), AppError> {
    if quipu::time::request_deadline().is_some_and(|deadline| deadline.passed()) {
        let waited_ms =
            WRITE_WAIT_START.with(|cell| cell.get().map_or(0, |start| start.elapsed().as_millis()));
        return Err(quipu::Error::WriteAdmissionTimeout { waited_ms }.into());
    }
    Ok(())
}

/// Wall-clock budget covering queue time AND execution, in milliseconds.
/// `0` disables admission-time stamping, which is the default: the per-query
/// budget then behaves exactly as it did before this existed.
static REQUEST_BUDGET_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Set the admission-time request budget. Called once, from `serve`.
///
/// DISABLED BY DEFAULT, deliberately. Arming this narrows the effective budget
/// for any request that queues — that is exactly what it is for, and it is also
/// a behaviour change on a store that auto-deploys from `main` within 15
/// minutes of a merge. Landing it inert means the code ships and is exercised
/// by its tests, and arming it stays a separate, observable act with a number
/// behind it rather than a side effect of a merge.
pub(crate) fn init_request_budget_ms(ms: u64) {
    REQUEST_BUDGET_MS.store(ms, std::sync::atomic::Ordering::Relaxed);
    if ms > 0 {
        eprintln!("request budget: {ms}ms from admission (covers queue + execution)");
    }
}

/// `/episode`'s own admission budget; `0` falls back to [`request_budget`].
static EPISODE_BUDGET_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Set the `/episode` admission budget. Called once, from `serve`. Inert at 0.
pub(crate) fn init_episode_budget_ms(ms: u64) {
    EPISODE_BUDGET_MS.store(ms, std::sync::atomic::Ordering::Relaxed);
    if ms > 0 {
        eprintln!("episode budget: {ms}ms from admission (covers resolution + commit)");
    }
}

/// The deadline for one `/episode` request, stamped once at its start and
/// carried through BOTH of its admissions (aegis-xi0). Stamping per admission
/// would hand the commit a fresh budget after resolution had spent the first.
pub(crate) fn episode_budget() -> (std::time::Instant, Option<quipu::time::Deadline>) {
    let started = std::time::Instant::now();
    let ms = EPISODE_BUDGET_MS.load(std::sync::atomic::Ordering::Relaxed);
    #[cfg(test)]
    let ms = TEST_EPISODE_BUDGET_MS
        .try_with(|value| *value)
        .unwrap_or(ms);
    let deadline = if ms > 0 {
        Some(quipu::time::Deadline::after_millis(ms))
    } else {
        request_budget()
    };
    (started, deadline)
}

/// The deadline for one `/import`, stamped at its start and carried through its
/// read-side resolution and its writer section (aegis-oa5). It is the ordinary
/// request budget: an import that cannot finish inside it rolls back and
/// releases the writer, and the caller splits the share.
pub(crate) fn import_budget() -> (std::time::Instant, Option<quipu::time::Deadline>) {
    (std::time::Instant::now(), request_budget())
}

/// Bound phase-zero episodes separately while retaining ordinary read admission.
pub(crate) async fn episode_read_blocking_until<T, F>(
    started: std::time::Instant,
    deadline: Option<quipu::time::Deadline>,
    f: F,
) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    episode_read_with(
        &EPISODE_READ_ADMISSION,
        read_admission(),
        started,
        deadline,
        f,
    )
    .await
}

async fn episode_read_with<T, F>(
    episodes: &'static tokio::sync::Semaphore,
    reads: &'static tokio::sync::Semaphore,
    started: std::time::Instant,
    deadline: Option<quipu::time::Deadline>,
    f: F,
) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    let acquire = episodes.acquire();
    let permit = if let Some(deadline) = deadline {
        tokio::time::timeout(
            std::time::Duration::from_millis(deadline.remaining_millis()),
            acquire,
        )
        .await
        .map_err(|_| AppError::from(deadline.request_timeout()))?
    } else {
        acquire.await
    }
    .map_err(|_| {
        AppError::from(quipu::Error::InvalidValue(
            "episode admission is closed".into(),
        ))
    })?;
    // Ownership must cross spawn_blocking: cancellation of the HTTP future
    // cannot release capacity while its uncancellable resolution is running.
    admit_with_deadline(reads, "episode-read", deadline, started, move || {
        let _episode_permit = permit;
        quipu::time::check_request_deadline()?;
        f()
    })
    .await
}

#[cfg(test)]
#[path = "admission/episode_cap_tests.rs"]
mod episode_cap_tests;

/// Write admission under a deadline the caller already stamped.
pub(crate) async fn write_blocking_until<T, F>(
    started: std::time::Instant,
    deadline: Option<quipu::time::Deadline>,
    f: F,
) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    admit_with_deadline(&WRITE_ADMISSION, "write", deadline, started, f).await
}

#[cfg(test)]
tokio::task_local! { static TEST_REQUEST_BUDGET_MS: u64; }
#[cfg(test)]
tokio::task_local! { pub(crate) static TEST_EPISODE_BUDGET_MS: u64; }

fn request_budget() -> Option<quipu::time::Deadline> {
    let ms = REQUEST_BUDGET_MS.load(std::sync::atomic::Ordering::Relaxed);
    #[cfg(test)]
    let ms = TEST_REQUEST_BUDGET_MS
        .try_with(|value| *value)
        .unwrap_or(ms);
    (ms > 0).then(|| quipu::time::Deadline::after_millis(ms))
}

/// Cancellable admission for synchronous READ handlers (aegis-raq1ok).
///
/// The write path above has been cancellable since it was written; **the read
/// path was not, and that asymmetry is what took the deployed server down for 9 minutes
/// on 2026-09-06.** Measured from the request log: 312 requests started and
/// never completed, 246 of them `/query`; the last read to complete did so at
/// `00:04:54Z` and the next at `00:13:42Z`, after a restart. Throughout,
/// `/health`, `/version` and `/set` answered 200 in ~0 ms — writes survived
/// precisely because they queue on `WRITE_ADMISSION`, where an abandoned
/// client's future is dropped BEFORE it reaches the uncancellable blocking
/// pool. Reads went straight to `blocking()` and kept their thread.
///
/// The consequence is a ratchet, not a slowdown. `spawn_blocking` work cannot
/// be cancelled, so every reader that gives up (yupana-hook abandons at ~30 s,
/// aegis-tjyhh4) leaves a thread parked on a read connection while its client
/// retries. In-flight reads rose 17 -> 292 without ever plateauing, the cgroup
/// hit `TasksMax` — `cgroup: fork rejected by pids controller`, the only such
/// line in the journal — and no read completed again until the process was
/// restarted.
///
/// Permits are sized to the read pool because a read cannot make progress
/// without a pool connection: beyond `read_pool_size`, extra admitted readers
/// are threads waiting on a `FairMutex`, which is exactly the wasted resource
/// that exhausted the pid budget. Bounding admission converts that unbounded
/// thread growth into bounded queueing on a cancellable semaphore.
static READ_ADMISSION: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();

/// Used when `init_read_admission` was never called — unit tests construct
/// handlers directly without going through `serve`. Deliberately not 1: an
/// uninitialised default that serialised every read would turn a missing
/// startup call into a silent throughput collapse rather than a loud failure.
const DEFAULT_READ_PERMITS: usize = 8;

/// Size read admission from the actual read pool. Called once, from `serve`.
///
/// An EMPTY pool is not "no limit": reads then serialise on the writer lock, so
/// one permit is the honest bound rather than a number that pretends to
/// parallelism the store cannot deliver.
pub(crate) fn init_read_admission_for_pool(pool_len: usize) {
    let permits = if pool_len > 0 { pool_len } else { 1 };
    init_read_admission(permits);
    eprintln!("read admission: {permits} concurrent reads (cancellable while queued)");
}

/// The permit count directly, for tests and for callers that have already
/// decided. Prefer [`init_read_admission_for_pool`] from `serve`.
pub(crate) fn init_read_admission(permits: usize) {
    let _ = READ_ADMISSION.set(tokio::sync::Semaphore::new(permits.max(1)));
}

fn read_admission() -> &'static tokio::sync::Semaphore {
    READ_ADMISSION.get_or_init(|| tokio::sync::Semaphore::new(DEFAULT_READ_PERMITS))
}

/// Admit one read before moving it onto Tokio's uncancellable blocking pool.
///
/// Mirrors [`write_blocking`]. The permit is acquired in async context, so a
/// client that disconnects while queued is cancelled here and never becomes a
/// blocking task; and it is held across the blocking closure, so dropping the
/// HTTP future cannot release admission while the read is still running.
pub(crate) async fn read_blocking<T, F>(f: F) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    admit_blocking_with(read_admission(), "read", f).await
}

/// `read_blocking` against a caller-supplied semaphore, so a test can prove the
/// cancellation property without touching the process-wide permit count.
#[cfg(test)]
pub(crate) async fn read_blocking_with<T, F>(
    admission: &'static tokio::sync::Semaphore,
    f: F,
) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    admit_blocking_with(admission, "read", f).await
}

#[cfg(test)]
#[path = "admission/write_timeout_tests.rs"]
mod write_timeout_tests;

#[cfg(test)]
#[path = "admission/point_timeout_tests.rs"]
mod point_timeout_tests;

#[cfg(test)]
#[path = "admission/observation_tests.rs"]
mod observation_tests;

#[cfg(test)]
#[path = "admission/inflight_tests.rs"]
mod inflight_tests;
