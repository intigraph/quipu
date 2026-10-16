use super::*;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

#[test]
fn admission_observation_separates_held_permit_from_blocking_startup() {
    for (name, admission_delay, startup_delay) in [
        ("uncontended", false, false),
        ("held_admission", true, false),
        ("held_startup", false, true),
    ] {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .unwrap();
        runtime.block_on(async {
            let admission: &'static Semaphore = Box::leak(Box::new(Semaphore::new(1)));
            let held = if admission_delay {
                Some(admission.acquire().await.unwrap())
            } else {
                None
            };
            let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
            let blocker = if startup_delay {
                let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
                let task = tokio::task::spawn_blocking(move || {
                    ready_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                });
                ready_rx.await.unwrap();
                Some(task)
            } else {
                drop(release_rx);
                None
            };
            let stamp = Arc::new(OnceLock::new());
            let release_stamp = stamp.clone();
            let released = Arc::new(OnceLock::new());
            let release_mark = released.clone();
            let release = tokio::spawn(async move {
                if admission_delay || startup_delay {
                    tokio::time::sleep(Duration::from_millis(120)).await;
                }
                if admission_delay {
                    assert!(release_stamp.get().is_none());
                } else if startup_delay {
                    assert!(release_stamp.get().is_some());
                }
                release_mark.set(Instant::now()).unwrap();
                drop(held);
                if startup_delay {
                    release_tx.send(()).unwrap();
                }
            });
            let start = Instant::now();
            let entered = tokio::time::timeout(
                Duration::from_secs(10),
                admit_with_deadline_observed(
                    admission,
                    "read",
                    None,
                    start,
                    Some(stamp.clone()),
                    || Ok(Instant::now()),
                ),
            )
            .await
            .unwrap()
            .unwrap();
            let acquired = *stamp.get().expect("positive permit acquisition control");
            let admission_ms = (acquired - start).as_secs_f64() * 1000.0;
            let startup_ms = (entered - acquired).as_secs_f64() * 1000.0;
            println!(
                "{}",
                serde_json::json!({"case":name,"admission_ms":admission_ms,
                "post_admission_startup_ms":startup_ms})
            );
            release.await.unwrap();
            if let Some(blocker) = blocker {
                blocker.await.unwrap();
            }
            if admission_delay {
                assert!(admission_ms >= 100.0);
                assert!(acquired >= *released.get().unwrap());
            } else if startup_delay {
                assert!(startup_ms >= 100.0);
                assert!(acquired <= *released.get().unwrap());
            } else {
                assert!(entered >= acquired);
            }
            assert_eq!(admission.available_permits(), 1);
        });
    }
}

#[tokio::test]
async fn cancelled_or_closed_admission_never_stamps_or_runs_work() {
    let admission: &'static Semaphore = Box::leak(Box::new(Semaphore::new(1)));
    let held = admission.acquire().await.unwrap();
    let stamp = Arc::new(OnceLock::new());
    let result = tokio::time::timeout(
        Duration::from_millis(20),
        admit_with_deadline_observed(
            admission,
            "read",
            None,
            Instant::now(),
            Some(stamp.clone()),
            || -> Result<(), AppError> { panic!("cancelled waiter must not enter blocking work") },
        ),
    )
    .await;
    assert!(result.is_err());
    assert!(stamp.get().is_none());
    drop(held);
    admission.close();
    let result = admit_with_deadline_observed(
        admission,
        "read",
        None,
        Instant::now(),
        Some(stamp.clone()),
        || Ok(()),
    )
    .await;
    assert!(result.is_err());
    assert!(stamp.get().is_none());
}
