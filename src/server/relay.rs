//! A relay: a replica that serves its own children
//! (docs/design/shared-applier-api.md §6, local-sync-mode.md §6 "Fan-out").
//!
//! One task follows the upstream stream into the local store through the
//! shared applier. Each applied commit wakes the local producers
//! (`commit_wake`), which serve children in source numbering
//! (`Session::open_relay`). The relay signs its children's frames with its own
//! stream key; a child pins the relay's key, never the master's.
//!
//! A child's challenge is answered only from an upstream proof that the relay
//! requested AFTER the child's challenge arrived ([`Relay::fresh_proof`]).
//! While the upstream session is down no proof arrives, so the child's
//! freshness stays UNKNOWN.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use parking_lot::Mutex;
use quipu::applier::{Applier, Outcome};
use quipu::replicate_source::StreamRequest;
use tokio::sync::watch;

use crate::SharedStore;

/// Wait before reconnecting to the upstream after its stream ends.
const RECONNECT: Duration = Duration::from_secs(2);

/// How the relay reaches its upstream: HTTP in a deployment, the upstream's
/// route handlers in tests.
pub(crate) trait Upstream: Send + Sync {
    /// Open a stream session; the body's SSE bytes, as text chunks.
    fn open(
        &self,
        request: StreamRequest,
    ) -> BoxFuture<'static, Option<BoxStream<'static, String>>>;
    /// Send a freshness challenge on an open session; `true` once accepted.
    fn challenge(&self, session: String, nonce: [u8; 16]) -> BoxFuture<'static, bool>;
}

/// What a relay serves and how it follows its upstream.
pub(crate) struct Relay {
    /// The relay's scope: children are served only inside it.
    pub(crate) graphs: Vec<String>,
    pub(crate) context_graphs: Vec<String>,
    upstream_store_id: String,
    upstream: Arc<dyn Upstream>,
    applier: Mutex<Applier>,
    /// The last upstream proof: (`t_send_ms` of its challenge, coverage).
    proofs: watch::Sender<Option<(u64, i64)>>,
    clock: Instant,
    /// The last challenge time issued, so challenge times strictly increase.
    last_challenge_ms: Mutex<u64>,
}

