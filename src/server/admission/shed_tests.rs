//! aegis-69x: early read shedding and the background priority class.
use super::shed::{ReadPriority, ShedPolicy, TEST_POLICY};
use super::*;
use axum::response::IntoResponse;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const ARMED: Option<ShedPolicy> = Some(ShedPolicy {
    wait_ms: 50,
    background_reserve: 1,
});

fn leaked(permits: usize) -> &'static tokio::sync::Semaphore {
    Box::leak(Box::new(tokio::sync::Semaphore::new(permits)))
}

fn status_and_retry(error: AppError) -> (u16, Option<String>) {
    let response = error.into_response();
    let retry = response
        .headers()
        .get(axum::http::header::RETRY_AFTER)
        .map(|v| v.to_str().unwrap().to_owned());
    (response.status().as_u16(), retry)
}

async fn read_under(
    policy: Option<ShedPolicy>,
    priority: ReadPriority,
    admission: &'static tokio::sync::Semaphore,
    budget_ms: u64,
    ran: Arc<AtomicBool>,
) -> Result<u8, AppError> {
    TEST_POLICY
        .scope(
            policy,
            super::shed::scope(
                priority,
                admit_with_deadline(
                    admission,
                    "read",
                    Some(quipu::time::Deadline::after_millis(budget_ms)),
                    std::time::Instant::now(),
                    move || {
                        ran.store(true, Ordering::SeqCst);
                        Ok(7)
                    },
                ),
            ),
        )
        .await
}

/// THE FAILING-FIRST CASE. On main an ordinary read behind a held permit
/// waits for the permit (its 10s budget ends it as 408 only after it is
/// admitted); here it is refused with 429 + Retry-After inside the wait.
#[tokio::test]
async fn queued_normal_read_gets_429_with_retry_after_within_the_wait() {
    let admission = leaked(1);
    let held = admission.acquire().await.unwrap();
    let ran = Arc::new(AtomicBool::new(false));
    let started = std::time::Instant::now();
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(1000),
        read_under(ARMED, ReadPriority::Normal, admission, 10_000, ran.clone()),
    )
    .await
    .expect("a busy read must be refused inside its wait, not after its budget");
    assert!(started.elapsed() < std::time::Duration::from_millis(1000));
    let (status, retry) = status_and_retry(result.expect_err("refused"));
    assert_eq!(status, 429);
    let retry: u64 = retry.expect("Retry-After present").parse().unwrap();
    assert!((1..=30).contains(&retry));
    assert!(!ran.load(Ordering::SeqCst), "a shed read never runs");
    drop(held);
    assert_eq!(admission.available_permits(), 1, "no permit leaked");
}

#[tokio::test]
async fn background_read_never_takes_the_reserved_permit() {
    let admission = leaked(2);
    let held = admission.acquire().await.unwrap(); // one free == reserve
    let ran = Arc::new(AtomicBool::new(false));
    let started = std::time::Instant::now();
    let result = read_under(
        ARMED,
        ReadPriority::Background,
        admission,
        10_000,
        ran.clone(),
    )
    .await;
    assert!(
        started.elapsed() < std::time::Duration::from_millis(40),
        "background never queues"
    );
    assert_eq!(status_and_retry(result.expect_err("refused")).0, 429);
    assert!(!ran.load(Ordering::SeqCst));
    // The same free permit still serves a normal read.
    let normal = read_under(ARMED, ReadPriority::Normal, admission, 10_000, ran.clone()).await;
    assert_eq!(normal.unwrap(), 7);
    drop(held);
}

#[tokio::test]
async fn background_read_runs_when_permits_exceed_the_reserve() {
    let admission = leaked(3);
    let ran = Arc::new(AtomicBool::new(false));
    let result = read_under(
        ARMED,
        ReadPriority::Background,
        admission,
        10_000,
        ran.clone(),
    )
    .await;
    assert_eq!(result.unwrap(), 7);
    assert!(ran.load(Ordering::SeqCst));
    assert_eq!(admission.available_permits(), 3);
}

/// OFF by default: no policy means the original wait, then completion once
/// the permit frees (no 429, header ignored).
#[tokio::test]
async fn policy_off_keeps_todays_waiting_behaviour() {
    let admission = leaked(1);
    let held = admission.acquire().await.unwrap();
    let ran = Arc::new(AtomicBool::new(false));
    let task = tokio::spawn(read_under(
        None,
        ReadPriority::Background,
        admission,
        10_000,
        ran.clone(),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    assert!(!task.is_finished(), "still queued: nothing sheds when OFF");
    drop(held);
    assert_eq!(task.await.unwrap().unwrap(), 7);
}

/// A budget shorter than the wait bounds it: refused at the budget, not later.
#[tokio::test]
async fn remaining_budget_bounds_the_wait() {
    let admission = leaked(1);
    let held = admission.acquire().await.unwrap();
    let ran = Arc::new(AtomicBool::new(false));
    let policy = Some(ShedPolicy {
        wait_ms: 5_000,
        background_reserve: 1,
    });
    let started = std::time::Instant::now();
    let result = read_under(policy, ReadPriority::Normal, admission, 30, ran.clone()).await;
    assert!(started.elapsed() < std::time::Duration::from_millis(1000));
    assert_eq!(status_and_retry(result.expect_err("refused")).0, 429);
    assert!(!ran.load(Ordering::SeqCst));
    drop(held);
}

/// Episode reads belong to a write's resolution and are never shed here.
#[test]
fn only_read_kinds_are_sheddable() {
    for kind in ["read", "point-read", "heavy-read"] {
        assert!(super::shed::sheddable(kind));
    }
    for kind in ["write", "episode-read"] {
        assert!(!super::shed::sheddable(kind));
    }
}

#[test]
fn priority_header_can_only_lower_the_class() {
    let parse = |values: &[&str]| {
        let mut headers = axum::http::HeaderMap::new();
        for v in values {
            headers.append(super::shed::PRIORITY_HEADER, v.parse().unwrap());
        }
        ReadPriority::from_headers(&headers)
    };
    assert_eq!(parse(&[]), ReadPriority::Normal);
    assert_eq!(parse(&["background"]), ReadPriority::Background);
    assert_eq!(parse(&["BACKGROUND"]), ReadPriority::Background);
    assert_eq!(parse(&["high"]), ReadPriority::Normal);
    assert_eq!(parse(&["normal"]), ReadPriority::Normal);
    assert_eq!(
        parse(&["background", "background"]),
        ReadPriority::Normal,
        "duplicates are ambiguous and never lower"
    );
}

#[test]
fn retry_after_divides_by_pool_capacity_not_free_permits() {
    // 2 s mean service, 3 waiting (4 ahead including this caller), 8 permits:
    // the queue drains in 1 s. Dividing by the free permits (0 when shedding
    // fires, treated as 1) would have advertised 8 s.
    assert_eq!(super::shed::retry_after_from(2_000, 3, 8), 1);
    assert_eq!(super::shed::retry_after_from(2_000, 3, 0), 8);
    assert_eq!(super::shed::retry_after_from(0, 0, 4), 1, "floor is 1 s");
    assert_eq!(super::shed::retry_after_from(60_000, 99, 1), 30, "cap");
}

#[test]
fn saturated_pool_capacity_counts_running_reads() {
    let exhausted = leaked(0);
    let _running = super::shed::InService::start();
    // Other tests may run reads concurrently; this one's read is counted.
    assert!(super::shed::capacity(exhausted) >= 1);
}
