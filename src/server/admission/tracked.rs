//! Admission and telemetry ownership follow the actual blocking closure.
use super::{AdmissionStamp, AppError, WriteWaitGuard, reject_expired_write};
use crate::base::blocking_pretracked;

pub(super) async fn admit_tracked<T, F>(
    admission: &'static tokio::sync::Semaphore,
    kind: &'static str,
    deadline: Option<quipu::time::Deadline>,
    started: std::time::Instant,
    stamp: Option<AdmissionStamp>,
    f: F,
    registry: std::sync::Arc<quipu::metrics::inflight::Registry>,
) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    let lane = match kind {
        "write" => quipu::metrics::inflight::Lane::Write,
        "point-read" => quipu::metrics::inflight::Lane::PointRead,
        _ => quipu::metrics::inflight::Lane::Read,
    };
    let work = registry.start(lane, quipu::metrics::inflight::Phase::Waiting);
    let acquire = admission.acquire();
    let acquired = if matches!(kind, "point-read" | "episode-read" | "heavy-read") {
        if let Some(deadline) = deadline {
            let remaining = deadline.remaining_millis();
            let limit_ms = u128::from(remaining) + started.elapsed().as_millis();
            tokio::time::timeout(std::time::Duration::from_millis(remaining), acquire)
                .await
                .map_err(|_| {
                    if kind == "episode-read" {
                        AppError::from(deadline.request_timeout())
                    } else {
                        AppError::from(quipu::Error::RequestTimeout {
                            elapsed_ms: started.elapsed().as_millis(),
                            limit_ms,
                        })
                    }
                })?
        } else {
            acquire.await
        }
    } else {
        acquire.await
    };
    let permit = acquired.map_err(|_| {
        AppError::from(quipu::Error::InvalidValue(format!(
            "{kind} admission is closed"
        )))
    })?;
    if let Some(stamp) = stamp {
        let _ = stamp.set(std::time::Instant::now());
    }
    blocking_pretracked(move || {
        let _permit = permit;
        work.activate();
        let _work = work;
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
