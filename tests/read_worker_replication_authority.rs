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
    request_operation(address, method, path, token, body, None)
}

fn request_operation(
    address: SocketAddr,
    method: &str,
    path: &str,
    token: &str,
    body: &str,
    operation: Option<&str>,
) -> Result<ureq::Response, Box<ureq::Error>> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(2))
        .redirects(0)
        .build();
    let mut call = agent
        .request(method, &format!("http://{address}{path}"))
        .set("Content-Type", "application/json")
        .set("Authorization", &format!("Bearer {token}"));
    if let Some(operation) = operation {
        call = call.set("x-quipu-operation", operation);
    }
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
fn capped_internal_reader_with_primary_registry_serves_both_lanes() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (root, address, worker) = fixture(1);
    let path = root.path().join(".bobbin/config.toml");
    let config = std::fs::read_to_string(&path)
        .unwrap()
        .replace("read_pool_size=0", "read_pool_size=4")
        .replace(
            "memory_budget_mib=256",
            "memory_budget_mib=512\nheavy_read_operations=['owner-rows']\nheavy_read_permits=1",
        );
    std::fs::write(path, config).unwrap();
    {
        let mut store = quipu::Store::open(root.path().join("store.db").to_str().unwrap()).unwrap();
        quipu::tool_knot(&mut store, &serde_json::json!({
            "turtle": "<urn:item:fixture> <urn:fixture:owner> <urn:owner:fixture> ; <urn:fixture:id> \"fixture\" ."
        })).unwrap();
    }
    let mut server = spawn(root.path(), address, false);
    ready(&mut server, root.path());
    let query = r#"{"query":"SELECT ?id WHERE { ?item <urn:fixture:owner> <urn:owner:fixture> ; <urn:fixture:id> ?id }"}"#;
    for operation in [Some("owner-rows"), None] {
        let response =
            request_operation(address, "POST", "/query", WRITER, query, operation).unwrap();
        assert_eq!(response.status(), 200);
        let pid: u32 = response
            .header("x-quipu-read-worker-pid")
            .unwrap()
            .parse()
            .unwrap();
        assert_ne!(pid, server.child.id());
        let body: serde_json::Value =
            serde_json::from_str(&response.into_string().unwrap()).unwrap();
        assert_eq!(body["count"], 1);
    }
    assert_eq!(
        request(worker, "POST", "/query", WRITER, query)
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        request(address, "POST", "/query", REPLICA, query)
            .unwrap()
            .status(),
        401
    );
}

