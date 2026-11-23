//! Real verified TLS to the conditional-object emulator; no verifier bypass.
use super::*;
use crate::ownership_handoff_model::witness::objects::{Created, Objects};
use crate::ownership_handoff_model::witness::s3_emulator::{BUCKET, Emulator, PREFIX};
use crate::ownership_handoff_model::witness::{Reply, assumed_genesis_bytes};
use crate::ownership_handoff_model::{Authority, AuthorityUnit, Journal, Placement};
use crate::store::{ownership_admission, ownership_inventory, ownership_journal};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

struct TlsBridge {
    endpoint: String,
    der: Vec<u8>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
    _files: tempfile::TempDir,
}

impl TlsBridge {
    fn start(upstream: &str) -> Self {
        let files = tempfile::tempdir().unwrap();
        let cert = files.path().join("cert.pem");
        let key = files.path().join("key.pem");
        let cert_der = files.path().join("cert.der");
        let key_der = files.path().join("key.der");
        let run = |args: &[&str]| {
            assert!(
                Command::new("openssl")
                    .args(args)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .expect("openssl TLS fixture")
                    .success()
            );
        };
        run(&[
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "1",
            "-subj",
            "/CN=localhost",
            "-addext",
            "basicConstraints=critical,CA:FALSE",
            "-addext",
            "subjectAltName=IP:127.0.0.1",
            "-keyout",
            key.to_str().unwrap(),
            "-out",
            cert.to_str().unwrap(),
        ]);
        run(&[
            "x509",
            "-in",
            cert.to_str().unwrap(),
            "-outform",
            "DER",
            "-out",
            cert_der.to_str().unwrap(),
        ]);
        run(&[
            "pkcs8",
            "-topk8",
            "-nocrypt",
            "-in",
            key.to_str().unwrap(),
            "-outform",
            "DER",
            "-out",
            key_der.to_str().unwrap(),
        ]);
        let der = fs::read(cert_der).unwrap();
        let config = ureq::rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![der.clone().into()],
                ureq::rustls::pki_types::PrivateKeyDer::Pkcs8(fs::read(key_der).unwrap().into()),
            )
            .unwrap();
        let config = Arc::new(config);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("https://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let upstream = upstream.strip_prefix("http://").unwrap().to_owned();
        let worker = std::thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                let (stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(_) => break,
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut tls = ureq::rustls::StreamOwned::new(
                    ureq::rustls::ServerConnection::new(config.clone()).unwrap(),
                    stream,
                );
                // Fixture forwards one bounded request; the emulator closes replies.
                let mut request = Vec::new();
                let mut byte = [0];
                while request.len() < 65536 && !request.ends_with(b"\r\n\r\n") {
                    if tls.read_exact(&mut byte).is_err() {
                        break;
                    }
                    request.push(byte[0]);
                }
                if !request.ends_with(b"\r\n\r\n") {
                    continue;
                }
                let length = String::from_utf8_lossy(&request)
                    .lines()
                    .find_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        k.eq_ignore_ascii_case("content-length")
                            .then(|| v.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                if length > 65536 {
                    continue;
                }
                let mut body = vec![0; length];
                if tls.read_exact(&mut body).is_err() {
                    continue;
                }
                let mut upstream = TcpStream::connect(&upstream).unwrap();
                upstream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                upstream.write_all(&request).unwrap();
                upstream.write_all(&body).unwrap();
                let mut reply = Vec::new();
                if upstream.take(1 << 20).read_to_end(&mut reply).is_ok() {
                    let _ = tls.write_all(&reply);
                    let _ = tls.flush();
                }
            }
        });
        Self {
            endpoint,
            der,
            stop,
            worker: Some(worker),
            _files: files,
        }
    }
}
impl Drop for TlsBridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn enroll(f: &Fixture, c: &EnrolledPilotConfig, emulator: &Emulator) {
    let authority = Authority {
        store_id: f.route.store_id.clone(),
        unit: AuthorityUnit::FullStore,
        generation: 0,
        epoch: f.route.log_epoch.clone(),
        owner: Placement {
            principal: "fixture-owner".into(),
            native: [1; 32],
        },
        policy: f.route.policy.clone(),
        predecessor: None,
    };
    let journal = f.root.join("owner.journal");
    ownership_journal::create(
        &journal,
        &Journal::Active {
            authority: authority.clone(),
        },
    )
    .unwrap();
    fs::set_permissions(&journal, fs::Permissions::from_mode(0o600)).unwrap();
    let db = f.root.join("ingest.db");
    let meta = fs::metadata(&db).unwrap();
    let entry = ownership_inventory::Entry {
        store_id: f.route.store_id.clone(),
        unit: AuthorityUnit::FullStore,
        database: ownership_inventory::Database {
            path: db.to_str().unwrap().into(),
            device: meta.dev(),
            inode: meta.ino(),
        },
        journal: journal.to_str().unwrap().into(),
        witness: c.profile.clone(),
        enrolled_at_generation: 0,
    };
    let binding = ownership_inventory::Binding {
        version: 1,
        store_id: f.route.store_id.clone(),
        inventory_digest: entry.digest().unwrap(),
    };
    fs::write(
        &c.inventory,
        ownership_inventory::Inventory {
            version: 1,
            records: vec![entry],
        }
        .encode()
        .unwrap(),
    )
    .unwrap();
    fs::set_permissions(&c.inventory, fs::Permissions::from_mode(0o600)).unwrap();
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.execute_batch(ownership_admission::MARKER_SQL).unwrap();
    conn.execute(
        "INSERT INTO ownership_handoff_enrollment(id,binding) VALUES(1,?1)",
        [serde_json::to_string(&binding).unwrap()],
    )
    .unwrap();
    assert_eq!(
        emulator
            .backend()
            .create("active/00", &assumed_genesis_bytes(&authority)),
        Reply::Ok(Created::New)
    );
}

#[test]
fn enrolled_factory_admits_over_verified_tls_and_refuses_untrusted_tls() {
    let f = fixture();
    f.legacy.fence_routed_graph(GRAPH, 7, 4).unwrap();
    let emulator = Emulator::start();
    let tls = TlsBridge::start(&emulator.endpoint);
    let mut c = config(&f);
    c.endpoint = tls.endpoint.clone();
    c.bucket = BUCKET.into();
    c.region = "us-east-1".into();
    c.prefix = PREFIX.into();
    enroll(&f, &c, &emulator);
    let credentials = || {
        Ok((
            "AKIAIOSFODNN7EXAMPLE".into(),
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(),
        ))
    };
    assert!(
        enabled()
            .admit_enrolled(
                catalog_for(&f.route),
                &f.legacy,
                &c,
                credentials,
                |_| Ok(())
            )
            .is_err()
    );
    c.witness_ca_der = Some(tls.der.clone());
    let router = enabled()
        .admit_enrolled(
            catalog_for(&f.route),
            &f.legacy,
            &c,
            credentials,
            |_| Ok(()),
        )
        .expect("verified TLS S1 factory")
        .unwrap();
    assert_eq!(router.single_routed_graph(), Some(GRAPH));
    let mut request = request(Some(GRAPH));
    request.catalog_revision = router.catalog_revision();
    assert!(
        router
            .routed_knot(&request, &input(Some(GRAPH), "tls"))
            .unwrap()
            .is_some()
    );
    assert_eq!(
        f.legacy
            .conn
            .query_row("SELECT COUNT(*) FROM facts", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        0
    );
}
