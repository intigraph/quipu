//! Real-server regression for primary-only replication authority in reader workers.
#![cfg(all(
    target_os = "linux",
    feature = "server",
    feature = "shacl",
    feature = "onnx"
))]

use sha2::{Digest, Sha256};
use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
const WRITER: &str = "private-writer-fixture";
const REPLICA: &str = "private-replication-fixture";

fn fixture(readers: usize) -> (tempfile::TempDir, SocketAddr, SocketAddr) {
    let root = tempfile::tempdir().unwrap();
    drop(quipu::Store::open(root.path().join("store.db").to_str().unwrap()).unwrap());
    let primary = TcpListener::bind("127.0.0.1:0").unwrap();
    let worker = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = primary.local_addr().unwrap();
    let worker_address = worker.local_addr().unwrap();
    let registry = root.path().join("replication.json");
    std::fs::write(
        &registry,
        serde_json::json!({"version":1,"credentials":[{
            "credential_id":"standby-fixture", "principal":"urn:replica:fixture",
            "audience":"quipu", "token_sha256":format!("{:x}", Sha256::digest(REPLICA.as_bytes()))
        }]})
        .to_string(),
    )
    .unwrap();
    std::fs::create_dir(root.path().join(".bobbin")).unwrap();
    std::fs::write(root.path().join(".bobbin/config.toml"), format!(
        "[quipu.server]\nauth_token={WRITER:?}\nreplication_credentials_file={:?}\nread_pool_size=0\nread_processes={readers}\nread_process_base_port={}\nmemory_budget_mib=256\n[quipu.events]\nretention_days=0\n[quipu.embedding]\nsuperseded_retention_days=0\n",
        registry.to_str().unwrap(), worker_address.port(),
    )).unwrap();
    (root, address, worker_address)
}

struct Server {
    child: Child,
    address: SocketAddr,
}

impl Drop for Server {
    fn drop(&mut self) {
        let children = std::fs::read_to_string(format!(
            "/proc/{}/task/{}/children",
            self.child.id(),
            self.child.id()
        ))
        .unwrap_or_default();
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Workers' existing parent-lifetime guard exits after the owned parent dies.
        for _ in 0..30 {
            let alive = children.split_whitespace().any(|pid| {
                std::fs::read_to_string(format!("/proc/{pid}/stat"))
                    .ok()
                    .and_then(|s| s.rsplit_once(')').map(|(_, tail)| tail.trim().to_owned()))
                    .is_some_and(|s| !s.starts_with('Z') && !s.starts_with('X'))
            });
            if !alive {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn spawn(root: &Path, address: SocketAddr, external_replica: bool) -> Server {
    let executable = std::env::var_os("D5M_REGRESSION_SERVER")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_quipu-server").into());
    let mut command = Command::new(executable);
    command
        .args(["--bind", &address.to_string(), "--db"])
        .arg(root.join("store.db"));
    if external_replica {
        command.args(["--read-only", "--primary-url", "http://127.0.0.1:9"]);
    }
    let child = command
        .current_dir(root)
        .env_clear()
        .env("HOME", root)
        .env("TOKIO_WORKER_THREADS", "2")
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(root.join("server.log")).unwrap())
        .spawn()
        .unwrap();
    Server { child, address }
}

fn request(
    address: SocketAddr,
    method: &str,
    path: &str,
    token: &str,
    body: &str,
) -> Result<ureq::Response, Box<ureq::Error>> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(2))
        .redirects(0)
        .build();
    let call = agent
        .request(method, &format!("http://{address}{path}"))
        .set("Content-Type", "application/json")
        .set("Authorization", &format!("Bearer {token}"));
    match call.send_string(body) {
        Ok(response) | Err(ureq::Error::Status(_, response)) => Ok(response),
        Err(error) => Err(Box::new(error)),
    }
}

fn ready(server: &mut Server, root: &Path) {
    let until = Instant::now() + Duration::from_secs(20);
    while Instant::now() < until {
        assert!(
            server.child.try_wait().unwrap().is_none(),
            "startup failed: {}",
            std::fs::read_to_string(root.join("server.log")).unwrap()
        );
        if request(server.address, "GET", "/health", WRITER, "").is_ok_and(|r| r.status() == 200) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "startup timed out: {}",
        std::fs::read_to_string(root.join("server.log")).unwrap()
    );
}

fn refused_start(server: &mut Server, root: &Path) {
    let until = Instant::now() + Duration::from_secs(20);
    while Instant::now() < until {
        if let Some(status) = server.child.try_wait().unwrap() {
            assert!(!status.success());
            let log = std::fs::read_to_string(root.join("server.log")).unwrap();
            assert!(
                log.contains("replication credential authority unavailable"),
                "unexpected startup refusal: {log}"
            );
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("configured invalid replication profile did not fail closed");
}

#[test]
fn registry_primary_with_internal_reader_starts_without_granting_replication_to_reader() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (root, address, worker) = fixture(1);
    let mut server = spawn(root.path(), address, false);
    ready(&mut server, root.path());
    let health = request(address, "GET", "/health", WRITER, "").unwrap();
    assert_eq!(health.header("x-quipu-read-processes-alive"), Some("1"));
    let query = r#"{"query":"SELECT (1 AS ?n) WHERE {}"}"#;
    let read = request(address, "POST", "/query", WRITER, query).unwrap();
    assert_eq!(read.status(), 200);
    let pid: u32 = read
        .header("x-quipu-read-worker-pid")
        .unwrap()
        .parse()
        .unwrap();
    assert_ne!(pid, server.child.id());
    let body: serde_json::Value = serde_json::from_str(&read.into_string().unwrap()).unwrap();
    assert_eq!(body["count"], 1);
    for path in ["/query", "/episode"] {
        let denied = request(address, "POST", path, REPLICA, query).unwrap();
        assert!(matches!(denied.status(), 401 | 403));
    }
    // Primary replication authority still exists: auth passes to body validation.
    let allowed = request(address, "POST", "/replicate/challenge", REPLICA, "{}").unwrap();
    assert!(matches!(allowed.status(), 400 | 422));
    assert_eq!(
        request(
            address,
            "POST",
            "/replicate/challenge",
            "unknown-fixture",
            "{}"
        )
        .unwrap()
        .status(),
        401
    );
    for path in ["/query", "/episode", "/replicate/challenge"] {
        assert_eq!(
            request(worker, "POST", path, WRITER, "{}")
                .unwrap()
                .status(),
            403
        );
    }
}

#[test]
fn configured_external_replica_still_fails_closed() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (root, address, _) = fixture(0);
    let mut server = spawn(root.path(), address, true);
    refused_start(&mut server, root.path());
}

#[test]
fn missing_primary_registry_still_fails_closed() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (root, address, _) = fixture(0);
    std::fs::remove_file(root.path().join("replication.json")).unwrap();
    let mut server = spawn(root.path(), address, false);
    refused_start(&mut server, root.path());
}