#[test]
fn capped_worker_without_an_ordinary_reader_refuses_startup() {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (root, address, _) = fixture(1);
    let path = root.path().join(".bobbin/config.toml");
    let config = std::fs::read_to_string(&path)
        .unwrap()
        .replace("read_pool_size=0", "read_pool_size=1")
        .replace(
            "memory_budget_mib=256",
            "memory_budget_mib=512\nheavy_read_operations=['owner-rows']\nheavy_read_permits=1",
        );
    std::fs::write(path, config).unwrap();
    let mut server = spawn(root.path(), address, false);
    let until = Instant::now() + Duration::from_secs(20);
    while Instant::now() < until {
        if let Some(status) = server.child.try_wait().unwrap() {
            assert!(!status.success());
            let log = std::fs::read_to_string(root.path().join("server.log")).unwrap();
            assert!(
                log.contains("heavy reads require"),
                "unexpected refusal: {log}"
            );
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("unusable heavy reserve did not refuse startup");
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

// Exercise the public listener with real signatures, not bearer-only requests.
// The pool must neither consume a signed read twice nor silently serve it on
// the primary. Each accepted request is also replayed through the public route.
#[test]
fn signed_reads_and_writes_cross_the_full_reader_pool_without_replaying_nonces() {
    use base64::Engine as _;
    use quipu::session_attestation::{
        AttestationEnvelope, SessionBinding, SignedBinding, WRITE_V2, WriteBinding, body_sha256,
        canonical_message,
    };
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use std::collections::BTreeSet;

    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (root, address, worker) = fixture(2);
    let config_path = root.path().join(".bobbin/config.toml");
    let config = std::fs::read_to_string(&config_path)
        .unwrap()
        .replace("read_pool_size=0", "read_pool_size=4")
        .replace(
            "memory_budget_mib=256",
            "memory_budget_mib=768\nheavy_read_operations=['owner-rows']\nheavy_read_permits=1",
        );
    let config = format!(
        "{config}\n[quipu.server.session_keys]\nrequired=true\nonboarding_read_paths=['/health']\n"
    );
    std::fs::write(config_path, config).unwrap();
    let key = Ed25519KeyPair::from_seed_unchecked(&[73; 32]).unwrap();
    let now = quipu::time::epoch_secs();
    let mut binding = SessionBinding::new(
        "urn:fixture:signed-reader",
        "signed-pool-fixture",
        hex::encode(key.public_key().as_ref()),
        "urn:fixture:introducer",
        now - 10,
        now + 3600,
    )
    .unwrap();
    binding.allow_write = true;
    let store = quipu::Store::open(root.path().join("store.db").to_str().unwrap()).unwrap();
    store.attestation_register(&binding).unwrap();
    let audience = store.store_id().unwrap();
    drop(store);
    let mut server = spawn(root.path(), address, false);
    ready(&mut server, root.path());
    assert_eq!(
        request(address, "GET", "/health", WRITER, "")
            .unwrap()
            .header("x-quipu-read-processes-alive"),
        Some("2")
    );

    let sign = |path: &str, body: &str, nonce: usize| {
        let mut envelope = AttestationEnvelope {
            version: WRITE_V2.into(),
            key_id: binding.key_id.clone(),
            session: binding.session.clone(),
            introducer: binding.introducer.clone(),
            issued_at_epoch: quipu::time::epoch_secs(),
            nonce: format!("{nonce:032x}"),
            signature: String::new(),
            audience: Some(audience.clone()),
        };
        let hash = body_sha256(body.as_bytes());
        envelope.signature = hex::encode(
            key.sign(&canonical_message(
                &envelope,
                &SignedBinding::Write(WriteBinding {
                    method: "POST",
                    path,
                    content_type: "application/json",
                    body_sha256: &hash,
                    audience: Some(&audience),
                }),
            ))
            .as_ref(),
        );
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&envelope).unwrap())
    };
    let send =
        |target: SocketAddr, path: &str, body: &str, signature: &str, operation: Option<&str>| {
            let agent = ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(3))
                .redirects(0)
                .build();
            let mut call = agent
                .post(&format!("http://{target}{path}"))
                .set("Content-Type", "application/json")
                .set("x-quipu-attestation", signature);
            if let Some(operation) = operation {
                call = call.set("x-quipu-operation", operation);
            }
            match call.send_string(body) {
                Ok(response) | Err(ureq::Error::Status(_, response)) => response,
                Err(error) => panic!("private fixture transport failed: {error}"),
            }
        };
    let write = r#"{"turtle":"<urn:signed:item> <urn:signed:value> \"present\" ."}"#;
    let signature = sign("/knot", write, 1);
    let response = send(address, "/knot", write, &signature, None);
    assert_eq!(
        response.status(),
        200,
        "signed write must succeed on primary"
    );
    assert!(response.header("x-quipu-read-worker-pid").is_none());
    assert_eq!(
        send(address, "/knot", write, &signature, None).status(),
        401
    );

    let query = r#"{"query":"SELECT ?v WHERE { <urn:signed:item> <urn:signed:value> ?v }"}"#;
    let mut seen = BTreeSet::new();
    for nonce in 2..8 {
        let signature = sign("/query", query, nonce);
        let response = send(address, "/query", query, &signature, None);
        assert_eq!(
            response.status(),
            200,
            "fresh signed read must not be reverified as a replay by its child"
        );
        let pid: u32 = response
            .header("x-quipu-read-worker-pid")
            .expect("must cross a real worker")
            .parse()
            .unwrap();
        assert_ne!(pid, server.child.id());
        seen.insert(pid);
        let body: serde_json::Value =
            serde_json::from_str(&response.into_string().unwrap()).unwrap();
        assert_eq!(body["count"], 1);
        assert!(body.to_string().contains("present"));
        assert_eq!(
            send(address, "/query", query, &signature, None).status(),
            401
        );
    }
    assert_eq!(
        seen.len(),
        2,
        "every configured worker must actually serve a signed read"
    );
    let signature = sign("/query", query, 8);
    let heavy = send(address, "/query", query, &signature, Some("owner-rows"));
    assert_eq!(heavy.status(), 200);
    assert!(
        seen.contains(
            &heavy
                .header("x-quipu-read-worker-pid")
                .unwrap()
                .parse::<u32>()
                .unwrap()
        )
    );
    assert_eq!(
        send(address, "/query", query, &signature, Some("owner-rows")).status(),
        401
    );
    // Exact-byte binding must still refuse altered bodies, and never fall back.
    let signature = sign("/query", query, 9);
    assert_eq!(
        send(address, "/query", "{}", &signature, None).status(),
        401
    );
    // A public caller cannot turn a serialized identity into trusted evidence,
    // even while also supplying the existing transport bearer.
    let spoof = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
        serde_json::json!({"principal":binding.agent,"credential_id":binding.key_id,
            "auth_class":"attested_session"})
        .to_string(),
    );
    let public = ureq::post(&format!("http://{address}/query"))
        .set("Content-Type", "application/json")
        .set("Authorization", &format!("Bearer {WRITER}"))
        .set("x-quipu-read-worker-identity", &spoof)
        .send_string(query);
    assert!(matches!(public, Err(ureq::Error::Status(401, _))));
    // Concurrent copies must still be arbitrated once by the primary ledger.
    let signature = sign("/query", query, 10);
    let mut statuses = std::thread::scope(|scope| {
        let one = scope.spawn(|| send(address, "/query", query, &signature, None).status());
        let two = scope.spawn(|| send(address, "/query", query, &signature, None).status());
        vec![one.join().unwrap(), two.join().unwrap()]
    });
    statuses.sort_unstable();
    assert_eq!(statuses, [200, 401]);
    // Corrupt the signature without changing any declared identity.
    let mut bad: serde_json::Value = serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(sign("/query", query, 11))
            .unwrap(),
    )
    .unwrap();
    bad["signature"] = serde_json::Value::String("00".repeat(64));
    let bad =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(&bad).unwrap());
    assert_eq!(send(address, "/query", query, &bad, None).status(), 401);
    // A signature for /query cannot be applied to another read endpoint.
    assert_eq!(
        send(address, "/ask", query, &sign("/query", query, 12), None).status(),
        401
    );
    for port in [worker.port(), worker.port() + 1] {
        let target = SocketAddr::new(worker.ip(), port);
        assert_eq!(
            send(
                target,
                "/query",
                query,
                &sign("/query", query, usize::from(port)),
                None
            )
            .status(),
            403
        );
    }
    let store = quipu::Store::open(root.path().join("store.db").to_str().unwrap()).unwrap();
    store.attestation_revoke(&binding.session).unwrap();
    assert_eq!(
        send(address, "/query", query, &sign("/query", query, 13), None).status(),
        401
    );
}
