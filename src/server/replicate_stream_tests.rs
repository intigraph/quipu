//! The replication stream end to end: a follower fed only by
//! `POST /replicate/stream` equals the source in its scope, follows a new
//! commit within the 5 s P2 bound, and gets a freshness bound only through
//! `POST /replicate/challenge`.
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{extract::State, http::StatusCode};
use futures::StreamExt;
use quipu::applier::{Applier, Outcome};
use quipu::replicate_source::StreamRequest;
use quipu::store::{Datum, Store};
use quipu::types::{Op, Value};
use ring::signature::Ed25519KeyPair;

use super::{ChallengeRequest, challenge, sessions, stream};
use crate::{SharedStore, StoreHandle};

const T: &str = "2026-10-24T00:00:00Z";
const BOARD: &str = "urn:graph:board";

fn write(store: &mut Store, entity: &str, value: &str) {
    let datum = Datum {
        entity: store.intern(entity).unwrap(),
        attribute: store.intern("urn:p:title").unwrap(),
        value: Value::Str(value.into()),
        valid_from: T.into(),
        valid_to: None,
        op: Op::Assert,
    };
    let g = store.graph_create(BOARD).unwrap();
    store.transact_to_graph(&[datum], T, None, None, g).unwrap();
}

/// A signing source with two board items, and its public stream key.
fn source(signing: bool) -> (SharedStore, String) {
    let mut store = Store::open_in_memory().unwrap();
    write(&mut store, "urn:item:1", "one");
    write(&mut store, "urn:item:2", "two");
    let doc = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
    let identity = quipu::signing::SigningIdentity::new(
        Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap(),
        "quipu",
    );
    let key = identity.public_key_hex();
    if signing {
        store.set_signing_identity(Arc::new(identity));
    }
    (Arc::new(StoreHandle::writer_only(store)), key)
}

/// A file-backed source with a real reader pool, as production serves (a pool of
/// 8 readers). With no pool `read()` IS the writer, so a writer-only
/// source cannot show whether an answer avoids the writer lock.
fn pooled_source() -> (SharedStore, String, tempfile::TempDir) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("source.db");
    let path = path.to_str().unwrap();
    let mut store = Store::open(path).unwrap();
    write(&mut store, "urn:item:1", "one");
    write(&mut store, "urn:item:2", "two");
    let doc = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
    let identity = quipu::signing::SigningIdentity::new(
        Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap(),
        "quipu",
    );
    let key = identity.public_key_hex();
    store.set_signing_identity(Arc::new(identity));
    let readers = crate::ReadPool::open(path, &store, 2);
    assert!(readers.len() > 0, "the regression needs a real reader pool");
    let shared = Arc::new(StoreHandle::serving(
        store,
        readers,
        path,
        quipu::config::FederationConfig::default(),
    ));
    (shared, key, temp)
}

fn request(shared: &SharedStore) -> StreamRequest {
    serde_json::from_value(serde_json::json!({
        "graphs": [BOARD], "since": 0,
        "expected_store_id": shared.read().store_id().unwrap(),
    }))
    .unwrap()
}

/// Parses SSE events out of the body as they arrive.
struct Events {
    bytes: axum::body::BodyDataStream,
    buffer: String,
}

impl Events {
    async fn next(&mut self, within: Duration) -> Option<(String, String)> {
        let deadline = Instant::now() + within;
        loop {
            if let Some(end) = self.buffer.find("\n\n") {
                let block: String = self.buffer.drain(..end + 2).collect();
                let mut event = String::new();
                let mut data = String::new();
                for line in block.lines() {
                    if let Some(v) = line.strip_prefix("event: ") {
                        event = v.to_string();
                    } else if let Some(v) = line.strip_prefix("data: ") {
                        data = v.to_string();
                    }
                }
                if !event.is_empty() {
                    return Some((event, data));
                }
                continue;
            }
            let left = deadline.checked_duration_since(Instant::now())?;
            let chunk = tokio::time::timeout(left, self.bytes.next()).await.ok()??;
            self.buffer
                .push_str(std::str::from_utf8(&chunk.ok()?).unwrap());
        }
    }
}

struct Follower {
    store: Store,
    applier: Applier,
    events: Events,
    clock: Instant,
}

impl Follower {
    async fn connect(shared: &SharedStore, key: &str) -> Self {
        let response = stream(State(shared.clone()), axum::Json(request(shared))).await;
        assert_eq!(response.status(), StatusCode::OK);
        let store = Store::open_in_memory().unwrap();
        let mut applier = Applier::open(&store).unwrap();
        let scope = quipu::asof_share::scope_digest(&[BOARD.to_string()], &[]);
        applier.trust_stream(key, &scope);
        Self {
            store,
            applier,
            events: Events {
                bytes: response.into_body().into_data_stream(),
                buffer: String::new(),
            },
            clock: Instant::now(),
        }
    }

    fn now(&self) -> u64 {
        u64::try_from(self.clock.elapsed().as_millis()).unwrap()
    }

