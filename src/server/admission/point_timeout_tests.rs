use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[tokio::test]
async fn point_queue_expires_without_running_or_spending_another_budget() {
    let admission = Box::leak(Box::new(tokio::sync::Semaphore::new(1)));
    let held = admission.acquire().await.unwrap();
    let ran = Arc::new(AtomicBool::new(false));
    let flag = ran.clone();
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(250),
        admit_with_deadline(
            admission,
            "point-read",
            Some(quipu::time::Deadline::after_millis(20)),
            std::time::Instant::now(),
            move || {
                flag.store(true, Ordering::SeqCst);
                Ok(())
            },
        ),
    )
    .await
    .expect("queue must respect the original deadline, without waiting for its permit");
    assert!(result.is_err());
    assert!(!ran.load(Ordering::SeqCst));
    assert_eq!(admission.available_permits(), 0);
    drop(held);
    assert_eq!(admission.available_permits(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn busy_point_lane_does_not_admit_or_starve_ordinary_work() {
    let point = Box::leak(Box::new(tokio::sync::Semaphore::new(1)));
    let ordinary = Box::leak(Box::new(tokio::sync::Semaphore::new(1)));
    let held = point.acquire().await.unwrap();
    let mut waiters = Vec::new();
    for _ in 0..16 {
        waiters.push(tokio::spawn(admit_with_deadline(
            point,
            "point-read",
            Some(quipu::time::Deadline::after_millis(30)),
            std::time::Instant::now(),
            || Ok(()),
        )));
    }
    let regular = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        admit_with_deadline(ordinary, "read", None, std::time::Instant::now(), || Ok(7)),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(regular, 7);
    for waiter in waiters {
        assert!(waiter.await.unwrap().is_err());
    }
    assert_eq!(point.available_permits(), 0);
    drop(held);
    assert_eq!(point.available_permits(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disconnected_point_keeps_its_permit_until_blocking_work_finishes() {
    let admission = Box::leak(Box::new(tokio::sync::Semaphore::new(1)));
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let task = tokio::spawn(admit_with_deadline(
        admission,
        "point-read",
        Some(quipu::time::Deadline::after_millis(1000)),
        std::time::Instant::now(),
        move || {
            let _ = ready_tx.send(());
            let _ = release_rx.recv();
            Ok(())
        },
    ));
    ready_rx.await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(
        admission.available_permits(),
        0,
        "blocking work still owns the permit"
    );
    let queued = admit_with_deadline(
        admission,
        "point-read",
        Some(quipu::time::Deadline::after_millis(20)),
        std::time::Instant::now(),
        || Ok(()),
    )
    .await;
    assert!(
        queued.is_err(),
        "disconnect must not admit a second blocking point read"
    );
    release_tx.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while admission.available_permits() != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
