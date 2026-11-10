//! W2: S3-compatible conditional-create witness backend.
//!
//! Create is `PUT` with `If-None-Match: *`: 2xx means this call created the
//! key, 412 means it already existed. Every other outcome (409 conditional
//! conflict, 5xx, timeout, transport error, oversized or unreadable body,
//! truncated listing) is UNKNOWN, and the witness resolves it by reading the
//! exact key back on a later call.
//!
//! Requests are signed with AWS Signature Version 4. Responses are NOT
//! signed (design D2); the stated limitation is the privileged-operator
//! exclusion. Selecting a real bucket and principal is a deployment decision
//! outside this slice. Credentials never appear in errors, Debug output or
//! version strings.
use super::Reply;
use super::objects::{Created, Object, Objects};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::time::Duration;

/// A witness record is at most this large; anything longer is UNKNOWN.
const MAX_BODY: u64 = crate::ownership_handoff_model::MAX_RECORD_BYTES as u64;
/// Listing bodies are bounded independently of record bodies.
const MAX_LIST_BODY: u64 = 1 << 20;

/// Static request credentials. Debug never prints the secret or key id.
#[derive(Clone)]
pub(crate) struct Credentials {
    access_key_id: String,
    secret: String,
}

impl Credentials {
    pub(crate) fn new(access_key_id: String, secret: String) -> Option<Self> {
        let ok =
            |s: &str| !s.is_empty() && s.len() <= 256 && s.bytes().all(|b| b.is_ascii_graphic());
        (ok(&access_key_id) && ok(&secret)).then_some(Self {
            access_key_id,
            secret,
        })
    }
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Credentials(<redacted>)")
    }
}

/// Where one lineage's witness records live. Path-style addressing.
#[derive(Clone, Debug)]
pub(crate) struct Location {
    endpoint: String,
    host: String,
    bucket: String,
    region: String,
    prefix: String,
}

impl Location {
    /// `endpoint` is `scheme://host[:port]` with no path. Bucket, region and
    /// prefix are restricted to characters that need no URI encoding, so the
    /// signed canonical URI is the literal path.
    pub(crate) fn new(endpoint: &str, bucket: &str, region: &str, prefix: &str) -> Option<Self> {
        let (scheme, host) = endpoint.split_once("://")?;
        if !matches!(scheme, "https" | "http")
            || host.is_empty()
            || !host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':'))
        {
            return None;
        }
        let safe = |s: &str, extra: &[u8]| {
            !s.is_empty()
                && s.len() <= 200
                && s.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || extra.contains(&b))
        };
        if !safe(bucket, b".-")
            || !safe(region, b"-")
            || !safe(prefix, b"-/")
            || prefix.starts_with('/')
            || prefix.ends_with('/')
            || prefix.contains("//")
        {
            return None;
        }
        Some(Self {
            endpoint: endpoint.to_owned(),
            host: host.to_owned(),
            bucket: bucket.to_owned(),
            region: region.to_owned(),
            prefix: prefix.to_owned(),
        })
    }
}

pub(crate) struct S3Objects {
    location: Location,
    credentials: Credentials,
    agent: ureq::Agent,
    clock: fn() -> u64,
}

impl S3Objects {
    /// `timeout` bounds every request; a slow witness is UNKNOWN.
    pub(crate) fn new(location: Location, credentials: Credentials, timeout: Duration) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout(timeout)
            .redirects(0)
            .build();
        Self {
            location,
            credentials,
            agent,
            clock: unix_now,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_clock(mut self, clock: fn() -> u64) -> Self {
        self.clock = clock;
        self
    }

    fn path(&self, key: &str) -> String {
        format!("/{}/{}/{}", self.location.bucket, self.location.prefix, key)
    }

    fn signed(
        &self,
        method: &str,
        path: &str,
        query: &[(&str, String)],
        extra: &[(&str, &str)],
        body: &[u8],
    ) -> ureq::Request {
        let stamp = Stamp::at((self.clock)());
        let payload = hex::encode(Sha256::digest(body));
        let mut headers: Vec<(String, String)> = vec![
            ("host".into(), self.location.host.clone()),
            ("x-amz-content-sha256".into(), payload.clone()),
            ("x-amz-date".into(), stamp.long.clone()),
        ];
        headers.extend(
            extra
                .iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), (*v).to_owned())),
        );
        headers.sort();
        let authorization = authorization(
            &self.credentials,
            &self.location.region,
            &stamp,
            method,
            path,
            query,
            &headers,
            &payload,
        );
        let url = if query.is_empty() {
            format!("{}{}", self.location.endpoint, path)
        } else {
            format!(
                "{}{}?{}",
                self.location.endpoint,
                path,
                canonical_query(query)
            )
        };
        let mut request = self.agent.request(method, &url);
        for (k, v) in &headers {
            if k != "host" {
                request = request.set(k, v);
            }
        }
        request.set("authorization", &authorization)
    }
}

impl Objects for S3Objects {
    fn get(&self, key: &str) -> Reply<Option<Object>> {
        let response = match self.signed("GET", &self.path(key), &[], &[], b"").call() {
            Ok(response) => response,
            Err(ureq::Error::Status(404, _)) => return Reply::Ok(None),
            Err(_) => return Reply::Unknown,
        };
        if response.status() != 200 {
            return Reply::Unknown;
        }
        let version = response
            .header("x-amz-version-id")
            .or_else(|| response.header("etag"))
            .filter(|v| !v.is_empty() && v.len() <= 256 && v.is_ascii())
            .map(str::to_owned);
        let Some(version) = version else {
            return Reply::Unknown;
        };
        match bounded(response, MAX_BODY) {
            Some(bytes) => Reply::Ok(Some(Object { bytes, version })),
            None => Reply::Unknown,
        }
    }

