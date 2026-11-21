//! Opt-in, per-store sharing of overlapping local reads. Never a result cache.
use super::*;
use parking_lot::Mutex;
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::watch;

const MAX_FLIGHTS: usize = 32;
const MAX_FOLLOWERS: usize = 16;
const MAX_RESULT_BYTES: u64 = 1024 * 1024;
type Generation = (i64, i64);
type Key = ([u8; 32], Generation);

pub(crate) fn configured(path: &str, enabled: bool, composed: bool) -> Option<Arc<Flights>> {
    if !enabled {
        return None;
    }
    let flights = (!composed).then(|| Flights::open(path)).flatten();
    eprintln!(
        "query single-flight: {}",
        if flights.is_some() {
            "enabled (32 flights, 16 followers, 1MiB/result)"
        } else {
            "unavailable for this store; preserving ordinary reads"
        }
    );
    flights
}

pub(crate) struct Flights {
    // One connection for generation probes only: no scans or evaluator work.
    marker: Mutex<Connection>,
    probe: tokio::sync::Semaphore,
    active: Mutex<HashMap<Key, Arc<Flight>>>,
    stats: Mutex<Stats>,
    #[cfg(test)]
    pin_hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
#[derive(Default)]
struct Stats {
    coalesced: u64,
    leaders: u64,
    leader_seconds: f64,
}
struct Flight {
    done: watch::Sender<Option<Arc<Answer>>>,
}
struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
    usage: Option<quipu::request_usage::RequestUsage>,
}
impl Answer {
    fn response(&self) -> axum::response::Response {
        let mut response = axum::body::Body::from(self.body.clone()).into_response();
        *response.status_mut() = self.status;
        *response.headers_mut() = self.headers.clone();
        if let Some(usage) = self.usage {
            response.extensions_mut().insert(usage);
        }
        response
    }
}
impl Flights {
    pub(crate) fn open(path: &str) -> Option<Arc<Self>> {
        if path == ":memory:" {
            return None;
        }
        let connection =
            Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
        connection.busy_timeout(Duration::ZERO).ok()?;
        connection
            .execute_batch("PRAGMA cache_size=-64; PRAGMA mmap_size=0; PRAGMA query_only=ON;")
            .ok()?;
        let flights = Arc::new(Self {
            marker: Mutex::new(connection),
            probe: tokio::sync::Semaphore::new(1),
            active: Mutex::new(HashMap::new()),
            stats: Mutex::new(Stats::default()),
            #[cfg(test)]
            pin_hook: Mutex::new(None),
        });
        // Do not advertise an enabled component on an unsupported/unproven store.
        flights.generation().is_some().then_some(flights)
    }
    fn generation(&self) -> Option<Generation> {
        // Never wait behind a generation probe or a SQLite writer. Missing or
        // uncertain proof takes the original path, not an inferred generation.
        self.generation_with_wait(Duration::ZERO)
    }
    fn generation_with_wait(&self, wait: Duration) -> Option<Generation> {
        let conn = self.marker.try_lock_for(wait)?;
        let version = || {
            conn.query_row("PRAGMA data_version", [], |row| row.get::<_, i64>(0))
                .ok()
        };
        let before = version()?;
        // Frozen packs and mounted sources have independent generations.
        // Until all are fenced, never coalesce reads of a composed store.
        let frozen: bool = conn
            .query_row("SELECT EXISTS(SELECT 1 FROM frozen_packs)", [], |row| {
                row.get(0)
            })
            .ok()?;
        if frozen {
            return None;
        }
        let tx = conn
            .query_row("SELECT COALESCE(MAX(id),0) FROM transactions", [], |row| {
                row.get(0)
            })
            .ok()?;
        (version()? == before).then_some((before, tx))
    }
    async fn generation_for(&self, deadline: quipu::time::Deadline) -> Option<Generation> {
        // Concurrent followers must not turn a tiny proof probe into a storm
        // of duplicate evaluator work. Queue asynchronously for at most5ms,
        // inside the caller's remaining budget; SQLite itself never waits.
        let wait = Duration::from_millis(deadline.remaining_millis().min(5));
        let _permit = tokio::time::timeout(wait, self.probe.acquire())
            .await
            .ok()?
            .ok()?;
        if deadline.passed() {
            return None;
        }
        self.generation()
    }
    fn join(self: &Arc<Self>, key: Key) -> Option<Join> {
        let mut active = self.active.lock();
        if let Some(flight) = active.get(&key) {
            // receiver_count is live, so cancelled/timed-out followers free a slot.
            if flight.done.receiver_count() >= MAX_FOLLOWERS {
                return None;
            }
            return Some(Join::Follower(flight.done.subscribe()));
        }
        if active.len() >= MAX_FLIGHTS {
            return None;
        }
        let (done, _) = watch::channel(None);
        let flight = Arc::new(Flight { done });
        active.insert(key, flight.clone());
        Some(Join::Leader(Leader {
            owner: self.clone(),
            key,
            flight,
            started: Instant::now(),
        }))
    }
    pub(crate) fn render(&self, body: &mut String) {
        use std::fmt::Write;
        let stats = self.stats.lock();
        body.push_str("# HELP quipu_query_singleflight_coalesced_total Followers served without another query execution.\n# TYPE quipu_query_singleflight_coalesced_total counter\n");
        let _ = writeln!(
            body,
            "quipu_query_singleflight_coalesced_total {}",
            stats.coalesced
        );
        body.push_str("# HELP quipu_query_singleflight_leader_duration_seconds Leader wall time including admission and response serialization.\n# TYPE quipu_query_singleflight_leader_duration_seconds summary\n");
        let _ = writeln!(
            body,
            "quipu_query_singleflight_leader_duration_seconds_count {}\nquipu_query_singleflight_leader_duration_seconds_sum {}",
            stats.leaders, stats.leader_seconds
        );
    }
}
pub(super) struct Proof {
    owner: Arc<Flights>,
    generation: Generation,
    valid: AtomicBool,
}
impl Proof {
    pub(super) fn observe(&self, head: i64) {
        let valid = head == self.generation.1
            && self.owner.generation_with_wait(Duration::from_millis(5)) == Some(self.generation);
        self.valid.store(valid, Ordering::Release);
        #[cfg(test)]
        if let Some(hook) = self.owner.pin_hook.lock().take() {
            hook();
        }
    }
}
enum Join {
    Leader(Leader),
    Follower(watch::Receiver<Option<Arc<Answer>>>),
}
struct Leader {
    owner: Arc<Flights>,
    key: Key,
    flight: Arc<Flight>,
    started: Instant,
}
impl Leader {
    fn remove(&self) {
        let mut active = self.owner.active.lock();
        if active
            .get(&self.key)
            .is_some_and(|current| Arc::ptr_eq(current, &self.flight))
        {
            active.remove(&self.key);
        }
    }
}
impl Drop for Leader {
    fn drop(&mut self) {
        self.remove();
        // Wake existing followers on cancellation, panic, error or oversize.
        if self.flight.done.borrow().is_none() {
            self.flight.done.send_replace(None);
        }
        let mut stats = self.owner.stats.lock();
        stats.leaders += 1;
        stats.leader_seconds += self.started.elapsed().as_secs_f64();
    }
}
fn fingerprint(input: &JsonValue, headers: &HeaderMap, is_post: bool) -> Option<[u8; 32]> {
    if input.get("federated").and_then(JsonValue::as_bool) == Some(true)
        || input.get("execution_profile").and_then(JsonValue::as_bool) == Some(true)
    {
        return None;
    }
    let text = input.get("query")?.as_str()?;
    // Match tokens, not substrings such as ?known or the value "UNKNOWN".
    // Comments may separate a function keyword from its opening parenthesis.
    // Conservatively matching inside a literal can still bypass sharing safely.
    static NONDETERMINISTIC: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let excluded = NONDETERMINISTIC.get_or_init(|| regex::Regex::new(
        r"(?i)(?:^|[^[:alnum:]_?:$])SERVICE(?:\s|#|<|\?)|\b(?:RAND|UUID|STRUUID|BNODE|NOW)(?:\s|#[^\r\n]*(?:[\r\n]|$))*\("
    ).expect("constant SPARQL non-determinism guard"));
    if excluded.is_match(text) {
        return None;
    }
    let mut normalized = input.clone();
    // Do not collapse internal whitespace: that would alter literal values.
    normalized["query"] = json!(text.trim());
    let mut digest = Sha256::new();
    digest.update(serde_json::to_vec(&normalized).ok()?);
    digest.update([u8::from(is_post)]);
    // Authenticated principal/credential survives private worker handoff.
    // Identity's serialization excludes per-request attestation/replay tokens.
    digest.update(serde_json::to_vec(&crate::auth::request_identity()).ok()?);
    for name in [
        "accept",
        "x-quipu-require-substring",
        "x-quipu-operation",
        "authorization",
        "cookie",
    ] {
        let values: Vec<_> = headers.get_all(name).iter().collect();
        digest.update((values.len() as u64).to_le_bytes());
        for value in values {
            digest.update((value.as_bytes().len() as u64).to_le_bytes());
            digest.update(value.as_bytes());
        }
    }
    Some(digest.finalize().into())
}

