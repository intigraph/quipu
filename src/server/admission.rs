//! Cancellable admission for synchronous store handlers.
//!
//! Writes have been admitted here since this module was written; reads were
//! added after aegis-raq1ok, where the missing read half wedged the server.

use super::base::{AppError, blocking};

/// Admit one write before moving it onto Tokio's uncancellable blocking pool.
///
/// A request future can be cancelled while it waits for this permit, so an
/// abandoned client never becomes another `spawn_blocking` task queued on the
/// writer mutex. The permit moves into the blocking closure and is held across
/// deferred embedding as well as the transaction: dropping the HTTP future
/// cannot release admission while its blocking work is still running.
static WRITE_ADMISSION: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

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
    let permit = admission.acquire().await.map_err(|_| {
        AppError::from(quipu::Error::InvalidValue(format!(
            "{kind} admission is closed"
        )))
    })?;
    blocking(move || {
        let _permit = permit;
        // Held across `f()` and restored on drop: blocking threads are pooled
        // and reused, so a leaked deadline would be inherited by an unrelated
        // later request on the same thread.
        let _deadline = quipu::time::set_request_deadline(deadline);
        let _wait = WriteWaitGuard::start((kind == "write").then_some(started));
        if kind == "write" {
            reject_expired_write()?;
        }
        f()
    })
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

