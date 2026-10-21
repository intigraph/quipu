use super::*;
use quipu::metrics::inflight::Registry;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Instant;

fn rendered(registry: &Registry) -> String {
    let mut text = String::new();
    registry.render(&mut text, true);
    text
}

#[tokio::test]
async fn inflight_cancelled_waiter_is_removed_without_running() {
    use std::future::Future;
    let admission = Box::leak(Box::new(tokio::sync::Semaphore::new(0)));
    let registry = Arc::new(Registry::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let work_calls = calls.clone();
    let mut future = Box::pin(admit_tracked(
        admission,
        "write",
        None,
        Instant::now(),
        None,
        move || {
            work_calls.fetch_add(1, Ordering::Relaxed);
            Ok(())
        },
        registry.clone(),
    ));
    std::future::poll_fn(|cx| {
        assert!(future.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    assert!(rendered(&registry).contains("lane=\"write\",phase=\"waiting\"} 1"));
    drop(future);
    assert!(rendered(&registry).contains("lane=\"write\",phase=\"waiting\"} 0"));
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn inflight_http_cancellation_retains_real_blocking_closure() {
    let admission = Box::leak(Box::new(tokio::sync::Semaphore::new(1)));
    let registry = Arc::new(Registry::default());
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let work = tokio::spawn(admit_tracked(
        admission,
        "write",
        None,
        Instant::now(),
        None,
        move || {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(())
        },
        registry.clone(),
    ));
    started_rx.await.unwrap();
    assert!(rendered(&registry).contains("lane=\"write\",phase=\"active\"} 1"));
    work.abort();
    assert!(work.await.unwrap_err().is_cancelled());
    assert!(rendered(&registry).contains("lane=\"write\",phase=\"active\"} 1"));
    release_tx.send(()).unwrap();
    // The permit is released only after the detached closure drops its work token.
    let _permit = admission.acquire().await.unwrap();
    assert!(rendered(&registry).contains("lane=\"write\",phase=\"active\"} 0"));
}

#[tokio::test]
async fn inflight_error_and_panic_leave_no_store_work() {
    let admission = Box::leak(Box::new(tokio::sync::Semaphore::new(1)));
    let registry = Arc::new(Registry::default());
    for panic in [false, true] {
        let result = admit_tracked(
            admission,
            "write",
            None,
            Instant::now(),
            None,
            move || -> Result<(), AppError> {
                assert!(!panic, "private blocking failure");
                Err(quipu::Error::InvalidValue("private error".into()).into())
            },
            registry.clone(),
        )
        .await;
        assert!(result.is_err());
        assert!(rendered(&registry).contains("lane=\"write\",phase=\"active\"} 0"));
        assert!(rendered(&registry).contains("quipu_store_oldest_write_age_seconds 0\n"));
    }
}
