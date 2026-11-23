//! Temporary verified TLS bridge for server startup tests.
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
pub(super) struct TlsBridge {
    pub(super) endpoint: String,
    pub(super) der: Vec<u8>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
    _files: tempfile::TempDir,
}

impl TlsBridge {
    pub(super) fn start(upstream: &str) -> Self {
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
