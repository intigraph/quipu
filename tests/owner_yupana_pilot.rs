//! Real enrolled pilot and external candidate Yupana qualification.
//! Explicitly ignored: run with YUPANA_OWNER_CANDIDATE naming a built candidate.
#![cfg(all(
    unix,
    feature = "shacl",
    feature = "onnx",
    feature = "server",
    feature = "ownership-witness-s3"
))]

#[path = "../src/server/owner_pilot_s3_fixture.rs"]
mod s3;
#[path = "../src/ownership_handoff_model/witness/s3_tls_bridge.rs"]
mod tls;

use quipu::{
    Store,
    writer_scope::{WriteGrant, WriteOperation},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const GRAPH: &str = "https://example.test/graphs/yupana-pilot";
// Synthetic fixture identities only; never use deployment credentials.
const TOKEN: &str = "fixture-yupana-owner-only";
const PRINCIPAL: &str = "urn:test:yupana-owner";
const CREDENTIAL: &str = "fixture-yupana";

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn private(path: &Path, bytes: impl AsRef<[u8]>) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}
fn cli(root: &Path, args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_quipu"))
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("HOME", root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "pilot command failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
fn checkpoint(path: &Path) {
    rusqlite::Connection::open(path)
        .unwrap()
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
}
fn hash(path: &Path) -> String {
    hex::encode(Sha256::digest(fs::read(path).unwrap()))
}
fn count(path: &Path) -> i64 {
    rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap()
        .query_row("SELECT count(*) FROM facts WHERE op = 1", [], |r| r.get(0))
        .unwrap()
}

struct Lab {
    root: tempfile::TempDir,
    endpoint: String,
    _emulator: s3::Emulator,
    _tls: tls::TlsBridge,
    server: Option<Running>,
}
impl Lab {
    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let r = root.path().canonicalize().unwrap();
        let emulator = s3::Emulator::start();
        let tls = tls::TlsBridge::start(&emulator.endpoint);
        private(&r.join("ca.der"), &tls.der);
        // Before opaque enrollment UNKNOWN, expose the exact verified TLS or
        // HTTP failure. An unsigned listing must reach the emulator and refuse.
        let mut roots = ureq::rustls::RootCertStore::empty();
        roots.add(tls.der.clone().into()).unwrap();
        let config = ureq::rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let probe = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(2))
            .redirects(0)
            .tls_config(std::sync::Arc::new(config))
            .build();
        match probe
            .get(&format!("{}/witness?list-type=2", tls.endpoint))
            .call()
        {
            Err(ureq::Error::Status(403, _)) => {
                eprintln!("witness verified-TLS preflight reached unsigned-request refusal")
            }
            result => panic!("witness verified-TLS preflight expected unsigned403: {result:?}"),
        }

        private(&r.join("witness.json"), br#"{"access_key_id":"AKIAIOSFODNN7EXAMPLE","secret_access_key":"wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"}"#);
        private(&r.join("credential"), TOKEN);
        private(
            &r.join("registry.json"),
            json!({"version":1,"credentials":[{
            "credential_id":CREDENTIAL,"principal":PRINCIPAL,"audience":"quipu",
            "token_sha256":hex::encode(Sha256::digest(TOKEN.as_bytes()))}]})
            .to_string(),
        );
        let owner_path = r.join("owner.db");
        let owner = Store::open(owner_path.to_str().unwrap()).unwrap();
        owner.graph_create(GRAPH).unwrap();
        drop(owner);
        Store::adopt(owner_path.to_str().unwrap()).unwrap();
        let owner = Store::open(owner_path.to_str().unwrap()).unwrap();
        // Trusted disposable initializer, before enrollment/mount, never a second
        // writable Store opened behind a live owner.
        owner
            .writer_grant_apply(&WriteGrant {
                version: 1,
                id: "fixture-grant".into(),
                principal: PRINCIPAL.into(),
                credential_id: CREDENTIAL.into(),
                instance: owner.writer_graph_instance(GRAPH).unwrap(),
                operations: BTreeSet::from([WriteOperation::Knot]),
                valid_from: 0,
                valid_until: quipu::time::epoch_secs() + 3600,
                revision: 1,
                revoked: false,
                mint_scope: None,
            })
            .unwrap();
        let owner_id = owner.store_id().unwrap();
        let epoch = owner.log_epoch().unwrap();
        drop(owner);
        let legacy_path = r.join("legacy.db");
        drop(Store::open(legacy_path.to_str().unwrap()).unwrap());
        // Adoption rotates log_epoch even at the same path. Pin only afterward.
        Store::adopt(legacy_path.to_str().unwrap()).unwrap();
        let mut legacy = Store::open(legacy_path.to_str().unwrap()).unwrap();
        quipu::tool_knot(&mut legacy, &json!({"turtle":"<urn:sentinel> <urn:label> \"legacy retained\" .","source":"fixture"})).unwrap();
        let legacy_id = legacy.store_id().unwrap();
        let legacy_epoch = legacy.log_epoch().unwrap();
        drop(legacy);
        let catalog = json!({"version":1,"revision":7,"legacy_store_id":legacy_id,
            "budget":{"max_owners":1,"queued_per_owner":1,"readers_per_owner":1,"max_queued":1,"max_connections":3},
            "routes":[{"graph":GRAPH,"file_id":"owner","store_id":owner_id,"log_epoch":epoch,"generation":4,"policy":"fixture-policy"}],
            "legacy_participant":{"graph":quipu::schema::ROOT_GRAPH_IRI,"file_id":"legacy","store_id":legacy_id,"log_epoch":legacy_epoch,"generation":1,"policy":"legacy-policy"}});
        private(&r.join("catalog-input.json"), catalog.to_string());
        let published = cli(
            &r,
            &[
                "pilot",
                "catalog",
                "--write",
                "catalog.json",
                "--from",
                "catalog-input.json",
                "--execute",
            ],
        );
        assert_eq!(published["catalog"]["revision"], 7);
        let admitted_catalog =
            quipu::graph_catalog::Catalog::load_file(&r.join("catalog.json")).unwrap();
        // Startup calls catalog.memory_allocation(mib, readers + 2, 0).
        // One owner plus legacy = 2 * (writer + fence + one reader) = 6
        // connections, each requiring 64 MiB: minimum 384, reserve 512 MiB.
        let pilot_memory_mib = 512u64;
        let allocation = admitted_catalog
            .memory_allocation(
                pilot_memory_mib,
                admitted_catalog.budget().readers_per_owner as u64 + 2,
                0,
            )
            .unwrap();
        assert_eq!(allocation.connections, 6);
        cli(
            &r,
            &[
                "pilot",
                "fence",
                "--db",
                "legacy.db",
                "--graph",
                GRAPH,
                "--revision",
                "7",
                "--generation",
                "4",
                "--execute",
            ],
        );
        cli(&r, &["pilot", "fence", "--verify", "--db", "legacy.db"]);
        let config = format!(
            r#"[quipu.server]
auth_token = "fixture-operator-not-producer"
scoped_writer_credentials_file = "{r}/registry.json"
[quipu.ingest_pilot]
enabled = true
graph = "{GRAPH}"
root = "{r}"
inventory = "{r}/owner.inventory"
profile = "fixture"
endpoint = "{witness}"
bucket = "witness"
region = "us-east-1"
prefix = "lineage/abc"
timeout_secs = 2
installed_policy = "fixture-policy"
credentials_file = "{r}/witness.json"
catalog_file = "{r}/catalog.json"
memory_mib = {pilot_memory_mib}
witness_ca_file = "{r}/ca.der"
"#,
            r = r.display(),
            witness = tls.endpoint
        );
        fs::create_dir(r.join(".bobbin")).unwrap();
        private(&r.join(".bobbin/config.toml"), config);
        let enrolled = cli(
            &r,
            &[
                "pilot",
                "enroll",
                "--db",
                "owner.db",
                "--journal",
                "owner.journal",
                "--unit",
                "full-store",
                "--principal",
                PRINCIPAL,
                "--witness-config",
                ".bobbin/config.toml",
                "--execute",
            ],
        );
        assert_eq!(
            enrolled["enrollment"]["receipt"]["witness_key"],
            "active/00"
        );
        assert_eq!(emulator.state.lock().unwrap().objects.len(), 1);
        let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);
        let endpoint = format!("http://{address}");
        // Read the actual published catalog, not guessed producer defaults.
        let cat: Value =
            serde_json::from_slice(&fs::read(r.join("catalog.json")).unwrap()).unwrap();
        // Read-only proof after enrollment: neither participant may drift from
        // the published catalog. Never reopen the enrolled owner as a writer.
        for route in [&cat["routes"][0], &cat["legacy_participant"]] {
            let path = r.join(format!("{}.db", route["file_id"].as_str().unwrap()));
            let db = rusqlite::Connection::open_with_flags(
                path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            let actual: (String, String) = db.query_row(
                "SELECT store_id, epoch FROM store_identity, log_epoch WHERE store_identity.id=1 AND log_epoch.id=1",
                [], |row| Ok((row.get(0)?, row.get(1)?)),
            ).unwrap();
            assert_eq!(
                json!([actual.0, actual.1]),
                json!([route["store_id"], route["log_epoch"]])
            );
        }
        let route = &cat["routes"][0];
        private(&r.join("pins.json"), json!({"version":1,"endpoint":endpoint,"graph":route["graph"],
            "catalog_revision":cat["revision"],"owner_generation":route["generation"],"policy":route["policy"]}).to_string());
        let child = Command::new(env!("CARGO_BIN_EXE_quipu-server"))
            .args(["--bind", &address.to_string(), "--db"])
            .arg(&legacy_path)
            .current_dir(&r)
            .env_clear()
            .env("HOME", &r)
            .stdout(Stdio::null())
            .stderr(fs::File::create(r.join("server.log")).unwrap())
            .spawn()
            .unwrap();
        let mut lab = Self {
            root,
            endpoint,
            _emulator: emulator,
            _tls: tls,
            server: Some(Running(child)),
        };
        let until = Instant::now() + Duration::from_secs(15);
        loop {
            assert!(
                lab.server.as_mut().unwrap().0.try_wait().unwrap().is_none(),
                "server startup refused: {}",
                fs::read_to_string(lab.path("server.log")).unwrap()
            );
            if ureq::get(&format!("{}/health", lab.endpoint))
                .timeout(Duration::from_secs(1))
                .call()
                .is_ok()
            {
                break;
            }
            assert!(Instant::now() < until, "server health timeout");
            std::thread::sleep(Duration::from_millis(20));
        }
        lab
    }
    fn request(&self, route: &str, id: &str, body: &Value, bearer: bool) -> (u16, Value) {
        let mut req = ureq::post(&format!("{}{route}", self.endpoint))
            .timeout(Duration::from_secs(5))
            .set("content-type", "application/json")
            .set("x-quipu-client", "agent-adhoc")
            .set("x-quipu-completion-contract", "quipu.knot-completion/v2")
            .set("x-quipu-write-id", id)
            .set("x-quipu-catalog-revision", "7")
            .set("x-quipu-owner-generation", "4")
            .set("x-quipu-owner-policy", "fixture-policy");
        if bearer {
            req = req.set("authorization", &format!("Bearer {TOKEN}"));
        }
        let response = match req.send_string(&body.to_string()) {
            Ok(r) | Err(ureq::Error::Status(_, r)) => r,
            Err(e) => panic!("transport: {e}"),
        };
        let status = response.status();
        let text = response.into_string().unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }
}

#[test]
#[ignore = "requires an explicitly built external Yupana owner candidate; see fixture recipe"]
fn external_yupana_lands_only_in_real_enrolled_owner() {
    let candidate = PathBuf::from(
        std::env::var_os("YUPANA_OWNER_CANDIDATE").expect("YUPANA_OWNER_CANDIDATE is required"),
    );
    assert!(candidate.is_absolute() && candidate.is_file());
    let mut lab = Lab::new();
    let repo = lab.path("repo");
    fs::create_dir(&repo).unwrap();
    fs::write(
        repo.join("lib.rs"),
        "pub fn pilot_landing() -> u32 { 42 }\n",
    )
    .unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["add", "lib.rs"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "fixture",
        ],
    ] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(&repo)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("HOME", lab.root.path())
                .status()
                .unwrap()
                .success()
        );
    }
    let custody = lab.path("attempts");
    fs::create_dir(&custody).unwrap();
    checkpoint(&lab.path("legacy.db"));
    let before_hash = hash(&lab.path("legacy.db"));
    let before_count = count(&lab.path("legacy.db"));
    let before_owner = count(&lab.path("owner.db"));
    let out = Command::new(candidate)
        .arg("promote")
        .arg(&repo)
        .args([
            "--repo",
            "owner-fixture",
            "--replace-snapshot",
            "--owner",
            "--to",
            &lab.endpoint,
        ])
        .arg("--owner-pins")
        .arg(lab.path("pins.json"))
        .arg("--owner-credential-file")
        .arg(lab.path("credential"))
        .arg("--owner-custody")
        .arg(&custody)
        .env_clear()
        .env("HOME", lab.root.path())
        .env("PATH", std::env::var_os("PATH").unwrap())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "candidate failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!String::from_utf8_lossy(&out.stdout).contains(TOKEN));
    assert!(!String::from_utf8_lossy(&out.stderr).contains(TOKEN));
    let records: Vec<_> = fs::read_dir(&custody)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(records.len(), 1, "one deterministic small snapshot attempt");
    assert!(records[0].to_str().unwrap().ends_with(".finished.json"));
    let retained = fs::read(&records[0]).unwrap();
    assert!(!String::from_utf8_lossy(&retained).contains(TOKEN));
    let frozen: Value = serde_json::from_slice(&retained).unwrap();
    let binding = &frozen["binding"];
    let id = binding["write_id"].as_str().unwrap();
    let (status, proof) = lab.request("/owner/knot/status", id, binding, true);
    assert_eq!(status, 200);
    assert_eq!(proof["state"], "finished");
    assert_ne!(
        lab.request("/owner/knot/seal", id, binding, true).0,
        200,
        "finished cannot seal"
    );
    assert_eq!(
        lab.request("/owner/knot/status", id, binding, true).1["state"],
        "finished"
    );
    let denied_id = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    assert_eq!(
        lab.request("/owner/knot", denied_id, &binding["request"], false)
            .0,
        401
    );
    let denied = json!({"write_id":denied_id,"request":binding["request"]});
    assert_eq!(
        lab.request("/owner/knot/status", denied_id, &denied, true)
            .1["state"],
        "unknown"
    );
    drop(lab.server.take());
    assert!(count(&lab.path("owner.db")) > before_owner, "owner landing");
    let sql = rusqlite::Connection::open_with_flags(
        lab.path("owner.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let landed: i64 = sql
        .query_row(
            "SELECT count(*) FROM facts WHERE g=(SELECT id FROM terms WHERE iri=?1) AND op=1",
            [GRAPH],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        landed > 0,
        "projection must land in the selected named graph"
    );
    let authenticated: i64 = sql.query_row("SELECT count(*) FROM transaction_auth WHERE principal=?1 AND credential_id=?2 AND auth_class='scoped_named_bearer'", [PRINCIPAL, CREDENTIAL], |r| r.get(0)).unwrap();
    assert!(
        authenticated > 0,
        "landing must carry scoped credential evidence"
    );
    drop(sql);
    checkpoint(&lab.path("legacy.db"));
    assert_eq!(count(&lab.path("legacy.db")), before_count);
    assert_eq!(
        hash(&lab.path("legacy.db")),
        before_hash,
        "legacy bytes unchanged"
    );
    assert_eq!(lab._emulator.state.lock().unwrap().unconditional_puts, 0);
}
