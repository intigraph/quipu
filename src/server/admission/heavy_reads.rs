//! Opt-in, cooperative heavy-operation cap before shared read admission.
use super::{
    AdmissionStamp, AppError, admit_with_deadline_observed, read_admission, request_budget,
};
use std::sync::OnceLock;
use tokio::sync::Semaphore;

struct Policy {
    operations: Vec<String>,
    cap: Semaphore,
}
static POLICY: OnceLock<Policy> = OnceLock::new();

pub(crate) fn init(config: &quipu::config::ServerConfig) {
    if config.heavy_read_operations.is_empty() {
        return;
    }
    let _ = POLICY.set(Policy {
        operations: config.heavy_read_operations.clone(),
        cap: Semaphore::new(config.heavy_read_permits),
    });
    eprintln!(
        "heavy read cap: {} ordinary slot(s), cooperative owner-rows scheduling",
        config.heavy_read_permits
    );
}

pub(crate) fn configured(operation: Option<&str>) -> bool {
    POLICY.get().is_some_and(|policy| {
        operation.is_some_and(|op| policy.operations.iter().any(|known| known == op))
    })
}

pub(crate) async fn query_blocking<T, F>(
    point: bool,
    heavy: bool,
    stamp: Option<AdmissionStamp>,
    f: F,
) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    if !heavy {
        return super::query_blocking(point, stamp, f).await;
    }
    let policy = POLICY.get().ok_or_else(|| {
        AppError::from(quipu::Error::InvalidValue(
            "heavy read policy unavailable".into(),
        ))
    })?;
    read_with(
        &policy.cap,
        read_admission(),
        std::time::Instant::now(),
        request_budget(),
        stamp,
        f,
    )
    .await
}

async fn read_with<T, F>(
    cap: &'static Semaphore,
    reads: &'static Semaphore,
    started: std::time::Instant,
    deadline: Option<quipu::time::Deadline>,
    stamp: Option<AdmissionStamp>,
    f: F,
) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    // Class first: waiters cannot reserve shared readers. Use one original
    // deadline across BOTH waits and the eventual blocking query.
    let acquire = cap.acquire();
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
            "heavy read admission closed".into(),
        ))
    })?;
    admit_with_deadline_observed(reads, "heavy-read", deadline, started, stamp, move || {
        // Both permits must survive cancellation of the HTTP future while
        // uncancellable blocking work remains alive.
        let _heavy_permit = permit;
        quipu::time::check_request_deadline()?;
        f()
    })
    .await
}

#[cfg(test)]
#[path = "heavy_reads_tests.rs"]
mod tests;