impl Relay {
    /// A relay over `store`, following `upstream_store_id` through `upstream`
    /// for the scope `graphs` and `context_graphs`.
    ///
    /// # Errors
    /// A store error opening the applier.
    pub(crate) fn new(
        store: &quipu::Store,
        graphs: Vec<String>,
        context_graphs: Vec<String>,
        upstream_store_id: String,
        upstream_key_hex: &str,
        upstream: Arc<dyn Upstream>,
    ) -> quipu::Result<Self> {
        let mut applier = Applier::open(store)?;
        let scope = quipu::asof_share::scope_digest(&graphs, &context_graphs);
        applier.trust_stream(upstream_key_hex, &scope);
        Ok(Self {
            graphs,
            context_graphs,
            upstream_store_id,
            upstream,
            applier: Mutex::new(applier),
            proofs: watch::channel(None).0,
            clock: Instant::now(),
            last_challenge_ms: Mutex::new(0),
        })
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.clock.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// The relay's freshness from its upstream, on its own clock.
    pub(crate) fn freshness(&self) -> quipu::applier::Freshness {
        self.applier.lock().freshness(self.now_ms())
    }

    /// Request an upstream proof now and wait up to `within` for it. Returns
    /// the proven upstream coverage `W_up`, held locally, from a challenge
    /// sent after this call began; `None` if the upstream session is down or
    /// does not answer in time.
    pub(crate) async fn fresh_proof(&self, within: Duration) -> Option<i64> {
        let mut proofs = self.proofs.subscribe();
        let t_send = {
            let mut last = self.last_challenge_ms.lock();
            *last = self.now_ms().max(*last + 1);
            *last
        };
        let nonce = random_nonce()?;
        let session = {
            let mut applier = self.applier.lock();
            let session = applier.session()?.to_string();
            applier.challenge(&session, nonce, t_send).ok()?;
            session
        };
        if !self.upstream.challenge(session, nonce).await {
            return None;
        }
        let wait = async {
            loop {
                // Any proof of a challenge sent at or after `t_send` was
                // requested after this call began: challenge times only grow.
                if let Some((t, watermark)) = *proofs.borrow_and_update()
                    && t >= t_send
                {
                    return Some(watermark);
                }
                proofs.changed().await.ok()?;
            }
        };
        tokio::time::timeout(within, wait).await.ok().flatten()
    }
}

/// The relay a configured server runs, or why it cannot run one. A relay
/// store holds only what its applier wrote, signs its children's frames with
/// its own key and trusts upstream coverage only under the pinned key.
pub(crate) fn open(
    store: &quipu::Store,
    config: &quipu::config::RelayConfig,
) -> Result<Arc<Relay>, String> {
    if let Some(reason) = Applier::new_replica_refusal(store).map_err(|e| e.to_string())? {
        return Err(reason);
    }
    if store.signing_identity().is_none() {
        return Err("a relay signs its children's frames, so it needs a signing key".into());
    }
    if config.upstream_key.len() != 64 || hex::decode(&config.upstream_key).is_err() {
        return Err("relay upstream_key must be an ed25519 public key in hex".into());
    }
    let upstream = Arc::new(HttpUpstream {
        url: config.upstream_url.clone(),
    });
    Relay::new(
        store,
        config.graphs.clone(),
        config.context_graphs.clone(),
        config.upstream_store_id.clone(),
        &config.upstream_key,
        upstream,
    )
    .map(Arc::new)
    .map_err(|e| e.to_string())
}

/// The relay `config.server.relay` asks for, opened, or `None`. Exits the
/// process when it cannot run: a relay that is also a replica, or one
/// [`open`] refuses.
pub(crate) fn open_configured(
    store: &quipu::Store,
    config: &quipu::QuipuConfig,
) -> Option<Arc<Relay>> {
    let relay_config = config.server.relay.as_ref()?;
    let fail = |reason: &str| -> ! {
        eprintln!("error: cannot start the relay: {reason}");
        std::process::exit(1)
    };
    if config.server.replica.is_some() {
        fail("server.replica and server.relay are exclusive");
    }
    let relay = open(store, relay_config).unwrap_or_else(|reason| fail(&reason));
    eprintln!("relay: following {}", relay_config.upstream_url);
    Some(relay)
}

/// A write the relay refuses: it accepts writes only from its applier.
pub(crate) fn write_refusal(path: &str) -> axum::response::Response {
    use axum::response::IntoResponse as _;
    (
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
        axum::Json(serde_json::json!({
            "error": "relay: this store accepts writes only from its upstream stream",
            "reason": "relay", "endpoint": path,
        })),
    )
        .into_response()
}

fn random_nonce() -> Option<[u8; 16]> {
    use ring::rand::SecureRandom as _;
    let mut nonce = [0u8; 16];
    ring::rand::SystemRandom::new().fill(&mut nonce).ok()?;
    Some(nonce)
}

/// Follow the upstream forever: stream, apply, reconnect from the cursor.
pub(crate) fn start(store: SharedStore, relay: Arc<Relay>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            follow(&store, &relay).await;
            // A broken session leaves no proof standing.
            relay.applier.lock().end_session();
            tokio::time::sleep(RECONNECT).await;
        }
    })
}

/// One upstream session, until it ends or a frame says to stop.
async fn follow(store: &SharedStore, relay: &Arc<Relay>) {
    let since = relay.applier.lock().progress().map_or(0, |p| p.source_tx);
    let request = StreamRequest {
        graphs: relay.graphs.clone(),
        context_graphs: relay.context_graphs.clone(),
        since,
        expected_store_id: relay.upstream_store_id.clone(),
    };
    let Some(mut chunks) = relay.upstream.open(request).await else {
        return;
    };
    let mut events = SseParser::default();
    while let Some(chunk) = chunks.next().await {
        for (event, data) in events.push(&chunk) {
            let (writer, applying) = (store.clone(), relay.clone());
            let applied = crate::base::blocking(move || {
                let mut writer = writer.writer.lock();
                let mut applier = applying.applier.lock();
                let outcome = applier.apply_frame(&mut writer, &event, &data, applying.now_ms())?;
                let coverage = applier.coverage();
                Ok((outcome, coverage))
            })
            .await;
            let Ok((outcome, coverage)) = applied else {
                return;
            };
            match outcome {
                Outcome::ProofAccepted { t_send_ms } => {
                    if let Some(watermark) = coverage {
                        relay.proofs.send_replace(Some((t_send_ms, watermark)));
                    }
                }
                // Reconnect from the cursor; a stopped, revoked or replaced
                // source answers the same on every later frame.
                Outcome::Resume { .. }
                | Outcome::Rebootstrap { .. }
                | Outcome::Refused { .. }
                | Outcome::Revoked { .. }
                | Outcome::Stopped { .. } => return,
                _ => {}
            }
        }
    }
}

