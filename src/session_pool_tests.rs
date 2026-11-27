//! aegis-wjo7: the embedding session pool. No sleeps: availability is proven
//! with `try_checkout`, concurrency with barriers that only a pool admitting
//! both holders at once can pass.
use super::*;
use std::sync::{
    Arc, Barrier,
    atomic::{AtomicUsize, Ordering},
};

fn pool(n: usize) -> SessionPool<usize> {
    SessionPool::new((0..n).collect()).unwrap()
}

#[test]
fn a_pool_is_one_to_max_sessions() {
    assert!(SessionPool::<usize>::new(vec![]).is_err());
    assert!(SessionPool::new((0..=MAX_SESSIONS).collect::<Vec<_>>()).is_err());
    assert_eq!(pool(MAX_SESSIONS).len(), MAX_SESSIONS);
}

/// Pool of 1 = today's single Mutex: every lane gets the one session, and
/// while anyone holds it nobody else, of either lane, gets anything.
#[test]
fn a_pool_of_one_is_the_single_session_for_every_lane() {
    let p = pool(1);
    for lane in [Lane::Interactive, Lane::Background] {
        let held = p.checkout(lane);
        assert_eq!(*held, 0);
        assert_eq!(held.index(), 0);
        assert!(p.try_checkout(Lane::Interactive).is_none(), "{lane:?}");
        assert!(p.try_checkout(Lane::Background).is_none(), "{lane:?}");
        drop(held);
        assert!(p.try_checkout(lane).is_some(), "released for {lane:?}");
    }
    // The single-session path keeps no lane state: a background holder is
    // just the holder of the one lock.
    let state = p.state.lock();
    assert!(state.free.len() == 1 && !state.background);
}

/// Pool of 2: a resolve (R) and a hybrid search (H) hold DIFFERENT sessions at
/// the same time. The try_checkout half proves it without blocking; then two
/// threads each wait at a barrier WHILE holding a session, which only a pool
/// admitting both at once can pass.
#[test]
fn with_two_sessions_r_and_h_run_concurrently() {
    let p = pool(2);
    let r = p.checkout(Lane::Interactive);
    let h = p
        .try_checkout(Lane::Interactive)
        .expect("a second session is free");
    assert_ne!(r.index(), h.index());
    assert!(p.try_checkout(Lane::Interactive).is_none(), "only two");
    drop((r, h));

    let p = Arc::new(pool(2));
    let both_held = Arc::new(Barrier::new(2));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let (p, both_held) = (p.clone(), both_held.clone());
            std::thread::spawn(move || {
                let session = p.checkout(Lane::Interactive);
                both_held.wait(); // passes only if both hold a session at once
                session.index()
            })
        })
        .collect();
    let mut held: Vec<usize> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    held.sort_unstable();
    assert_eq!(held, vec![0, 1]);
}

/// The write-embedding drain never holds more than one session, and while it
/// holds one, interactive callers still get every other session.
#[test]
fn the_drain_never_holds_more_than_one_session() {
    let p = pool(3);
    let drain = p.checkout(Lane::Background);
    assert!(
        p.try_checkout(Lane::Background).is_none(),
        "a second drain waits even with two sessions free"
    );
    let a = p
        .try_checkout(Lane::Interactive)
        .expect("reads keep a session");
    let b = p.try_checkout(Lane::Interactive).expect("and the last one");
    assert!(p.try_checkout(Lane::Interactive).is_none());
    drop(drain);
    assert!(
        p.try_checkout(Lane::Interactive).is_some(),
        "the drain's session returns"
    );
    drop((a, b));
    assert!(
        p.try_checkout(Lane::Background).is_some(),
        "drain lane reopens"
    );

    // Under contention: many drain threads, a running maximum of concurrent
    // drain holders, and reads that must always find a session.
    let p = Arc::new(pool(2));
    let (now, max) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let drains: Vec<_> = (0..8)
        .map(|_| {
            let (p, now, max) = (p.clone(), now.clone(), max.clone());
            std::thread::spawn(move || {
                for _ in 0..200 {
                    let _held = p.checkout(Lane::Background);
                    let n = now.fetch_add(1, Ordering::SeqCst) + 1;
                    max.fetch_max(n, Ordering::SeqCst);
                    now.fetch_sub(1, Ordering::SeqCst);
                }
            })
        })
        .collect();
    for _ in 0..200 {
        // A read can always get a session: the drain holds at most one of two.
        let read = p.checkout(Lane::Interactive);
        drop(read);
    }
    for d in drains {
        d.join().unwrap();
    }
    assert_eq!(max.load(Ordering::SeqCst), 1);
}
