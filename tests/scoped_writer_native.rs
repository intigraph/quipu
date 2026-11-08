//! Actual configured listener with store-API fixture enrollment, not a test router.
#![cfg(all(feature = "server", feature = "onnx", feature = "shacl"))]
use quipu::writer_scope::{WriteGrant, WriteOperation};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    net::TcpListener,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Serving(Child);
impl Drop for Serving {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn configured_scoped_listener_has_no_unrestricted_write_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("data.db");
    let mut store = quipu::Store::open(db.to_str().unwrap()).unwrap();
    store.set_base_ns("https://example.org/ontology/");
    store.graph_create("urn:native:allowed").unwrap();
    store.graph_create("urn:native:foreign").unwrap();
    let token = format!(
        "{:x}",
        Sha256::digest(dir.path().to_string_lossy().as_bytes())
    );
    let operator = format!(
        "{:x}",
        Sha256::digest(format!("{}-operator", dir.path().display()).as_bytes())
    );
    let registry = dir.path().join("writers.json");
    std::fs::write(
        &registry,
        serde_json::to_vec(&json!({"version":1,"credentials":[{
            "credential_id":"native-curator","principal":"urn:native:curator","audience":"quipu",
            "token_sha256":format!("{:x}",Sha256::digest(token.as_bytes()))
        }]}))
        .unwrap(),
    )
    .unwrap();
    let grant = WriteGrant {
        version: 1,
        id: "native-grant".into(),
        principal: "urn:native:curator".into(),
        credential_id: "native-curator".into(),
        instance: store.writer_graph_instance("urn:native:allowed").unwrap(),
        operations: BTreeSet::from([WriteOperation::Update]),
        valid_from: 0,
        valid_until: quipu::time::epoch_secs() + 3600,
        revision: 1,
        revoked: false,
        mint_scope: None,
    };
    // A deliberately has no owner CLI: enroll the isolated fixture through
    // the reviewed local Store API before starting the actual HTTP listener.
    store.writer_grant_apply(&grant).unwrap();
    drop(store);
    std::fs::create_dir(dir.path().join(".bobbin")).unwrap();
    std::fs::write(dir.path().join(".bobbin/config.toml"),format!(
        "[quipu]\nbase_ns = 'https://example.org/ontology/'\n[quipu.embedding]\nauto_embed = false\n[quipu.server]\nauth_token = '{operator}'\nscoped_writer_credentials_file = '{}'\n[quipu.governance]\nenforce_authority = true\n",
        registry.display())).unwrap();
    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = socket.local_addr().unwrap();
    drop(socket);
    let log = std::fs::File::create(dir.path().join("server.log")).unwrap();
    let mut serving = Serving(
        Command::new(env!("CARGO_BIN_EXE_quipu-server"))
            .current_dir(dir.path())
            .arg("--db")
            .arg(&db)
            .arg("--bind")
            .arg(address.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if ureq::get(&format!("http://{address}/health"))
            .timeout(Duration::from_millis(100))
            .call()
            .is_ok()
        {
            break;
        }
        assert!(
            serving.0.try_wait().unwrap().is_none(),
            "configured listener exited"
        );
        assert!(
            Instant::now() < deadline,
            "configured listener did not become healthy"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let request = |presented: &str, graph: &str| {
        let update = format!(
            "INSERT {{ GRAPH <{graph}> {{ <urn:native:s> <urn:native:p> 1 }} }} WHERE {{ FILTER NOT EXISTS {{ GRAPH <{graph}> {{ <urn:native:s> <urn:native:p> ?old }} }} }}"
        );
        let result = ureq::post(&format!(
            "http://{address}/update?actor=urn:native:impostor"
        ))
        .set("authorization", &format!("Bearer {presented}"))
        .set("content-type", "application/sparql-update")
        .send_string(&update);
        let response = match result {
            Ok(r) | Err(ureq::Error::Status(_, r)) => r,
            Err(e) => panic!("{e}"),
        };
        let status = response.status();
        (
            status,
            serde_json::from_str::<serde_json::Value>(&response.into_string().unwrap()).unwrap(),
        )
    };
    let invalid = request("invalid-fixture-presentation", "urn:native:allowed").0;
    println!("invalid bearer -> HTTP {invalid}");
    assert_eq!(invalid, 401);
    let (status, report) = request(&token, "urn:native:allowed");
    println!(
        "scoped allowed graph -> HTTP {status}, asserted={}",
        report["asserted"]
    );
    assert_eq!(status, 200, "{report}");
    assert_eq!(report["asserted"], 1);
    let read = quipu::Store::open_read_only(db.to_str().unwrap()).unwrap();
    let before = read.latest_tx_id().unwrap();
    let evidence = read.transaction_auth(before).unwrap().unwrap();
    assert_eq!(evidence.principal, "urn:native:curator");
    assert_eq!(evidence.auth_class, "scoped_named_bearer");
    let foreign = request(&token, "urn:native:foreign").0;
    println!("scoped foreign graph -> HTTP {foreign}");
    assert_eq!(foreign, 403);
    assert_eq!(read.latest_tx_id().unwrap(), before);
    let (status, retry) = request(&token, "urn:native:allowed");
    assert_eq!(status, 200);
    println!(
        "scoped allowed retry -> HTTP {status}, asserted={}",
        retry["asserted"]
    );
    assert_eq!(retry["asserted"], 0);
    assert!(retry["tx"].is_null());
    // Explicit compatibility witness: operator keys stay unrestricted; they
    // must never be advertised as restricted curator credentials.
    let compatibility = request(&operator, "urn:native:foreign").0;
    println!("unrestricted operator foreign graph -> HTTP {compatibility}");
    assert_eq!(compatibility, 200);
    drop(serving);
    // A missing restricted registry must refuse startup, never silently use
    // the configured unrestricted operator bearer as a fallback profile.
    std::fs::remove_file(registry).unwrap();
    let refused_log = dir.path().join("refused.log");
    let mut refused = Serving(
        Command::new(env!("CARGO_BIN_EXE_quipu-server"))
            .current_dir(dir.path())
            .arg("--db")
            .arg(&db)
            .arg("--bind")
            .arg(address.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&refused_log).unwrap())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = refused.0.try_wait().unwrap() {
            assert_eq!(status.code(), Some(2));
            assert!(
                std::fs::read_to_string(&refused_log)
                    .unwrap()
                    .contains("cannot open registry file")
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "missing scoped registry did not refuse startup"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
