//! # arming: library — private-file process contention, never a served database.
#![cfg(not(target_arch = "wasm32"))]

use quipu::{bindings::Engine, writer_lock::WriterLock};
use std::process::Command;

// Forked subprocesses briefly inherit other tests' open descriptors until exec.
// Serialize process fixtures so release assertions measure their own owner.
static PROCESS_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn writer_lock_child() {
    let Ok(path) = std::env::var("QUIPU_LOCK_TEST_PATH") else {
        return;
    };
    let opened = Engine::open(&path);
    if std::env::var("QUIPU_LOCK_TEST_EXPECT").unwrap() == "refuse" {
        let error = opened.err().expect("second process must refuse");
        assert!(error.to_string().contains("single-writer lock"), "{error}");
    } else {
        assert!(opened.is_ok(), "owner exit must release the lock");
    }
}

fn child(path: &std::path::Path, expect: &str) {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "writer_lock_child", "--nocapture"])
        .env("QUIPU_LOCK_TEST_PATH", path)
        .env("QUIPU_LOCK_TEST_EXPECT", expect)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn process_refuses_owner_then_opens_after_release() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private.db");
    // Same guard as the server takes, before any SQLite open/migration.
    let guard = WriterLock::acquire(path.to_str().unwrap()).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
    child(&path, "refuse");
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
    // Unrelated databases and memory engines are independent.
    let other = dir.path().join("other.db");
    child(&other, "open");
    assert!(Engine::open_in_memory().is_ok());
    drop(guard);
    child(&path, "open");
}

#[test]
fn binding_blocks_server_guard_and_other_bindings_until_drop() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("binding.db");
    let path = path.to_str().unwrap();
    let engine = Engine::open(path).unwrap();
    assert!(WriterLock::acquire(path).is_err());
    assert!(Engine::open(path).is_err());
    child(std::path::Path::new(path), "refuse");
    drop(engine);
    let guard = WriterLock::acquire(path).unwrap();
    drop(guard);
    assert!(Engine::open(path).is_ok());
}

#[cfg(unix)]
#[test]
fn symlinks_and_hard_links_share_the_lock() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("owner.db");
    let guard = WriterLock::acquire(path.to_str().unwrap()).unwrap();
    let hard = dir.path().join("hard.db");
    let sym = dir.path().join("sym.db");
    std::fs::hard_link(&path, &hard).unwrap();
    std::os::unix::fs::symlink(&path, &sym).unwrap();
    child(&hard, "refuse");
    child(&sym, "refuse");
    drop(guard);
    child(&sym, "open");
}

#[test]
fn failed_sqlite_open_releases_ownership_without_truncation() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invalid.db");
    let bytes = b"not a SQLite database";
    std::fs::write(&path, bytes).unwrap();
    assert!(Engine::open(path.to_str().unwrap()).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(WriterLock::acquire(path.to_str().unwrap()).is_ok());
}

#[test]
fn memory_and_uri_paths_cannot_create_an_unrelated_lock_file() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    assert_eq!(
        Engine::open(":memory:").unwrap().capabilities()["storage"],
        "memory"
    );
    assert!(Engine::open("").is_ok());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("uri.db");
    assert!(Engine::open(&format!("file:{}?mode=rwc", path.display())).is_err());
    assert!(!path.exists());
}

#[cfg(all(feature = "server", feature = "onnx", feature = "shacl"))]
#[test]
fn real_server_and_binding_refuse_each_other_and_exit_releases_lock() {
    let _serial = PROCESS_TESTS.lock().unwrap();
    use std::{
        fs::File,
        process::Stdio,
        time::{Duration, Instant},
    };
    struct Server(std::process::Child);
    impl Drop for Server {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("served.db");
    let log = dir.path().join("server.log");
    let spawn = || {
        Server(
            Command::new(env!("CARGO_BIN_EXE_quipu-server"))
                .args(["--db", path.to_str().unwrap(), "--bind", "127.0.0.1:0"])
                .current_dir(dir.path())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(File::create(&log).unwrap())
                .spawn()
                .unwrap(),
        )
    };
    let engine = Engine::open(path.to_str().unwrap()).unwrap();
    let mut refused = spawn();
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = refused.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "server must refuse promptly");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(!status.success());
    assert!(
        std::fs::read_to_string(&log)
            .unwrap()
            .contains("single-writer lock")
    );
    drop(refused);
    drop(engine);
    let mut server = spawn();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        assert!(
            server.0.try_wait().unwrap().is_none(),
            "{}",
            std::fs::read_to_string(&log).unwrap()
        );
        if std::fs::read_to_string(&log)
            .unwrap()
            .contains("listening on")
        {
            break;
        }
        assert!(Instant::now() < deadline, "server never listened");
        std::thread::sleep(Duration::from_millis(20));
    }
    child(&path, "refuse");
    // Abrupt termination must release the OS lock; no stale-file deletion.
    drop(server);
    child(&path, "open");
}