    /// Apply frames until `until` matches an outcome, or `within` passes.
    async fn until(
        &mut self,
        within: Duration,
        until: impl Fn(&Outcome) -> bool,
    ) -> Option<Outcome> {
        let deadline = Instant::now() + within;
        loop {
            let left = deadline.checked_duration_since(Instant::now())?;
            let (event, data) = self.events.next(left).await?;
            let now = self.now();
            let outcome = self
                .applier
                .apply_frame(&mut self.store, &event, &data, now)
                .unwrap();
            if until(&outcome) {
                return Some(outcome);
            }
        }
    }
}

fn board(store: &Store) -> std::collections::BTreeSet<String> {
    // The live facts of the board, by meaning (term ids differ per store).
    let g = store.registered_graph_id(BOARD).unwrap().unwrap();
    let page = store
        .changes_in_graphs(0, 1_000, quipu::store::changes::Capture::NewValues, &[g])
        .unwrap();
    page.records
        .iter()
        .map(|r| {
            format!(
                "{} {} {} {}",
                r["op"], r["entity"], r["attribute"], r["value"]
            )
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_follower_fed_only_by_the_stream_tracks_the_source_within_five_seconds() {
    let (shared, key) = source(true);
    let mut follower = Follower::connect(&shared, &key).await;
    let head = shared.read().latest_tx_id().unwrap();
    let caught_up = follower
        .until(
            Duration::from_secs(5),
            |o| matches!(o, Outcome::CoverageAdvanced { watermark } if *watermark == head),
        )
        .await;
    assert!(caught_up.is_some(), "no coverage of the head");
    assert_eq!(board(&follower.store), board(&shared.read()));

    // A new commit reaches the follower from the stream alone, within 5 s.
    let committed = Instant::now();
    write(&mut shared.lock(), "urn:item:3", "three");
    let head = shared.read().latest_tx_id().unwrap();
    let followed = follower
        .until(
            Duration::from_secs(5),
            |o| matches!(o, Outcome::CoverageAdvanced { watermark } if *watermark == head),
        )
        .await;
    assert!(
        followed.is_some(),
        "the new commit did not arrive within 5 s"
    );
    assert!(committed.elapsed() < Duration::from_secs(5));
    assert_eq!(board(&follower.store), board(&shared.read()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_challenge_through_the_route_bounds_freshness() {
    let (shared, key) = source(true);
    let mut follower = Follower::connect(&shared, &key).await;
    let head = shared.read().latest_tx_id().unwrap();
    follower
        .until(
            Duration::from_secs(5),
            |o| matches!(o, Outcome::CoverageAdvanced { watermark } if *watermark == head),
        )
        .await
        .unwrap();
    assert_eq!(
        follower
            .applier
            .freshness(follower.now())
            .staleness_bound_ms,
        None
    );
    let session = follower.applier.session().unwrap().to_string();
    let nonce = [0x5a; 16];
    let sent = follower.now();
    follower.applier.challenge(&session, nonce, sent).unwrap();
    let response = challenge(axum::Json(ChallengeRequest {
        session: session.clone(),
        nonce: hex::encode(nonce),
    }))
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let proof = follower
        .until(Duration::from_secs(5), |o| {
            matches!(o, Outcome::ProofAccepted { .. })
        })
        .await;
    assert_eq!(proof, Some(Outcome::ProofAccepted { t_send_ms: sent }));
    let freshness = follower.applier.freshness(follower.now());
    assert!(
        freshness.bounded && freshness.within(5_000),
        "{freshness:?}"
    );
}

/// aegis-awpw: a challenge that arrives while a hinted commit is still holding
/// the writer lock is answered from the committed head, not after the write.
/// Before the fix the producer awaited `writer.lock()` inline after every hint,
/// so this proof only came once the 3 s write released the lock.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_challenge_is_answered_while_a_hinted_write_holds_the_writer() {
    let (shared, key, _temp) = pooled_source();
    let mut follower = Follower::connect(&shared, &key).await;
    let head = shared.read().latest_tx_id().unwrap();
    follower
        .until(
            Duration::from_secs(5),
            |o| matches!(o, Outcome::CoverageAdvanced { watermark } if *watermark == head),
        )
        .await
        .unwrap();
    // A write in flight: hold the real writer lock for 3 s, then hint a commit.
    let (held_tx, held_rx) = std::sync::mpsc::channel();
    let writer = shared.clone();
    let holder = std::thread::spawn(move || {
        let _write = writer.writer.lock();
        held_tx.send(()).unwrap();
        std::thread::sleep(Duration::from_secs(3));
    });
    held_rx.recv().unwrap();
    shared.commit_wake.send_modify(|_| ());
    tokio::time::sleep(Duration::from_millis(100)).await; // the producer is now waiting on the write
    let session = follower.applier.session().unwrap().to_string();
    let nonce = [0x6b; 16];
    let sent = follower.now();
    follower.applier.challenge(&session, nonce, sent).unwrap();
    let started = Instant::now();
    let response = challenge(axum::Json(ChallengeRequest {
        session: session.clone(),
        nonce: hex::encode(nonce),
    }))
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let proof = follower
        .until(Duration::from_secs(1), |o| {
            matches!(o, Outcome::ProofAccepted { .. })
        })
        .await;
    let waited = started.elapsed();
    assert_eq!(
        proof,
        Some(Outcome::ProofAccepted { t_send_ms: sent }),
        "no proof within 1 s while the writer was held (waited {waited:?})"
    );
    assert!(
        !holder.is_finished(),
        "the write must still be in flight when the proof lands"
    );
    holder.join().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_answered_challenge_records_every_stage() {
    // aegis-awpw: the histogram is process-wide and other tests also answer
    // challenges, so assert growth, never exact totals.
    fn counts() -> std::collections::BTreeMap<String, u64> {
        let mut body = String::new();
        super::timing::render(&mut body);
        body.lines()
            .filter_map(|line| {
                line.strip_prefix("quipu_replicate_challenge_seconds_count{stage=\"")
            })
            .map(|rest| {
                let (stage, value) = rest.split_once("\"} ").unwrap();
                (stage.to_string(), value.parse().unwrap())
            })
            .collect()
    }
    let (shared, key) = source(true);
    let mut follower = Follower::connect(&shared, &key).await;
    let head = shared.read().latest_tx_id().unwrap();
    follower
        .until(
            Duration::from_secs(5),
            |o| matches!(o, Outcome::CoverageAdvanced { watermark } if *watermark == head),
        )
        .await
        .unwrap();
    let before = counts();
    assert_eq!(before.len(), 4, "{before:?}");
    let session = follower.applier.session().unwrap().to_string();
    let nonce = [0x6b; 16];
    let sent = follower.now();
    follower.applier.challenge(&session, nonce, sent).unwrap();
    let started = Instant::now();
    let response = challenge(axum::Json(ChallengeRequest {
        session,
        nonce: hex::encode(nonce),
    }))
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let proof = follower
        .until(Duration::from_secs(5), |o| {
            matches!(o, Outcome::ProofAccepted { .. })
        })
        .await;
    assert_eq!(proof, Some(Outcome::ProofAccepted { t_send_ms: sent }));
    let elapsed = started.elapsed();
    let after = counts();
    for stage in ["inbox", "admission", "poll", "answer"] {
        assert!(
            after[stage] > before[stage],
            "stage {stage} not observed: {before:?} -> {after:?}"
        );
    }
    // The answer stage cannot exceed what the follower waited end to end.
    assert!(elapsed < Duration::from_secs(5));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_challenge_needs_an_open_session_and_a_sixteen_byte_nonce() {
    let unknown = challenge(axum::Json(ChallengeRequest {
        session: "00".repeat(16),
        nonce: "00".repeat(16),
    }))
    .await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    let short = challenge(axum::Json(ChallengeRequest {
        session: "00".repeat(16),
        nonce: "00".into(),
    }))
    .await;
    assert_eq!(short.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_source_without_a_signing_key_serves_no_stream() {
    let (shared, _) = source(false);
    let response = stream(State(shared.clone()), axum::Json(request(&shared))).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_scope_the_source_does_not_serve_is_refused_before_the_stream() {
    let (shared, _) = source(true);
    let mut wrong = request(&shared);
    wrong.graphs = vec!["urn:graph:missing".into()];
    let response = stream(State(shared.clone()), axum::Json(wrong)).await;
    assert!(response.status().is_client_error(), "{}", response.status());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_follower_that_goes_away_frees_its_session() {
    let (shared, key) = source(true);
    let mut follower = Follower::connect(&shared, &key).await;
    follower
        .until(Duration::from_secs(5), |o| {
            matches!(o, Outcome::SessionOpened { .. })
        })
        .await
        .unwrap();
    let session = follower.applier.session().unwrap().to_string();
    assert!(sessions().lock().unwrap().contains_key(&session));
    drop(follower);
    let deadline = Instant::now() + Duration::from_secs(5);
    while sessions().lock().unwrap().contains_key(&session) {
        assert!(Instant::now() < deadline, "session still registered");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[path = "replicate_stream_relay_tests.rs"]
mod relay;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ownership_unadopted_disk_copy_is_forbidden_before_http_stream_admission() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("copy.db");
    let original = Store::open_in_memory().unwrap();
    original.graph_create(BOARD).unwrap();
    let bytes = original.serialize_db().unwrap();
    std::fs::write(&path, bytes).unwrap();
    Store::adopt(path.to_str().unwrap()).unwrap();
    std::fs::remove_file(path.with_extension("db.ownership.json")).unwrap();
    let mut copy = Store::open(path.to_str().unwrap()).unwrap();
    let doc = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
    copy.set_signing_identity(Arc::new(quipu::signing::SigningIdentity::new(
        Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap(),
        "quipu",
    )));
    let shared = Arc::new(StoreHandle::writer_only(copy));
    let response = stream(State(shared.clone()), axum::Json(request(&shared))).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    Store::adopt(path.to_str().unwrap()).unwrap();
    // Same data/handler/signing identity, now an explicitly owned placement.
    let response = stream(State(shared.clone()), axum::Json(request(&shared))).await;
    assert_eq!(response.status(), StatusCode::OK);
}
