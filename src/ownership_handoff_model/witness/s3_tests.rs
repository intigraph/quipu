use super::super::conformance::{self, Harness};
use super::super::{AuthorityWitness, Fault, ObjectWitness, Reply};
use super::*;
use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

// ---- independent known-answer vectors ----

#[test]
fn hmac_matches_rfc4231() {
    // Test case 1.
    assert_eq!(
        hex::encode(hmac_sha256(&[0x0b; 20], b"Hi There")),
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
    );
    // Test case 2.
    assert_eq!(
        hex::encode(hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    // Test case 6: key longer than the block size.
    assert_eq!(
        hex::encode(hmac_sha256(
            &[0xaa; 131],
            b"Test Using Larger Than Block-Size Key - Hash Key First"
        )),
        "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
    );
}

#[test]
fn stamps_are_civil_utc() {
    let s = Stamp::at(1_369_353_600);
    assert_eq!(
        (s.short.as_str(), s.long.as_str()),
        ("20130524", "20130524T000000Z")
    );
    assert_eq!(Stamp::at(0).long, "19700101T000000Z");
    assert_eq!(Stamp::at(951_782_400 + 3_723).long, "20000229T010203Z");
    assert_eq!(Stamp::at(1_793_923_199).long, "20261105T235959Z");
    assert_eq!(Stamp::at(1_709_251_199).long, "20240229T235959Z");
}

fn aws_example() -> Credentials {
    Credentials::new(
        "AKIAIOSFODNN7EXAMPLE".into(),
        "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(),
    )
    .unwrap()
}

const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

#[test]
fn sigv4_matches_published_get_object_example() {
    // AWS S3 SigV4 documentation, "GET Object" example.
    let headers = vec![
        (
            "host".to_owned(),
            "examplebucket.s3.amazonaws.com".to_owned(),
        ),
        ("range".to_owned(), "bytes=0-9".to_owned()),
        ("x-amz-content-sha256".to_owned(), EMPTY.to_owned()),
        ("x-amz-date".to_owned(), "20130524T000000Z".to_owned()),
    ];
    let auth = authorization(
        &aws_example(),
        "us-east-1",
        &Stamp::at(1_369_353_600),
        "GET",
        "/test.txt",
        &[],
        &headers,
        EMPTY,
    );
    assert!(
        auth.ends_with(
            "Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        ),
        "{auth}"
    );
    assert!(auth.contains("SignedHeaders=host;range;x-amz-content-sha256;x-amz-date"));
}

#[test]
fn sigv4_matches_published_list_objects_example() {
    // AWS S3 SigV4 documentation, "GET Bucket (List Objects)" example.
    let headers = vec![
        (
            "host".to_owned(),
            "examplebucket.s3.amazonaws.com".to_owned(),
        ),
        ("x-amz-content-sha256".to_owned(), EMPTY.to_owned()),
        ("x-amz-date".to_owned(), "20130524T000000Z".to_owned()),
    ];
    let auth = authorization(
        &aws_example(),
        "us-east-1",
        &Stamp::at(1_369_353_600),
        "GET",
        "/",
        &[("max-keys", "2".into()), ("prefix", "J".into())],
        &headers,
        EMPTY,
    );
    assert!(
        auth.ends_with(
            "Signature=34b48302e7b5fa45bde8084f4b7868a86f0a534bc59db6670ed5711ef69dc6f7"
        ),
        "{auth}"
    );
}

#[test]
fn credentials_and_locations_refuse_unsafe_input() {
    assert_eq!(format!("{:?}", aws_example()), "Credentials(<redacted>)");
    assert!(Credentials::new(String::new(), "s".into()).is_none());
    assert!(Credentials::new("id".into(), "has space".into()).is_none());
    assert!(Location::new("https://s3.example", "witness", "us-east-1", "lineage/abc").is_some());
    for (endpoint, bucket, region, prefix) in [
        ("https://s3.example/path", "witness", "us-east-1", "p"),
        ("ftp://s3.example", "witness", "us-east-1", "p"),
        ("https://s3.example", "Witness", "us-east-1", "p"),
        ("https://s3.example", "witness", "us east", "p"),
        ("https://s3.example", "witness", "us-east-1", "/p"),
        ("https://s3.example", "witness", "us-east-1", "p/"),
        ("https://s3.example", "witness", "us-east-1", "a//b"),
        ("https://s3.example", "witness", "us-east-1", "a/../b"),
        ("https://s3.example", "witness", "us-east-1", ""),
    ] {
        assert!(
            Location::new(endpoint, bucket, region, prefix).is_none(),
            "{endpoint} {bucket} {region} {prefix}"
        );
    }
}

#[test]
fn listing_scan_is_strict() {
    let ok = "<ListBucketResult><IsTruncated>false</IsTruncated><KeyCount>2</KeyCount>\
              <Contents><Key>p/active/00</Key></Contents><Contents><Key>p/reserved/00/00</Key></Contents>\
              </ListBucketResult>";
    assert_eq!(
        list_keys(ok, "p/"),
        Some(vec!["active/00".to_owned(), "reserved/00/00".to_owned()])
    );
    for bad in [
        ok.replace("false", "true"),
        ok.replace("<IsTruncated>false</IsTruncated>", ""),
        ok.replace("<KeyCount>2</KeyCount>", "<KeyCount>3</KeyCount>"),
        ok.replace("p/active/00", "q/active/00"),
        ok.replace("p/active/00", "p/active/00&amp;"),
        ok.replace("ListBucketResult", "Error"),
    ] {
        assert_eq!(list_keys(&bad, "p/"), None, "{bad}");
    }
}

// ---- private conditional-create emulator ----

#[derive(Clone, Copy, Debug)]
enum EmuFault {
    Status500,
    Slow,
    LostReplyOnPut,
    NoVersion,
    Oversize,
    TruncatedList,
}

#[derive(Default)]
struct EmuState {
    objects: BTreeMap<String, (Vec<u8>, u64)>,
    faults: VecDeque<EmuFault>,
    next_version: u64,
    extra_listed: Option<String>,
    unconditional_puts: usize,
}

struct Emulator {
    endpoint: String,
    state: Arc<Mutex<EmuState>>,
}

const BUCKET: &str = "witness";
const PREFIX: &str = "lineage/abc";
const TIMEOUT: Duration = Duration::from_millis(400);

impl Emulator {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(EmuState::default()));
        let shared = state.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let state = shared.clone();
                std::thread::spawn(move || serve(stream, &state));
            }
        });
        Self { endpoint, state }
    }
    fn backend(&self) -> S3Objects {
        S3Objects::new(
            Location::new(&self.endpoint, BUCKET, "us-east-1", PREFIX).unwrap(),
            aws_example(),
            TIMEOUT,
        )
    }
    fn push(&self, fault: EmuFault) {
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

// ---- conformance over the emulator ----

struct S3Harness {
    emulators: Mutex<Vec<Arc<Emulator>>>,
}

impl Harness for S3Harness {
    type Backend = S3Objects;
    fn fresh(&self) -> ObjectWitness<S3Objects> {
        let emulator = Arc::new(Emulator::start());
        let backend = emulator.backend();
        assert_eq!(
            backend.create("active/00", &conformance::genesis_bytes()),
            Reply::Ok(Created::New),
            "assumed genesis (private fixture, not enrollment)"
        );
        self.emulators.lock().unwrap().push(emulator);
        ObjectWitness::attach(conformance::genesis().store_id, backend)
    }
    fn inject(&self, witness: &ObjectWitness<S3Objects>, fault: Fault) {
        let emulators = self.emulators.lock().unwrap();
        let emulator = emulators
            .iter()
            .find(|e| e.endpoint == witness.backend.location.endpoint)
            .unwrap();
        emulator.push(match fault {
            Fault::Unknown => EmuFault::Status500,
            Fault::Slow => EmuFault::Slow,
            Fault::LostReply => EmuFault::LostReplyOnPut,
            Fault::Conflict => panic!("conflict has no transport analog; covered by contenders"),
        });
    }
}

#[test]
fn s3_backend_passes_the_shared_conformance_suite() {
    conformance::run(&S3Harness {
        emulators: Mutex::new(Vec::new()),
    });
}

#[test]
fn transport_doubt_is_unknown_never_absence() {
    let e = Emulator::start();
    let b = e.backend();
    assert_eq!(b.create("active/00", b"{}"), Reply::Ok(Created::New));
    assert_eq!(b.create("active/00", b"{}"), Reply::Ok(Created::Exists));
    let got = b.get("active/00");
    assert!(
        matches!(&got, Reply::Ok(Some(o)) if o.bytes == b"{}" && o.version == "\"v1\""),
        "{got:?}"
    );
    assert_eq!(b.get("active/01"), Reply::Ok(None));
    e.push(EmuFault::NoVersion);
    assert_eq!(b.get("active/00"), Reply::Unknown, "missing version id");
    e.push(EmuFault::Oversize);
    assert_eq!(b.get("active/00"), Reply::Unknown, "oversized body");
    e.push(EmuFault::Status500);
    assert_eq!(
        b.get("active/00"),
        Reply::Unknown,
        "server error is not absence"
    );
    e.push(EmuFault::Slow);
    assert_eq!(b.get("active/01"), Reply::Unknown, "timeout is not absence");
    e.push(EmuFault::TruncatedList);
    assert_eq!(b.list(10), Reply::Unknown, "truncated listing");
    assert_eq!(b.list(10), Reply::Ok(vec!["active/00".to_owned()]));
    assert_eq!(b.list(0), Reply::Unknown, "over limit");
    let dead = S3Objects::new(
        Location::new("http://127.0.0.1:9", BUCKET, "us-east-1", PREFIX).unwrap(),
        aws_example(),
        TIMEOUT,
    );
    assert_eq!(dead.get("active/00"), Reply::Unknown, "unreachable witness");
    assert_eq!(dead.create("active/00", b"{}"), Reply::Unknown);
}

#[test]
fn emulator_enforces_conditional_create_and_signing() {
    let e = Emulator::start();
    // A client that forgot If-None-Match is refused, so dropping it from
    // S3Objects::create fails the conformance suite rather than overwriting.
    let raw = ureq::put(&format!("{}/{BUCKET}/{PREFIX}/active/00", e.endpoint))
        .set(
            "authorization",
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/x",
        )
        .set("x-amz-date", "20260101T000000Z")
        .set("x-amz-content-sha256", &hex::encode(Sha256::digest(b"{}")))
        .send_bytes(b"{}");
    assert!(matches!(raw, Err(ureq::Error::Status(400, _))));
    assert_eq!(e.state.lock().unwrap().unconditional_puts, 1);
    let unsigned = ureq::get(&format!("{}/{BUCKET}/{PREFIX}/active/00", e.endpoint)).call();
    assert!(matches!(unsigned, Err(ureq::Error::Status(403, _))));
}

#[test]
fn foreign_listed_name_makes_the_head_unknown() {
    let h = S3Harness {
        emulators: Mutex::new(Vec::new()),
    };
    let w = h.fresh();
    let id = conformance::genesis().store_id;
    assert!(matches!(w.head(&id, 0), Reply::Ok(_)));
    h.emulators.lock().unwrap()[0]
        .state
        .lock()
        .unwrap()
        .extra_listed = Some("active/00.bak".into());
    assert_eq!(w.head(&id, 0), Reply::Unknown);
}