pub(super) async fn answer(
    store: SharedStore,
    headers: HeaderMap,
    input: JsonValue,
    is_post: bool,
    shape: quipu::request_usage::QueryShape,
) -> Result<axum::response::Response, AppError> {
    let Some(owner) = store.query_flights.clone() else {
        return answer_query(store, headers, input, is_post, shape, None).await;
    };
    let budget = store.search_config.request_timeout_ms;
    let wait_ms = if budget > 0 {
        budget
    } else {
        match store.search_config.query_timeout_ms {
            0 => 30_000,
            ms => ms.min(30_000),
        }
    };
    let deadline = quipu::time::Deadline::after_millis(wait_ms);
    let Some(hash) = fingerprint(&input, &headers, is_post) else {
        return answer_query(store, headers, input, is_post, shape, None).await;
    };
    let generation = owner.generation_for(deadline).await;
    if deadline.passed() {
        return Err(deadline.request_timeout().into());
    }
    let Some(generation) = generation else {
        return answer_query(store, headers, input, is_post, shape, None).await;
    };
    let key = (hash, generation);
    match owner.join(key) {
        Some(Join::Follower(mut receiver)) => {
            let ready = tokio::time::timeout(
                Duration::from_millis(deadline.remaining_millis()),
                receiver.changed(),
            )
            .await;
            if ready.is_err() {
                return Err(deadline.request_timeout().into());
            }
            let answer = receiver.borrow_and_update().clone();
            if ready.is_ok_and(|result| result.is_ok())
                && let Some(answer) = answer
                && !deadline.passed()
            {
                owner.stats.lock().coalesced += 1;
                return Ok(answer.response());
            }
            if deadline.passed() || deadline.remaining_millis() == 0 {
                return Err(deadline.request_timeout().into());
            }
            // A cancelled/failed leader or intervening write is not an empty
            // answer. Retry the original path within this follower's budget.
            tokio::time::timeout(
                Duration::from_millis(deadline.remaining_millis()),
                answer_query(store, headers, input, is_post, shape, None),
            )
            .await
            .map_err(|_| AppError::from(deadline.request_timeout()))?
        }
        Some(Join::Leader(leader)) => {
            let proof = Arc::new(Proof {
                owner: owner.clone(),
                generation: key.1,
                valid: AtomicBool::new(false),
            });
            let response =
                answer_query(store, headers, input, is_post, shape, Some(proof.clone())).await?;
            use axum::body::HttpBody;
            if !response.status().is_success()
                || response
                    .body()
                    .size_hint()
                    .exact()
                    .is_none_or(|n| n > MAX_RESULT_BYTES)
            {
                return Ok(response);
            }
            let (parts, body) = response.into_parts();
            let body = axum::body::to_bytes(body, MAX_RESULT_BYTES as usize)
                .await
                .map_err(|error| {
                    AppError::from(quipu::Error::Store(format!(
                        "query response buffering: {error}"
                    )))
                })?;
            let answer = Arc::new(Answer {
                status: parts.status,
                headers: parts.headers.clone(),
                body: body.clone(),
                usage: parts.extensions.get().copied(),
            });
            // Remove BEFORE notification: a later arrival cannot reuse a
            // completed answer, even at an unchanged watermark.
            leader.remove();
            if proof.valid.load(Ordering::Acquire) {
                leader.flight.done.send_replace(Some(answer));
            }
            Ok(axum::response::Response::from_parts(
                parts,
                axum::body::Body::from(body),
            ))
        }
        None => answer_query(store, headers, input, is_post, shape, None).await,
    }
}

#[cfg(test)]
mod tests;
