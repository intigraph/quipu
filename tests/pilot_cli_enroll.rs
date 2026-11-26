//! `quipu pilot enroll` through the binary (aegis-4qv.2). The witness is a
//! minimal create-only S3 stand-in: `PUT` requires `If-None-Match: *` and
//! answers 412 for an existing key, so an overwrite is impossible. Every
//! refusal must leave the database byte-identical and the witness unchanged.
#![cfg(all(unix, feature = "ownership-witness-s3"))]

use quipu::Store;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

type Objects = Arc<Mutex<BTreeMap<String, Vec<u8>>>>;

/// Published AWS documentation example credentials (not a secret).
const CREDENTIALS: &str = r#"{"access_key_id":"AKIAIOSFODNN7EXAMPLE","secret_access_key":"wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"}"#;

fn respond(stream: &mut TcpStream, status: &str, headers: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status}\r\nconnection: close\r\ncontent-length: {}\r\n{headers}\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

fn serve(mut stream: TcpStream, objects: &Objects) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return;
    };
    let (method, target) = (method.to_owned(), target.to_owned());
    let mut headers = BTreeMap::new();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).is_err() || h == "\r\n" || h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_owned());
        }
    }
    let length: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0; length];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    let path = target.split('?').next().unwrap_or_default();
    match (method.as_str(), path.strip_prefix("/witness/")) {
        ("PUT", Some(key)) => {
            if headers.get("if-none-match").map(String::as_str) != Some("*") {
                return respond(&mut stream, "400 Bad Request", "", b"");
            }
            let mut objects = objects.lock().unwrap();
            if objects.contains_key(key) {
                return respond(&mut stream, "412 Precondition Failed", "", b"");
            }
            objects.insert(key.to_owned(), body);
            respond(&mut stream, "200 OK", "etag: \"v1\"\r\n", b"");
        }
        ("GET", Some(key)) => match objects.lock().unwrap().get(key).cloned() {
            Some(bytes) => respond(&mut stream, "200 OK", "etag: \"v1\"\r\n", &bytes),
            None => respond(&mut stream, "404 Not Found", "", b""),
        },
        ("GET", None) if path == "/witness" => {
            let mut xml = String::from(
                "<?xml version=\"1.0\"?><ListBucketResult><IsTruncated>false</IsTruncated>",
            );
            for key in objects.lock().unwrap().keys() {
                xml.push_str(&format!("<Contents><Key>{key}</Key></Contents>"));
            }
            xml.push_str("</ListBucketResult>");
            respond(&mut stream, "200 OK", "", xml.as_bytes());
        }
        _ => respond(&mut stream, "405 Method Not Allowed", "", b""),
    }
}

/// Start the stand-in; returns its `http://` origin and its object map.
fn witness() -> (String, Objects) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let objects: Objects = Arc::default();
    let shared = objects.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let objects = shared.clone();
            std::thread::spawn(move || serve(stream, &objects));
        }
    });
    (endpoint, objects)
}

struct Lab {
    dir: tempfile::TempDir,
    objects: Objects,
}

