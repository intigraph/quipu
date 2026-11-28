//! Private conditional-create S3 emulator for witness tests (design W2).
//! It refuses unconditional PUT and unsigned or payload-mismatched requests.
#![allow(dead_code)]
const MAX_BODY: u64 = 65536;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
pub(crate) enum EmuFault {
    Status500,
    Slow,
    LostReplyOnPut,
    NoVersion,
    Oversize,
    TruncatedList,
}

#[derive(Default)]
pub(crate) struct EmuState {
    pub(crate) objects: BTreeMap<String, (Vec<u8>, u64)>,
    pub(crate) faults: VecDeque<EmuFault>,
    pub(crate) next_version: u64,
    pub(crate) extra_listed: Option<String>,
    pub(crate) unconditional_puts: usize,
}

pub(crate) struct Emulator {
    pub(crate) endpoint: String,
    pub(crate) state: Arc<Mutex<EmuState>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

pub(crate) const BUCKET: &str = "witness";
pub(crate) const PREFIX: &str = "lineage/abc";
pub(crate) const TIMEOUT: Duration = Duration::from_millis(400);

impl Emulator {
    pub(crate) fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(EmuState::default()));
        let shared = state.clone();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let done = stop.clone();
        let worker = std::thread::spawn(move || {
            while !done.load(std::sync::atomic::Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        // accept() inherits O_NONBLOCK on macOS/BSD; timeout IO must block.
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        stream
                            .set_write_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        serve(stream, &shared);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            endpoint,
            state,
            stop,
            worker: Some(worker),
        }
    }

    pub(crate) fn push(&self, fault: EmuFault) {
        self.state.lock().unwrap().faults.push_back(fault);
    }
}

fn take_fault(state: &Mutex<EmuState>, method: &str) -> Option<EmuFault> {
    let mut s = state.lock().unwrap();
    match s.faults.front().copied() {
        // A lost reply is armed for the next create only.
        Some(EmuFault::LostReplyOnPut) if method != "PUT" => None,
        _ => s.faults.pop_front(),
    }
}

fn respond(stream: &mut TcpStream, status: &str, headers: &[(&str, String)], body: &[u8]) {
    eprintln!(
        "witness emulator response: {status}, body_bytes={}",
        body.len()
    );
    let mut out = format!(
        "HTTP/1.1 {status}\r\nconnection: close\r\ncontent-length: {}\r\n",
        body.len()
    );
    for (k, v) in headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    let _ = stream.write_all(out.as_bytes());
    let _ = stream.write_all(body);
}

fn serve(mut stream: TcpStream, state: &Mutex<EmuState>) {
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
    if std::io::Read::read_exact(&mut reader, &mut body).is_err() {
        return;
    }
    // Every request must be signed over its actual payload.
    let signed = headers
        .get("authorization")
        .is_some_and(|a| a.starts_with("AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/"))
        && headers.get("x-amz-content-sha256") == Some(&hex::encode(Sha256::digest(&body)))
        && headers.contains_key("x-amz-date");
    eprintln!(
        "witness emulator request: method={method}, target={target}, signed_shape_and_payload={signed}"
    );
    if !signed {
        return respond(&mut stream, "403 Forbidden", &[], b"");
    }
    let fault = take_fault(state, &method);
    match fault {
        Some(EmuFault::Status500) => {
            return respond(&mut stream, "500 Internal Server Error", &[], b"");
        }
        Some(EmuFault::Slow) => std::thread::sleep(TIMEOUT * 3),
        _ => {}
    }
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let object_prefix = format!("/{BUCKET}/");
    match (method.as_str(), path) {
        ("PUT", p) if p.starts_with(&object_prefix) => {
            let key = p[object_prefix.len()..].to_owned();
            if headers.get("if-none-match").map(String::as_str) != Some("*") {
                state.lock().unwrap().unconditional_puts += 1;
                return respond(
                    &mut stream,
                    "400 Bad Request",
                    &[],
                    b"conditional create required",
                );
            }
            let mut s = state.lock().unwrap();
            if s.objects.contains_key(&key) {
                drop(s);
                return respond(&mut stream, "412 Precondition Failed", &[], b"");
            }
            s.next_version += 1;
            let version = s.next_version;
            s.objects.insert(key, (body, version));
            drop(s);
            if matches!(fault, Some(EmuFault::LostReplyOnPut)) {
                return; // committed, reply lost
            }
            respond(
                &mut stream,
                "200 OK",
                &[("etag", format!("\"v{version}\""))],
                b"",
            );
        }
        ("GET", p) if p.starts_with(&object_prefix) => {
            let key = &p[object_prefix.len()..];
            let found = state.lock().unwrap().objects.get(key).cloned();
            match found {
                None => respond(&mut stream, "404 Not Found", &[], b""),
                Some((bytes, version)) => {
                    let bytes = if matches!(fault, Some(EmuFault::Oversize)) {
                        vec![b' '; MAX_BODY as usize + 1]
                    } else {
                        bytes
                    };
                    let headers = if matches!(fault, Some(EmuFault::NoVersion)) {
                        vec![]
                    } else {
                        vec![("etag", format!("\"v{version}\""))]
                    };
                    respond(&mut stream, "200 OK", &headers, &bytes);
                }
            }
        }
        ("GET", p) if p == format!("/{BUCKET}") => {
            let params: BTreeMap<&str, &str> = query
                .split('&')
                .filter_map(|kv| kv.split_once('='))
                .collect();
            let prefix = params.get("prefix").unwrap_or(&"").replace("%2F", "/");
            let max: usize = params
                .get("max-keys")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1000);
            let s = state.lock().unwrap();
            let mut keys: Vec<String> = s
                .objects
                .keys()
                .filter(|k| k.starts_with(&prefix))
                .cloned()
                .collect();
            if let Some(extra) = &s.extra_listed {
                keys.push(format!("{prefix}{extra}"));
            }
            drop(s);
            let truncated = keys.len() > max || matches!(fault, Some(EmuFault::TruncatedList));
            keys.truncate(max);
            let mut xml = format!(
                "<?xml version=\"1.0\"?><ListBucketResult><IsTruncated>{truncated}</IsTruncated><KeyCount>{}</KeyCount>",
                keys.len()
            );
            for k in &keys {
                xml.push_str(&format!("<Contents><Key>{k}</Key></Contents>"));
            }
            xml.push_str("</ListBucketResult>");
            respond(&mut stream, "200 OK", &[], xml.as_bytes());
        }
        _ => respond(&mut stream, "405 Method Not Allowed", &[], b""),
    }
}

impl Drop for Emulator {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        self.worker.take().unwrap().join().unwrap();
    }
}