/// Splits an SSE body into `(event, data)` pairs as its chunks arrive.
#[derive(Default)]
pub(crate) struct SseParser {
    buffer: String,
}

impl SseParser {
    /// Add a chunk; every event it completes, in order.
    pub(crate) fn push(&mut self, chunk: &str) -> Vec<(String, String)> {
        self.buffer.push_str(chunk);
        let mut out = Vec::new();
        while let Some(end) = self.buffer.find("\n\n") {
            let block: String = self.buffer.drain(..end + 2).collect();
            let mut event = String::new();
            let mut data = String::new();
            for line in block.lines() {
                if let Some(v) = line.strip_prefix("event:") {
                    event = v.trim_start().to_string();
                } else if let Some(v) = line.strip_prefix("data:") {
                    if !data.is_empty() {
                        data.push('\n');
                    }
                    data.push_str(v.strip_prefix(' ').unwrap_or(v));
                }
            }
            if !event.is_empty() {
                out.push((event, data));
            }
        }
        out
    }
}

/// The upstream over HTTP: `POST {url}/replicate/stream` and
/// `POST {url}/replicate/challenge`.
pub(crate) struct HttpUpstream {
    pub(crate) url: String,
}

impl Upstream for HttpUpstream {
    fn open(
        &self,
        request: StreamRequest,
    ) -> BoxFuture<'static, Option<BoxStream<'static, String>>> {
        let url = format!("{}/replicate/stream", self.url.trim_end_matches('/'));
        let body = serde_json::json!({
            "graphs": request.graphs, "context_graphs": request.context_graphs,
            "since": request.since, "expected_store_id": request.expected_store_id,
        });
        Box::pin(async move {
            let (chunks, received) = tokio::sync::mpsc::channel::<String>(64);
            let (opened, ready) = tokio::sync::oneshot::channel::<bool>();
            // ureq is blocking: read the body on its own thread, never holding
            // a store connection across the network.
            std::thread::spawn(move || {
                use std::io::Read as _;
                let response = ureq::AgentBuilder::new()
                    .timeout_connect(Duration::from_secs(5))
                    .redirects(0)
                    .build()
                    .post(&url)
                    .set("X-Quipu-Client", "replica-relay")
                    .set("Content-Type", "application/json")
                    .send_string(&body.to_string());
                let Ok(response) = response else {
                    let _ = opened.send(false);
                    return;
                };
                let _ = opened.send(true);
                let mut reader = response.into_reader();
                let mut buffer = [0u8; 8192];
                let mut pending = Vec::new();
                while let Ok(n) = reader.read(&mut buffer) {
                    if n == 0 {
                        return;
                    }
                    pending.extend_from_slice(&buffer[..n]);
                    // Hand on only whole UTF-8; keep a split character.
                    let valid = match std::str::from_utf8(&pending) {
                        Ok(text) => text.len(),
                        Err(error) => error.valid_up_to(),
                    };
                    let text = String::from_utf8_lossy(&pending[..valid]).into_owned();
                    pending.drain(..valid);
                    if chunks.blocking_send(text).is_err() {
                        return;
                    }
                }
            });
            if !ready.await.unwrap_or(false) {
                return None;
            }
            let stream = futures::stream::unfold(received, |mut received| async move {
                received.recv().await.map(|chunk| (chunk, received))
            });
            Some(stream.boxed())
        })
    }

    fn challenge(&self, session: String, nonce: [u8; 16]) -> BoxFuture<'static, bool> {
        let url = format!("{}/replicate/challenge", self.url.trim_end_matches('/'));
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                ureq::AgentBuilder::new()
                    .timeout(Duration::from_secs(5))
                    .redirects(0)
                    .build()
                    .post(&url)
                    .set("X-Quipu-Client", "replica-relay")
                    .set("Content-Type", "application/json")
                    .send_string(
                        &serde_json::json!({"session": session, "nonce": hex::encode(nonce)})
                            .to_string(),
                    )
                    .is_ok_and(|r| r.status() == 202)
            })
            .await
            .unwrap_or(false)
        })
    }
}

#[cfg(test)]
#[path = "relay_tests.rs"]
mod tests;