    fn create(&self, key: &str, bytes: &[u8]) -> Reply<Created> {
        let request = self.signed(
            "PUT",
            &self.path(key),
            &[],
            &[("if-none-match", "*")],
            bytes,
        );
        match request.send_bytes(bytes) {
            Ok(response) if (200..300).contains(&response.status()) => Reply::Ok(Created::New),
            Err(ureq::Error::Status(412, _)) => Reply::Ok(Created::Exists),
            _ => Reply::Unknown,
        }
    }

    fn list(&self, limit: usize) -> Reply<Vec<String>> {
        let prefix = format!("{}/", self.location.prefix);
        let query = [
            ("list-type", "2".to_owned()),
            ("max-keys", (limit + 1).to_string()),
            ("prefix", prefix.clone()),
        ];
        let path = format!("/{}", self.location.bucket);
        let response = match self.signed("GET", &path, &query, &[], b"").call() {
            Ok(response) if response.status() == 200 => response,
            _ => return Reply::Unknown,
        };
        let Some(body) = bounded(response, MAX_LIST_BODY) else {
            return Reply::Unknown;
        };
        let Ok(body) = String::from_utf8(body) else {
            return Reply::Unknown;
        };
        match list_keys(&body, &prefix) {
            Some(names) if names.len() <= limit => Reply::Ok(names),
            _ => Reply::Unknown,
        }
    }
}

/// Read at most `max` bytes; one byte more means the body is refused.
fn bounded(response: ureq::Response, max: u64) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(max + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= max).then_some(bytes)
}

/// Strict scan of a `ListObjectsV2` body. Only canonical ASCII names under the
/// lineage prefix are representable; a truncated listing, an escaped or
/// unexpected character, or a key outside the prefix is UNKNOWN.
fn list_keys(body: &str, prefix: &str) -> Option<Vec<String>> {
    if !body.contains("<ListBucketResult") {
        return None;
    }
    match between(body, "<IsTruncated>", "</IsTruncated>")? {
        "false" => {}
        _ => return None,
    }
    let mut names = Vec::new();
    let mut rest = body;
    while let Some(at) = rest.find("<Key>") {
        rest = &rest[at + "<Key>".len()..];
        let end = rest.find("</Key>")?;
        let key = &rest[..end];
        rest = &rest[end..];
        if !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'.' | b'_'))
        {
            return None;
        }
        names.push(key.strip_prefix(prefix)?.to_owned());
    }
    if let Some(count) = between(body, "<KeyCount>", "</KeyCount>")
        && count.parse::<usize>().ok()? != names.len()
    {
        return None;
    }
    Some(names)
}

fn between<'a>(body: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = body.find(open)? + open.len();
    let end = body[start..].find(close)? + start;
    Some(&body[start..end])
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `SigV4` date stamps.
pub(crate) struct Stamp {
    pub(crate) short: String,
    pub(crate) long: String,
}

impl Stamp {
    pub(crate) fn at(unix: u64) -> Self {
        let days = (unix / 86_400) as i64;
        let secs = unix % 86_400;
        let (y, m, d) = civil(days);
        let short = format!("{y:04}{m:02}{d:02}");
        let long = format!(
            "{short}T{:02}{:02}{:02}Z",
            secs / 3600,
            (secs / 60) % 60,
            secs % 60
        );
        Self { short, long }
    }
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    // Day is 1..=31 and month 1..=12 by construction; refuse rather than wrap.
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(0);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(0);
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

fn canonical_query(query: &[(&str, String)]) -> String {
    let mut pairs: Vec<(String, String)> = query
        .iter()
        .map(|(k, v)| (uri_encode(k), uri_encode(v)))
        .collect();
    pairs.sort();
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// RFC 3986 unreserved characters pass; everything else is `%XX` (`SigV4`).
fn uri_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn authorization(
    credentials: &Credentials,
    region: &str,
    stamp: &Stamp,
    method: &str,
    path: &str,
    query: &[(&str, String)],
    headers: &[(String, String)],
    payload: &str,
) -> String {
    let canonical_headers: String = headers
        .iter()
        .map(|(k, v)| format!("{k}:{}\n", v.trim()))
        .collect();
    let signed_headers = headers
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let canonical = format!(
        "{method}\n{path}\n{}\n{canonical_headers}\n{signed_headers}\n{payload}",
        canonical_query(query)
    );
    let scope = format!("{}/{region}/s3/aws4_request", stamp.short);
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{scope}\n{}",
        stamp.long,
        hex::encode(Sha256::digest(canonical.as_bytes()))
    );
    let key = [
        stamp.short.as_bytes(),
        region.as_bytes(),
        b"s3",
        b"aws4_request",
    ]
    .iter()
    .fold(
        format!("AWS4{}", credentials.secret).into_bytes(),
        |key, part| hmac_sha256(&key, part).to_vec(),
    );
    let signature = hex::encode(hmac_sha256(&key, to_sign.as_bytes()));
    format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
        credentials.access_key_id
    )
}

/// HMAC-SHA256 (RFC 2104).
pub(crate) fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut block = [0u8; BLOCK];
    if key.len() > BLOCK {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let pad = |byte: u8| block.map(|b| b ^ byte);
    let inner = Sha256::new()
        .chain_update(pad(0x36))
        .chain_update(message)
        .finalize();
    Sha256::new()
        .chain_update(pad(0x5c))
        .chain_update(inner)
        .finalize()
        .into()
}

#[cfg(test)]
#[path = "s3_tests.rs"]
mod tests;