impl Lab {
    /// An owner database, owner-only credentials and a pilot config naming
    /// `endpoint` (a live stand-in unless `endpoint` is given explicitly).
    fn new(endpoint: Option<&str>) -> Self {
        let (live, objects) = witness();
        let endpoint = endpoint.unwrap_or(&live);
        let dir = tempfile::tempdir().unwrap();
        drop(Store::open(dir.path().join("owner.db").to_str().unwrap()).unwrap());
        let creds = dir.path().join("credentials.json");
        std::fs::write(&creds, CREDENTIALS).unwrap();
        std::fs::set_permissions(&creds, std::fs::Permissions::from_mode(0o600)).unwrap();
        let config = format!(
            r#"[quipu.ingest_pilot]
enabled = true
graph = "urn:g"
root = "/r"
inventory = "/r/i"
profile = "pilot"
endpoint = "{endpoint}"
bucket = "witness"
region = "us-east-1"
prefix = "lineage/abc"
timeout_secs = 2
installed_policy = "pilot-policy"
credentials_file = "{}"
catalog_file = "/r/c"
memory_mib = 64
"#,
            creds.display()
        );
        let path = dir.path().join("witness.toml");
        std::fs::write(&path, config).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        Self { dir, objects }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn chmod(&self, name: &str, mode: u32) {
        std::fs::set_permissions(self.path(name), std::fs::Permissions::from_mode(mode)).unwrap();
    }

    fn enroll(&self, extra: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_quipu"))
            .args(["pilot", "enroll", "--db", "owner.db", "--journal"])
            .args(["owner.journal", "--unit", "full-store", "--principal"])
            .args(["pilot-owner", "--witness-config", "witness.toml"])
            .args(extra)
            .current_dir(self.dir.path())
            .env_remove("QUIPU_SERVER")
            .output()
            .unwrap()
    }

    fn ok(&self, extra: &[&str]) -> Value {
        let out = self.enroll(extra);
        assert!(
            out.status.success(),
            "enroll {extra:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }

    fn keys(&self) -> Vec<String> {
        self.objects.lock().unwrap().keys().cloned().collect()
    }

    /// The verb must refuse naming `why`, leave the database and inventory
    /// byte-identical, and leave the witness listing unchanged.
    fn refuses(&self, extra: &[&str], why: &str) {
        let (db, inventory) = (hash(&self.path("owner.db")), self.path("owner.inventory"));
        let (inv, keys) = (hash(&inventory), self.keys());
        let out = self.enroll(extra);
        assert!(!out.status.success(), "enroll {extra:?} should refuse");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(why), "refusal names `{why}`: {stderr}");
        assert!(!stderr.contains("EXAMPLEKEY"), "no secret in errors");
        assert_eq!(hash(&self.path("owner.db")), db, "database untouched");
        assert_eq!(hash(&inventory), inv, "inventory untouched");
        assert_eq!(self.keys(), keys, "witness untouched");
    }
}

fn hash(path: &Path) -> String {
    std::fs::read(path).map_or_else(|_| "absent".into(), |b| hex::encode(Sha256::digest(b)))
}

#[test]
fn dry_run_then_execute_then_reenroll_refuses() {
    let lab = Lab::new(None);
    let plan = lab.ok(&[]);
    assert_eq!(plan["mode"], "dry-run");
    assert_eq!(plan["enrollment"]["mode"], "dry-run");
    assert!(plan["enrollment"]["plan"]["authority"]["store_id"].is_string());
    assert!(lab.keys().is_empty(), "dry run writes no witness object");
    assert!(!lab.path("owner.journal").exists() && !lab.path("owner.inventory").exists());
    assert_eq!(plan["before"]["database"], plan["after"]["database"]);

    let done = lab.ok(&["--execute"]);
    assert_eq!(done["mode"], "execute");
    let receipt = &done["enrollment"]["receipt"];
    assert_eq!(receipt["witness_key"], "active/00");
    assert_eq!(
        receipt, &plan["enrollment"]["plan"]["receipt"],
        "the executed receipt is the planned one"
    );
    assert_eq!(lab.keys(), ["lineage/abc/active/00"]);
    let inventory = std::fs::metadata(lab.path("owner.inventory")).unwrap();
    assert_eq!(inventory.permissions().mode() & 0o777, 0o600);
    assert!(!lab.path("owner.enroll-intent").exists(), "intent retired");
    assert_eq!(done["after"]["inventory"]["exists"], true);

    lab.refuses(&["--execute"], "enrollment refused");
    lab.refuses(&[], "enrollment refused");
}

#[test]
fn each_guard_refuses_and_leaves_everything_unchanged() {
    let lab = Lab::new(None);
    lab.refuses(
        &["--secret-access-key", "x", "--execute"],
        "never accepted on argv",
    );
    lab.refuses(&["--bogus", "--execute"], "unknown argument");
    lab.chmod("witness.toml", 0o664);
    lab.refuses(&["--execute"], "writable by group or other");
    lab.chmod("witness.toml", 0o600);
    lab.chmod("credentials.json", 0o640);
    lab.refuses(&["--execute"], "unsafe permissions");
    lab.chmod("credentials.json", 0o600);
    std::fs::write(lab.path("owner.inventory"), b"stale").unwrap();
    lab.refuses(&["--execute"], "enrollment refused");
    std::fs::remove_file(lab.path("owner.inventory")).unwrap();
    lab.objects
        .lock()
        .unwrap()
        .insert("lineage/abc/active/00".into(), b"someone else".to_vec());
    lab.refuses(&["--execute"], "enrollment refused");
    assert_eq!(lab.keys(), ["lineage/abc/active/00"], "never deleted");
}

#[test]
fn unreachable_witness_refuses_the_dry_run() {
    let lab = Lab::new(Some("http://127.0.0.1:1"));
    lab.refuses(&[], "witness");
    lab.refuses(&["--execute"], "witness");
    assert!(!lab.path("owner.journal").exists());
}

#[test]
fn missing_principal_or_unknown_unit_refuses() {
    let lab = Lab::new(None);
    let out = Command::new(env!("CARGO_BIN_EXE_quipu"))
        .args(["pilot", "enroll", "--db", "owner.db", "--journal", "j"])
        .args(["--unit", "full-store", "--witness-config", "witness.toml"])
        .current_dir(lab.dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--principal is required"));
    let out = Command::new(env!("CARGO_BIN_EXE_quipu"))
        .args(["pilot", "enroll", "--db", "owner.db", "--journal", "j"])
        .args(["--unit", "graph", "--principal", "p"])
        .args(["--witness-config", "witness.toml", "--execute"])
        .current_dir(lab.dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown authority unit"));
    assert!(lab.keys().is_empty());
}
