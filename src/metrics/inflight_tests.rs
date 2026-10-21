use super::*;
use std::time::Duration;

#[test]
fn stalled_write_shape_is_visible_for_five_minutes_and_idle_restart_is_zero() {
    let registry = Arc::new(Registry::default());
    let started = Instant::now();
    let writes: Vec<_> = (0..35)
        .map(|_| registry.start_at(Lane::Write, Phase::Waiting, started))
        .collect();
    writes[0].activate();
    for seconds in [0, 60, 120, 180, 240, 300] {
        let (counts, age) = registry.snapshot(started + Duration::from_secs(seconds));
        assert_eq!(counts[Lane::Write.index()], [34, 1]);
        assert!(counts[Lane::Write.index()].iter().sum::<usize>() >= 10);
        assert_eq!(age, seconds as f64);
        println!(
            "stall-shape t={seconds}s waiting={} active={} oldest={age}s",
            counts[0][0], counts[0][1]
        );
    }
    drop(writes);
    assert_eq!(
        registry.snapshot(started + Duration::from_secs(301)),
        ([[0; 2]; 4], 0.0)
    );
    let restarted = Registry::default();
    assert_eq!(restarted.snapshot(started), ([[0; 2]; 4], 0.0));
    println!("stall-shape completed: waiting=0 active=0 oldest=0s");
    println!("stall-shape fresh-process control: waiting=0 active=0 oldest=0s");
}

#[test]
fn reads_do_not_age_writes_and_oldest_recomputes_after_completion() {
    let registry = Arc::new(Registry::default());
    let now = Instant::now();
    let _read = registry.start_at(Lane::Read, Phase::Active, now);
    let _point = registry.start_at(Lane::PointRead, Phase::Waiting, now);
    assert_eq!(registry.snapshot(now + Duration::from_secs(600)).1, 0.0);
    let old = registry.start_at(Lane::Write, Phase::Active, now);
    let young = registry.start_at(Lane::Write, Phase::Waiting, now + Duration::from_secs(100));
    assert_eq!(registry.snapshot(now + Duration::from_secs(300)).1, 300.0);
    drop(old);
    assert_eq!(registry.snapshot(now + Duration::from_secs(300)).1, 200.0);
    drop(young);
    assert_eq!(registry.snapshot(now + Duration::from_secs(300)).1, 0.0);
}

#[test]
fn cancelled_http_cannot_hide_detached_store_work() {
    let http = Arc::new(Registry::default());
    let store = Arc::new(Registry::default());
    let request = http.start(Lane::Write, Phase::Active);
    let work = store.start(Lane::Write, Phase::Waiting);
    let (began_tx, began_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        work.activate();
        let _work = work;
        began_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    });
    began_rx.recv().unwrap();
    drop(request);
    assert_eq!(http.snapshot(Instant::now()).0[0], [0, 0]);
    assert_eq!(store.snapshot(Instant::now()).0[0], [0, 1]);
    release_tx.send(()).unwrap();
    thread.join().unwrap();
    assert_eq!(store.snapshot(Instant::now()), ([[0; 2]; 4], 0.0));
}

#[test]
fn queued_cancellation_and_panic_clear_without_poisoning_the_registry() {
    let registry = Arc::new(Registry::default());
    drop(registry.start(Lane::Write, Phase::Waiting));
    let copy = registry.clone();
    let result = std::panic::catch_unwind(move || {
        let _work = copy.start(Lane::Write, Phase::Active);
        panic!("private work failure");
    });
    assert!(result.is_err());
    assert_eq!(registry.snapshot(Instant::now()), ([[0; 2]; 4], 0.0));
}

#[test]
fn exposition_has_explicit_zeroes_and_a_closed_label_set() {
    let registry = Arc::new(Registry::default());
    let _work = registry.start(Lane::Write, Phase::Waiting);
    let mut text = String::new();
    registry.render(&mut text, true);
    assert!(text.contains("# TYPE quipu_store_requests_in_flight gauge"));
    assert!(text.contains("quipu_store_requests_in_flight{lane=\"write\",phase=\"waiting\"} 1"));
    assert!(text.contains("quipu_store_requests_in_flight{lane=\"write\",phase=\"active\"} 0"));
    assert_eq!(
        text.lines()
            .filter(|s| s.starts_with("quipu_store_requests_in_flight{"))
            .count(),
        6
    );
    let mut http = String::new();
    registry.render(&mut http, false);
    assert_eq!(
        http.lines()
            .filter(|s| s.starts_with("quipu_http_requests_in_flight{"))
            .count(),
        3
    );
    assert!(!http.contains("phase="));
}
