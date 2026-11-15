//! Retention must respect config `read_only`, not just replica CLI mode.
#![cfg(all(feature = "shacl", feature = "onnx", feature = "server"))]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use quipu::vector::KnowledgeVectorStore;
use rusqlite::types::Value;

static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
const OLD: &str = "2000-01-01T00:00:00Z";
const OLD_EVENTS: &str = "SELECT * FROM events WHERE ts = '2000-01-01T00:00:00Z' ORDER BY offset";
const ALL_VECTORS: &str = "SELECT rowid, * FROM vectors ORDER BY rowid";
const CURRENT_VECTORS: &str = "SELECT rowid, * FROM vectors WHERE valid_to IS NULL ORDER BY rowid";

fn rows(root: &Path, sql: &str) -> Vec<Vec<Value>> {
    let db = rusqlite::Connection::open_with_flags(
        root.join("store.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let mut statement = db.prepare(sql).unwrap();
    let columns = statement.column_count();
    statement
        .query_map([], |row| {
            (0..columns).map(|column| row.get(column)).collect()
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn fixture(read_only: bool, events: u32, vectors: u32) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let mut store = quipu::Store::open(root.path().join("store.db").to_str().unwrap()).unwrap();
    let episode: quipu::episode::Episode = serde_json::from_value(serde_json::json!({
        "name": "retention-fixture",
        "nodes": [{"name": "retention-entity", "type": "T"}]
    }))
    .unwrap();
    quipu::episode::ingest_episode(&mut store, &episode, OLD, quipu::namespace::DEFAULT_BASE_NS)
        .unwrap();
    store.embed_entity(1, "old", &[0.5; 4], OLD).unwrap();
    store.close_embedding(1, "2000-01-02T00:00:00Z").unwrap();
    store
        .embed_entity(1, "current", &[0.6; 4], "2000-01-02T00:00:00Z")
        .unwrap();
    store.embed_entity(2, "current", &[0.7; 4], OLD).unwrap();
    drop(store);
    std::fs::create_dir(root.path().join(".bobbin")).unwrap();
    std::fs::write(
        root.path().join(".bobbin/config.toml"),
        format!(
            "[quipu]\nbase_ns = '{}'\n[quipu.server]\nread_only = {read_only}\nauth_token = 'retention-fixture-bearer'\n[quipu.events]\nretention_days = {events}\n[quipu.embedding]\nsuperseded_retention_days = {vectors}\nsuperseded_prune_max_per_pass = 8\n",
            quipu::namespace::DEFAULT_BASE_NS,
        ),
    )
    .unwrap();
    assert!(!rows(root.path(), OLD_EVENTS).is_empty());
    assert_eq!(rows(root.path(), ALL_VECTORS).len(), 3);
    assert_eq!(rows(root.path(), CURRENT_VECTORS).len(), 2);
    root
}

struct Server {
    child: Child,
    address: std::net::SocketAddr,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Server {
    fn request(&self, method: &str, path: &str, body: &str) -> std::io::Result<u16> {
        let mut socket = TcpStream::connect_timeout(&self.address, Duration::from_millis(250))?;
        socket.set_read_timeout(Some(Duration::from_secs(1)))?;
        write!(
            socket,
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nAuthorization: Bearer retention-fixture-bearer\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )?;
        let mut response = String::new();
        socket.read_to_string(&mut response)?;
        response
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| std::io::Error::other("missing HTTP status"))
    }
}

fn spawn(root: &Path) -> Server {
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let log = std::fs::File::create(root.join("server.log")).unwrap();
    let executable = std::env::var_os("L64_REGRESSION_SERVER")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_quipu-server").into());
    let child = Command::new(executable)
        // Deliberately no --read-only flag: that selects a different replica path.
        .args(["--bind", &address.to_string(), "--db"])
        .arg(root.join("store.db"))
        .current_dir(root)
        .env_clear()
        .env("HOME", root)
        .env("TOKIO_WORKER_THREADS", "2")
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()
        .unwrap();
    let mut server = Server { child, address };
    let until = Instant::now() + Duration::from_secs(15);
    while Instant::now() < until {
        assert!(
            server.child.try_wait().unwrap().is_none(),
            "server exited: {}",
            std::fs::read_to_string(root.join("server.log")).unwrap()
        );
        if server.request("GET", "/health", "").ok() == Some(200) {
            return server;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("server did not become healthy");
}

fn check(read_only: bool, events: u32, vectors: u32) {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = fixture(read_only, events, vectors);
    let old_events = rows(root.path(), OLD_EVENTS);
    let old_vectors = rows(root.path(), ALL_VECTORS);
    let current_vectors = rows(root.path(), CURRENT_VECTORS);
    assert!(
        !old_events.is_empty(),
        "eligible event fixture must be nonempty"
    );
    assert!(
        old_vectors.len() > current_vectors.len(),
        "eligible old vector fixture must exist"
    );
    let mut server = spawn(root.path());
    if read_only {
        assert_eq!(
            server
                .request("POST", "/knot", r#"{"turtle":"<urn:x> <urn:p> <urn:o> ."}"#)
                .unwrap(),
            403,
            "the fixture must reach configured read-only serving"
        );
    }
    let prune_events = !read_only && events > 0;
    let prune_vectors = !read_only && vectors > 0;
    let expected_events = if prune_events { Vec::new() } else { old_events };
    let expected_vectors = if prune_vectors {
        current_vectors
    } else {
        old_vectors
    };
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let actual_events = rows(root.path(), OLD_EVENTS);
        let actual_vectors = rows(root.path(), ALL_VECTORS);
        if read_only || (events == 0 && vectors == 0) {
            assert_eq!(actual_events, expected_events, "old event rows changed");
            assert_eq!(actual_vectors, expected_vectors, "vector rows changed");
        } else if actual_events == expected_events && actual_vectors == expected_vectors {
            break;
        }
        if Instant::now() >= until {
            assert_eq!(
                actual_events, expected_events,
                "enabled event retention did not prune"
            );
            assert_eq!(
                actual_vectors, expected_vectors,
                "enabled vector retention did not prune"
            );
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        server.child.try_wait().unwrap().is_none(),
        "owned server exited"
    );
    assert_eq!(server.request("GET", "/health", "").unwrap(), 200);
}

#[test]
fn configured_readonly_keeps_eligible_events() {
    check(true, 30, 0);
}

#[test]
fn configured_readonly_keeps_eligible_vectors() {
    check(true, 0, 30);
}

#[test]
fn writable_event_retention_prunes_events_and_keeps_vectors() {
    check(false, 30, 0);
}

#[test]
fn writable_vector_retention_prunes_old_versions_and_keeps_events() {
    check(false, 0, 30);
}

#[test]
fn zero_retention_keeps_both_row_sets_on_a_writable_server() {
    check(false, 0, 0);
}
