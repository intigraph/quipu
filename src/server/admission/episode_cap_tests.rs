use super::*;

#[tokio::test]
async fn episode_deadline_covers_wait_for_ordinary_read_capacity() {
    let episodes = Box::leak(Box::new(tokio::sync::Semaphore::new(1)));
    let reads = Box::leak(Box::new(tokio::sync::Semaphore::new(1)));
    let held = reads.acquire().await.unwrap();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        episode_read_with(
            episodes,
            reads,
            std::time::Instant::now(),
            Some(quipu::time::Deadline::after_millis(20)),
            || -> Result<(), AppError> { panic!("expired episode ran") },
        ),
    )
    .await;
    assert!(
        result
            .expect("read admission ignored episode deadline")
            .is_err()
    );
    assert_eq!(episodes.available_permits(), 1);
    drop(held);
}

#[tokio::test]
async fn cancelled_running_episode_keeps_cap_and_waiter_leaves_reads_free() {
    let episodes: &'static tokio::sync::Semaphore =
        Box::leak(Box::new(tokio::sync::Semaphore::new(1)));
    let reads: &'static tokio::sync::Semaphore =
        Box::leak(Box::new(tokio::sync::Semaphore::new(4)));
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let running = tokio::spawn(episode_read_with(
        episodes,
        reads,
        std::time::Instant::now(),
        None,
        move || {
            let _ = entered_tx.send(());
            release_rx.recv().unwrap();
            Ok(())
        },
    ));
    entered_rx.await.unwrap();
    running.abort();
    assert!(running.await.unwrap_err().is_cancelled());
    let result = episode_read_with(
        episodes,
        reads,
        std::time::Instant::now(),
        Some(quipu::time::Deadline::after_millis(20)),
        || -> Result<(), AppError> { panic!("queued episode ran") },
    )
    .await;
    assert!(result.is_err());
    assert_eq!(episodes.available_permits(), 0);
    assert_eq!(reads.available_permits(), 3);
    let ordinary = admit_with_deadline(reads, "read", None, std::time::Instant::now(), || Ok(7))
        .await
        .unwrap();
    assert_eq!(ordinary, 7);
    release_tx.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while episodes.available_permits() != 1 || reads.available_permits() != 4 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        episode_read_with(episodes, reads, std::time::Instant::now(), None, || Ok(9))
            .await
            .unwrap(),
        9
    );
}
