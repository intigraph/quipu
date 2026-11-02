use std::sync::mpsc;
use std::time::Duration;

use super::*;

fn latch() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = path_for(&dir.path().join("lineage.journal"));
    (dir, path)
}

#[test]
fn shared_holders_coexist() {
    let (_dir, path) = latch();
    let a = shared(&path).unwrap();
    let b = try_shared(&path).unwrap();
    drop((a, b));
}

#[test]
fn retirement_refuses_hook_admission_without_waiting() {
    let (_dir, path) = latch();
    let held = exclusive(&path).unwrap();
    let err = try_shared(&path).unwrap_err().to_string();
    assert!(err.contains("held by retirement"), "{err}");
    drop(held);
    try_shared(&path).unwrap();
}

#[test]
fn retirement_waits_for_an_admitted_writer() {
    let (_dir, path) = latch();
    let writer = shared(&path).unwrap();
    let (tx, rx) = mpsc::channel();
    let retire_path = path.clone();
    let retirement = std::thread::spawn(move || {
        let held = exclusive(&retire_path).unwrap();
        tx.send(()).unwrap();
        drop(held);
    });
    assert!(
        rx.recv_timeout(Duration::from_millis(300)).is_err(),
        "retirement took the latch while a writer held it"
    );
    drop(writer);
    rx.recv_timeout(Duration::from_secs(10))
        .expect("retirement proceeds once the writer finishes");
    retirement.join().unwrap();
}