/// Read admission under a deadline the caller already stamped.
pub(crate) async fn read_blocking_until<T, F>(
    started: std::time::Instant,
    deadline: Option<quipu::time::Deadline>,
    f: F,
) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    admit_with_deadline(read_admission(), "read", deadline, started, f).await
}

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
mod write_timeout_tests {
    use super::*;
    use axum::response::IntoResponse;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    fn hold_writer(
        shared: super::super::SharedStore,
    ) -> (std::sync::mpsc::Sender<()>, std::thread::JoinHandle<()>) {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _held = shared.writer.lock();
            ready_tx.send(()).unwrap();
            let _released = release_rx.recv();
        });
        ready_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        (release_tx, thread)
    }

    async fn assert_write_not_started(error: AppError) {
        let response = error.into_response();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(response.headers()[axum::http::header::RETRY_AFTER], "1");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["code"], "write_not_started");
        assert_eq!(body["write_started"], false);
        assert!(body["waited_ms"].as_u64().unwrap() >= 20);
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .contains("write did not run")
        );
        assert!(!body["error"].as_str().unwrap().contains("query_timeout_ms"));
    }

    #[tokio::test]
    async fn expired_write_admission_never_invokes_handler() {
        let admission = Box::leak(Box::new(tokio::sync::Semaphore::new(1)));
        let permit = admission.acquire().await.unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        let flag = ran.clone();
        let waiter = tokio::spawn(admit_with_deadline(
            admission,
            "write",
            Some(quipu::time::Deadline::after_millis(20)),
            std::time::Instant::now(),
            move || {
                flag.store(true, Ordering::SeqCst);
                Ok(())
            },
        ));
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        drop(permit);
        assert_write_not_started(waiter.await.unwrap().unwrap_err()).await;
        assert!(!ran.load(Ordering::SeqCst));
        assert_eq!(admission.available_permits(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn expired_writer_lock_never_invokes_tool() {
        let shared = Arc::new(super::super::StoreHandle::writer_only(
            quipu::Store::open_in_memory().unwrap(),
        ));
        let (release, holder) = hold_writer(shared.clone());
        let admission = Box::leak(Box::new(tokio::sync::Semaphore::new(1)));
        let ran = Arc::new(AtomicBool::new(false));
        let flag = ran.clone();
        let other = shared.clone();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let waiter = tokio::spawn(admit_with_deadline(
            admission,
            "write",
            Some(quipu::time::Deadline::after_millis(200)),
            std::time::Instant::now(),
            move || {
                entered_tx.send(()).unwrap();
                let _guard = other.write_lock()?;
                flag.store(true, Ordering::SeqCst);
                Ok(())
            },
        ));
        tokio::time::timeout(std::time::Duration::from_secs(2), entered_rx)
            .await
            .unwrap()
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        release.send(()).unwrap();
        holder.join().unwrap();
        assert_write_not_started(waiter.await.unwrap().unwrap_err()).await;
        assert!(!ran.load(Ordering::SeqCst));
        assert_eq!(admission.available_permits(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn actual_write_handler_refuses_before_creating_a_graph() {
        let shared = Arc::new(super::super::StoreHandle::writer_only(
            quipu::Store::open_in_memory().unwrap(),
        ));
        let (release, holder) = hold_writer(shared.clone());
        let other = shared.clone();
        let waiter = tokio::spawn(TEST_REQUEST_BUDGET_MS.scope(
            200,
            super::super::tools::graph_create(
                axum::extract::State(other),
                axum::Json(serde_json::json!({ "graph": "urn:timeout:control" })),
            ),
        ));
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        release.send(()).unwrap();
        holder.join().unwrap();
        assert_write_not_started(waiter.await.unwrap().unwrap_err()).await;
        let graphs = quipu::tool_graph_list(&shared.read(), &serde_json::json!({})).unwrap();
        assert!(!graphs.to_string().contains("urn:timeout:control"));
        // Control: exactly the same real handler creates the graph without a budget.
        let _created = super::super::tools::graph_create(
            axum::extract::State(shared.clone()),
            axum::Json(serde_json::json!({ "graph": "urn:timeout:control" })),
        )
        .await
        .unwrap();
        let graphs = quipu::tool_graph_list(&shared.read(), &serde_json::json!({})).unwrap();
        assert!(graphs.to_string().contains("urn:timeout:control"));
    }

    struct BudgetProvider {
        expire: bool,
    }

    impl quipu::EmbeddingProvider for BudgetProvider {
        fn embed_text(&self, _: &str) -> quipu::Result<Vec<f32>> {
            if self.expire {
                let deadline =
                    quipu::time::request_deadline().expect("real handler admission deadline");
                std::thread::sleep(std::time::Duration::from_millis(
                    deadline.remaining_millis() + 2,
                ));
            }
            Ok(vec![1.0, 0.0, 0.0])
        }
        fn dimension(&self) -> usize {
            3
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn episode_handler_refuses_expired_batch_then_commits_large_control() {
        let mut store = quipu::Store::open_in_memory().unwrap();
        store.resolution_config_mut().enabled = true;
        store.embedding_config_mut().auto_embed = false;
        store.set_embedding_provider(Arc::new(BudgetProvider { expire: true }));
        let shared = Arc::new(super::super::StoreHandle::writer_only(store));
        let input = |count: usize| {
            let nodes: Vec<_> = (0..count)
                .map(|i| {
                    serde_json::json!({
                        "name": format!("http-document-{i}"), "type": "Document",
                        "description": "request budget control",
                        "properties": {"ordinal": i, "category": "control", "status": "committed"}
                    })
                })
                .collect();
            serde_json::json!({"name": format!("http-batch-{count}"), "nodes": nodes, "edges": []})
        };
        let result = TEST_REQUEST_BUDGET_MS
            .scope(
                5000,
                super::super::tools::episode(
                    axum::extract::State(shared.clone()),
                    axum::Json(input(57)),
                ),
            )
            .await;
        let response = result.unwrap_err().into_response();
        assert_eq!(response.status(), axum::http::StatusCode::REQUEST_TIMEOUT);
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(body["error"].as_str().unwrap().contains("budget 5000ms"));
        assert!(!body["error"].as_str().unwrap().contains("exceeded 0ms"));
        assert!(shared.read().current_facts().unwrap().is_empty());
        shared
            .write_lock()
            .unwrap()
            .set_embedding_provider(Arc::new(BudgetProvider { expire: false }));
        let created = TEST_REQUEST_BUDGET_MS
            .scope(
                5000,
                super::super::tools::episode(
                    axum::extract::State(shared.clone()),
                    axum::Json(input(200)),
                ),
            )
            .await
            .unwrap()
            .0;
        assert_eq!(created["outcome"], "created");
        assert!(created["count"].as_u64().unwrap() >= 1200);
        assert!(shared.read().current_facts().unwrap().len() >= 1200);
    }

    /// Resolution that cannot finish until a concurrent write has: it waits
    /// for `released`, giving up after five seconds and recording that it did.
    struct HeldEmbed {
        released: Arc<AtomicBool>,
        gave_up: Arc<AtomicBool>,
        entered: Arc<AtomicBool>,
    }

    impl quipu::EmbeddingProvider for HeldEmbed {
        fn embed_text(&self, _: &str) -> quipu::Result<Vec<f32>> {
            self.entered.store(true, Ordering::SeqCst);
            let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !self.released.load(Ordering::SeqCst) {
                if std::time::Instant::now() > until {
                    self.gave_up.store(true, Ordering::SeqCst);
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Ok(vec![1.0, 0.0, 0.0])
        }
        fn dimension(&self) -> usize {
            3
        }
    }

    /// aegis-xi0: a non-strict `/episode` resolves on a reader, so a write can
    /// complete while it is still resolving. Before, resolution ran under the
    /// single writer: the write could only finish after resolution, so the
    /// held embedder would give up instead of being released.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn episode_resolution_does_not_hold_the_writer() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("episode.db");
        let path = db.to_str().unwrap();
        let mut store = quipu::Store::open(path).unwrap();
        store.resolution_config_mut().enabled = true;
        store.embedding_config_mut().auto_embed = false;
        let seed = quipu::Datum {
            entity: store.intern("urn:xi0:concurrent").unwrap(),
            attribute: store
                .intern("http://www.w3.org/2000/01/rdf-schema#label")
                .unwrap(),
            value: quipu::Value::Str("seeded".into()),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
            op: quipu::Op::Assert,
        };
        store
            .transact(&[seed], "2026-01-01T00:00:00Z", None, None)
            .unwrap();
        let (released, gave_up, entered) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        store.set_embedding_provider(Arc::new(HeldEmbed {
            released: released.clone(),
            gave_up: gave_up.clone(),
            entered: entered.clone(),
        }));
        let readers = super::super::ReadPool::open(path, &store, 1);
        let shared = Arc::new(super::super::StoreHandle::serving(
            store,
            readers,
            path,
            Default::default(),
        ));
        assert!(shared.resolves_off_writer(), "precondition: pooled reader");
        let input = serde_json::json!({"name": "held-batch", "edges": [],
            "nodes": [{"name": "held-document", "type": "Document"}]});

        let episode = tokio::spawn(super::super::tools::episode(
            axum::extract::State(shared.clone()),
            axum::Json(input),
        ));
        while !entered.load(Ordering::SeqCst) {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let _set = super::super::tools::set_predicate(
            axum::extract::State(shared.clone()),
            axum::Json(serde_json::json!({"entity": "urn:xi0:concurrent",
                "predicate": "http://www.w3.org/2000/01/rdf-schema#label", "value": "concurrent"})),
        )
        .await
        .unwrap();
        released.store(true, Ordering::SeqCst);
        let created = episode.await.unwrap().unwrap().0;

        assert_eq!(created["outcome"], "created");
        assert!(
            !gave_up.load(Ordering::SeqCst),
            "the write could not complete while the episode was resolving"
        );
    }

    #[tokio::test]
    async fn expired_read_retains_query_timeout_response() {
        let admission = Box::leak(Box::new(tokio::sync::Semaphore::new(1)));
        let result = admit_with_deadline(
            admission,
            "read",
            Some(quipu::time::Deadline::after_millis(0)),
            std::time::Instant::now(),
            || {
                let store = quipu::Store::open_in_memory()?;
                let _result = quipu::sparql_query(&store, "SELECT ?s ?p ?o WHERE { ?s ?p ?o }")?;
                Ok(())
            },
        )
        .await;
        let response = result.unwrap_err().into_response();
        assert_eq!(response.status(), axum::http::StatusCode::REQUEST_TIMEOUT);
        assert!(
            response
                .headers()
                .get(axum::http::header::RETRY_AFTER)
                .is_none()
        );
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(body["error"].as_str().unwrap().contains("narrow the query"));
        assert!(body.get("write_started").is_none());
    }

    #[tokio::test]
    async fn unexpired_write_runs_and_thread_context_is_restored() {
        let admission = Box::leak(Box::new(tokio::sync::Semaphore::new(1)));
        let result =
            admit_with_deadline(admission, "write", None, std::time::Instant::now(), || {
                reject_expired_write()?;
                Ok(42)
            })
            .await
            .unwrap();
        assert_eq!(result, 42);
        assert_eq!(admission.available_permits(), 1);
        assert!(WRITE_WAIT_START.with(std::cell::Cell::get).is_none());
        assert!(quipu::time::request_deadline().is_none());
    }
}
