use super::*;
use std::{sync::mpsc, time::Duration};

async fn count(owner: &OwnerWriteAdmission, expected: usize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while owner.reserved.load(Ordering::Acquire) != expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("owner reservation did not reach expected state");
}

#[tokio::test]
async fn another_owner_progresses_while_legacy_permit_and_first_owner_are_held() {
    let legacy = super::super::WRITE_ADMISSION.acquire().await.unwrap();
    let a = Arc::new(OwnerWriteAdmission::new(1).unwrap());
    let b = Arc::new(OwnerWriteAdmission::new(1).unwrap());
    let (started, running) = tokio::sync::oneshot::channel();
    let (release, held) = mpsc::channel();
    let worker = tokio::spawn(async move {
        a.write(move || {
            started.send(()).unwrap();
            held.recv().unwrap();
            Ok(())
        })
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .unwrap()
        .unwrap();
    let completed = tokio::time::timeout(Duration::from_secs(5), b.write(|| Ok(7))).await;
    release.send(()).unwrap();
    drop(legacy);
    worker.await.unwrap().unwrap();
    assert_eq!(
        completed
            .expect("B entered the unrelated legacy/A queue")
            .unwrap(),
        7
    );
    count(&b, 0).await;
}

#[tokio::test]
async fn queue_capacity_and_queued_cancellation_release_only_their_own_reservation() {
    let owner = Arc::new(OwnerWriteAdmission::new(1).unwrap());
    let (started, running) = tokio::sync::oneshot::channel();
    let (release, held) = mpsc::channel();
    let first = {
        let owner = owner.clone();
        tokio::spawn(async move {
            owner
                .write(move || {
                    started.send(()).unwrap();
                    held.recv().unwrap();
                    Ok(())
                })
                .await
        })
    };
    running.await.unwrap();
    let queued = {
        let owner = owner.clone();
        tokio::spawn(async move { owner.write(|| Ok(())).await })
    };
    count(&owner, 2).await;
    assert!(
        owner
            .write(|| -> Result<(), AppError> { panic!("full queue dispatched work") })
            .await
            .is_err()
    );
    queued.abort();
    assert!(queued.await.unwrap_err().is_cancelled());
    count(&owner, 1).await;
    release.send(()).unwrap();
    first.await.unwrap().unwrap();
    count(&owner, 0).await;
    owner.write(|| Ok(())).await.unwrap();
}

#[tokio::test]
async fn cancellation_after_dispatch_retains_the_permit_until_actual_work_finishes() {
    let owner = Arc::new(OwnerWriteAdmission::new(1).unwrap());
    let (started, running) = tokio::sync::oneshot::channel();
    let (release, held) = mpsc::channel();
    let (ended, complete) = tokio::sync::oneshot::channel();
    let request = {
        let owner = owner.clone();
        tokio::spawn(async move {
            owner
                .write(move || {
                    started.send(()).unwrap();
                    held.recv().unwrap();
                    ended.send(()).unwrap();
                    Ok(())
                })
                .await
        })
    };
    running.await.unwrap();
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    assert_eq!(owner.reserved.load(Ordering::Acquire), 1);
    assert_eq!(
        owner.semaphore.available_permits(),
        0,
        "cancelled HTTP released an active worker"
    );
    release.send(()).unwrap();
    complete.await.unwrap();
    count(&owner, 0).await;
    owner.write(|| Ok(())).await.unwrap();
}

#[tokio::test]
async fn deadline_and_identity_use_the_shared_blocking_boundary() {
    let owner = Arc::new(OwnerWriteAdmission::new(1).unwrap());
    let called = Arc::new(AtomicUsize::new(0));
    let writes = called.clone();
    assert!(
        owner
            .write_with_deadline(
                Some(quipu::time::Deadline::after_millis(0)),
                Instant::now(),
                move || {
                    writes.fetch_add(1, Ordering::AcqRel);
                    Ok(())
                }
            )
            .await
            .is_err()
    );
    assert_eq!(called.load(Ordering::Acquire), 0);
    count(&owner, 0).await;
    let identity = quipu::transaction_auth::Identity {
        principal: "urn:test:owner-request".into(),
        credential_id: Some("stable-key".into()),
        auth_class: "named_bearer".into(),
        attestation: Default::default(),
    };
    let result = crate::auth::run_identified(
        Some(identity.clone()),
        owner.write(|| Ok(quipu::transaction_auth::current_identity().unwrap())),
    )
    .await
    .unwrap();
    assert_eq!(result.principal, identity.principal);
    assert_eq!(result.credential_id, identity.credential_id);
    assert_eq!(result.auth_class, identity.auth_class);
}

#[tokio::test]
async fn closed_owner_and_invalid_budget_refuse_without_dispatch() {
    assert!(OwnerWriteAdmission::new(0).is_err());
    assert!(OwnerWriteAdmission::new(usize::MAX).is_err());
    let owner = Arc::new(OwnerWriteAdmission::new(1).unwrap());
    owner.semaphore.close();
    assert!(
        owner
            .write(|| -> Result<(), AppError> { panic!("closed owner dispatched work") })
            .await
            .is_err()
    );
    count(&owner, 0).await;
}
