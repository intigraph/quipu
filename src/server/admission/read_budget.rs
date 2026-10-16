//! Optional client read budgets; absent headers retain the legacy request cap.

use super::{
    AdmissionStamp, AppError, POINT_READ_ADMISSION, admit_with_deadline,
    admit_with_deadline_observed, read_admission, request_budget,
};

/// One monotonic deadline stamped before admission, never renewed between stages.
pub(crate) struct ReadBudget {
    pub(crate) started: std::time::Instant,
    pub(crate) deadline: Option<quipu::time::Deadline>,
    pub(super) explicit: bool,
}

impl ReadBudget {
    pub(crate) fn from_headers(
        headers: &axum::http::HeaderMap,
        config: &quipu::SearchConfig,
    ) -> Result<Self, AppError> {
        let started = std::time::Instant::now();
        let server = request_budget();
        let values: Vec<_> = headers.get_all("x-quipu-timeout-ms").iter().collect();
        let deadline = if values.is_empty() {
            server
        } else {
            let invalid = || {
                AppError::from(quipu::Error::InvalidValue(
                    "X-Quipu-Timeout-Ms requires one unsigned integer millisecond budget".into(),
                ))
            };
            if values.len() != 1 {
                return Err(invalid());
            }
            let raw = values[0].to_str().map_err(|_| invalid())?;
            if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
                return Err(invalid());
            }
            let mut ms = raw.parse::<u64>().map_err(|_| invalid())?;
            if config.query_timeout_ms > 0 {
                ms = ms.min(config.query_timeout_ms);
            }
            if let Some(cap) = server {
                ms = ms.min(cap.remaining_millis());
            }
            // Bound construction even when both server caps are disabled.
            // Reject rather than overflow Instant on an untrusted header.
            if started
                .checked_add(std::time::Duration::from_millis(ms))
                .is_none()
            {
                return Err(invalid());
            }
            Some(quipu::time::Deadline::after_millis(ms))
        };
        Ok(Self {
            started,
            deadline,
            explicit: !values.is_empty(),
        })
    }
}

pub(crate) async fn client_read_blocking<T, F>(budget: ReadBudget, f: F) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    admit_with_deadline(
        read_admission(),
        if budget.explicit {
            "client-read"
        } else {
            "read"
        },
        budget.deadline,
        budget.started,
        f,
    )
    .await
}

pub(crate) async fn query_blocking<T, F>(
    point: bool,
    stamp: Option<AdmissionStamp>,
    budget: ReadBudget,
    f: F,
) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    let (admission, kind) = if point {
        (&POINT_READ_ADMISSION, "point-read")
    } else {
        (
            read_admission(),
            if budget.explicit {
                "client-read"
            } else {
                "read"
            },
        )
    };
    admit_with_deadline_observed(admission, kind, budget.deadline, budget.started, stamp, f).await
}
