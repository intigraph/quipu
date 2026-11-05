use super::*;
use std::future::Future;
use std::task::Poll;

fn semaphore(n: usize) -> &'static Semaphore {
    Box::leak(Box::new(Semaphore::new(n)))
}

#[tokio::test]
async fn waiting_heavy_reads_leave_shared_readers_available() {
    let cap = semaphore(1);
    let reads = semaphore(4);
    let held = cap.acquire().await.unwrap();
    let (release, wait) = std::sync::mpsc::channel::<()>();
    let mut pending = Box::pin(read_with(
        cap,
        reads,
        std::time::Instant::now(),
        None,
        None,
        move || {
            let _ = wait.recv_timeout(std::time::Duration::from_secs(3));
            Ok(())
        },
    ));
    std::future::poll_fn(|cx| {
        assert!(pending.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert_eq!(
        reads.available_permits(),
        4,
        "class waiter consumed shared admission"
    );
    let cheap = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        admit_with_deadline_observed(reads, "read", None, std::time::Instant::now(), None, || {
            Ok(17)
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(cheap, 17);
    drop(pending);
    drop(held);
    drop(release);
    assert_eq!(cap.available_permits(), 1);
    assert_eq!(reads.available_permits(), 4);
}

#[tokio::test]
async fn same_deadline_covers_class_and_shared_queue() {
    for class_blocked in [true, false] {
        let cap = semaphore(1);
        let reads = semaphore(1);
        let held = if class_blocked {
            cap.acquire().await.unwrap()
        } else {
            reads.acquire().await.unwrap()
        };
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            read_with(
                cap,
                reads,
                std::time::Instant::now(),
                Some(quipu::time::Deadline::after_millis(20)),
                None,
                || -> Result<(), AppError> { panic!("expired query executed") },
            ),
        )
        .await
        .expect("admission ignored deadline");
        assert!(result.is_err());
        drop(held);
        assert_eq!(cap.available_permits(), 1);
        assert_eq!(reads.available_permits(), 1);
    }
}

#[tokio::test]
async fn cancelling_shared_wait_returns_class_capacity_without_running() {
    let cap = semaphore(1);
    let reads = semaphore(1);
    let held = reads.acquire().await.unwrap();
    let mut pending = Box::pin(read_with(
        cap,
        reads,
        std::time::Instant::now(),
        None,
        None,
        || -> Result<(), AppError> { panic!("cancelled waiter ran") },
    ));
    std::future::poll_fn(|cx| {
        assert!(pending.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert_eq!(cap.available_permits(), 0);
    drop(pending);
    assert_eq!(cap.available_permits(), 1);
    drop(held);
}

#[tokio::test]
async fn cancellation_of_running_query_keeps_both_permits_until_completion() {
    let cap = semaphore(1);
    let reads = semaphore(4);
    let (entered, started) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let running = tokio::spawn(read_with(
        cap,
        reads,
        std::time::Instant::now(),
        None,
        None,
        move || {
            let _ = entered.send(());
            wait.recv_timeout(std::time::Duration::from_secs(3))
                .unwrap();
            Ok(())
        },
    ));
    started.await.unwrap();
    running.abort();
    assert!(running.await.unwrap_err().is_cancelled());
    assert_eq!(cap.available_permits(), 0);
    assert_eq!(reads.available_permits(), 3);
    release.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while cap.available_permits() != 1 || reads.available_permits() != 4 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
