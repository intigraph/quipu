//! A relay end to end (shared-applier-api.md §6): master -> relay -> leaf,
//! each hop over the stream routes. The relay follows the master through the
//! shared applier and serves the leaf in the master's numbering, signed with
//! its own key; a leaf challenge is bounded only by a fresh upstream proof.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use axum::{extract::State, http::StatusCode};
use futures::StreamExt;
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use quipu::applier::{Applier, Outcome};
use quipu::replicate_source::StreamRequest;
use quipu::store::Store;
use ring::signature::Ed25519KeyPair;

use super::super::{ChallengeRequest, challenge, stream};
use super::{BOARD, Events, board, source, write};
use crate::relay::{Relay, Upstream};
use crate::{SharedStore, StoreHandle};

/// The master reached through its route handlers, in process.
struct InProcess {
    master: SharedStore,
    /// While false, challenges are accepted but never reach the master: the
    /// upstream answers nothing, as when its session is down.
    answering: Arc<AtomicBool>,
    opens: Arc<AtomicUsize>,
}

impl Upstream for InProcess {
    fn open(
        &self,
        request: StreamRequest,
    ) -> BoxFuture<'static, Option<BoxStream<'static, String>>> {
        let master = self.master.clone();
        self.opens.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            let response = stream(State(master), axum::Json(request)).await;
            if response.status() != StatusCode::OK {
                return None;
            }
            let body = response.into_body().into_data_stream();
            Some(
                body.take_while(|chunk| futures::future::ready(chunk.is_ok()))
                    .map(|chunk| String::from_utf8(chunk.unwrap().to_vec()).unwrap())
                    .boxed(),
            )
        })
    }

    fn challenge(&self, session: String, nonce: [u8; 16]) -> BoxFuture<'static, bool> {
        let answering = self.answering.load(Ordering::SeqCst);
        Box::pin(async move {
            if !answering {
                return true;
            }
            let response = challenge(axum::Json(ChallengeRequest {
                session,
                nonce: hex::encode(nonce),
            }))
            .await;
            response.status() == StatusCode::ACCEPTED
        })
    }
}

struct Tree {
    master: SharedStore,
    relay: SharedStore,
    relay_key: String,
    master_id: String,
    answering: Arc<AtomicBool>,
    opens: Arc<AtomicUsize>,
    _follow: tokio::task::JoinHandle<()>,
}

impl Tree {
    fn new() -> Self {
        let (master, master_key) = source(true);
        Self::build(master, &master_key, None)
    }

    /// The relay reaches the master over real HTTP, as a deployment does.
    async fn over_http() -> Self {
        Self::over_http_authenticated(None).await
    }

    async fn over_http_authenticated(file: Option<std::path::PathBuf>) -> Self {
        let (master, master_key) = source(true);
        let app = axum::Router::new()
            .route("/replicate/stream", axum::routing::post(stream))
            .route("/replicate/challenge", axum::routing::post(challenge))
            .with_state(master.clone());
        let app = if file.is_some() {
            use axum::response::IntoResponse;
            let policy = quipu::http_auth::BearerPolicy::new(
                Some("private-relay-tree-fixture".into()),
                None,
                None,
                1,
            )
            .unwrap();
            app.layer(axum::middleware::from_fn(
                move |request: axum::extract::Request, next: axum::middleware::Next| {
                    let policy = policy.clone();
                    async move {
                        let result = quipu::http_auth::authorize_http_bearers(
                            request.uri().path(),
                            request.method().as_str(),
                            false,
                            &policy,
                            request
                                .headers()
                                .get("authorization")
                                .and_then(|v| v.to_str().ok()),
                            2,
                        );
                        if result.decision != quipu::http_auth::AccessDecision::Allow {
                            return StatusCode::UNAUTHORIZED.into_response();
                        }
                        next.run(request).await
                    }
                },
            ))
        } else {
            app
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self::build(
            master,
            &master_key,
            Some(crate::relay::HttpUpstream::new(url, file).unwrap()),
        )
    }

    fn build(
        master: SharedStore,
        master_key: &str,
        http: Option<crate::relay::HttpUpstream>,
    ) -> Self {
        let master_id = master.read().store_id().unwrap();
        let mut store = Store::open_in_memory().unwrap();
        let doc = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
        let identity = quipu::signing::SigningIdentity::new(
            Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap(),
            "quipu",
        );
        let relay_key = identity.public_key_hex();
        store.set_signing_identity(Arc::new(identity));
        let answering = Arc::new(AtomicBool::new(true));
        let opens = Arc::new(AtomicUsize::new(0));
        let upstream: Arc<dyn Upstream> = match http {
            Some(upstream) => Arc::new(upstream),
            None => Arc::new(InProcess {
                master: master.clone(),
                answering: answering.clone(),
                opens: opens.clone(),
            }),
        };
        let relay = Arc::new(
            Relay::new(
                &store,
                vec![BOARD.to_string()],
                Vec::new(),
                master_id.clone(),
                master_key,
                upstream,
            )
            .unwrap(),
        );
        let mut handle = StoreHandle::writer_only(store);
        handle.relay = Some(relay.clone());
        let shared: SharedStore = Arc::new(handle);
        let follow = crate::relay::start(shared.clone(), relay);
        Self {
            master,
            relay: shared,
            relay_key,
            master_id,
            answering,
            opens,
            _follow: follow,
        }
    }

    fn request(&self, graphs: &[&str]) -> StreamRequest {
        serde_json::from_value(serde_json::json!({
            "graphs": graphs, "since": 0, "expected_store_id": self.master_id,
        }))
        .unwrap()
    }

    async fn leaf(&self) -> Leaf {
        let response = stream(
            State(self.relay.clone()),
            axum::Json(self.request(&[BOARD])),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let store = Store::open_in_memory().unwrap();
        let mut applier = Applier::open(&store).unwrap();
        let scope = quipu::asof_share::scope_digest(&[BOARD.to_string()], &[]);
        // The leaf pins its RELAY's key, never the master's.
        applier.trust_stream(&self.relay_key, &scope);
        Leaf {
            store,
            applier,
            events: Events {
                bytes: response.into_body().into_data_stream(),
                buffer: String::new(),
            },
            clock: Instant::now(),
        }
    }

    /// Wait until the relay holds verified upstream coverage.
    async fn relay_covered(&self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(relay) = &self.relay.relay
                && relay.freshness().coverage.is_some()
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the relay never reached coverage"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

struct Leaf {
    store: Store,
    applier: Applier,
    events: Events,
    clock: Instant,
}

impl Leaf {
    fn now(&self) -> u64 {
        u64::try_from(self.clock.elapsed().as_millis()).unwrap()
    }

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

    async fn covers(&mut self, head: i64) -> bool {
        self.until(
            Duration::from_secs(5),
            |o| matches!(o, Outcome::CoverageAdvanced { watermark } if *watermark == head),
        )
        .await
        .is_some()
    }

    /// Challenge the relay; the proof outcome, if one arrives within `within`.
    async fn challenge(&mut self, nonce: [u8; 16], within: Duration) -> Option<Outcome> {
        let session = self.applier.session().unwrap().to_string();
        let sent = self.now();
        self.applier.challenge(&session, nonce, sent).unwrap();
        let response = challenge(axum::Json(ChallengeRequest {
            session,
            nonce: hex::encode(nonce),
        }))
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        self.until(within, |o| matches!(o, Outcome::ProofAccepted { .. }))
            .await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_leaf_behind_a_relay_tracks_the_master_within_five_seconds() {
    let tree = Tree::new();
    tree.relay_covered().await;
    let mut leaf = tree.leaf().await;
    let head = tree.master.read().latest_tx_id().unwrap();
    assert!(
        leaf.covers(head).await,
        "the leaf never covered the master's head"
    );
    assert_eq!(board(&leaf.store), board(&tree.master.read()));

    // A new master commit crosses both hops within 5 s.
    let committed = Instant::now();
    write(&mut tree.master.lock(), "urn:item:3", "three");
    let head = tree.master.read().latest_tx_id().unwrap();
    assert!(
        leaf.covers(head).await,
        "the new commit did not reach the leaf"
    );
    assert!(committed.elapsed() < Duration::from_secs(5));
    assert_eq!(board(&leaf.store), board(&tree.master.read()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_leaf_challenge_is_bounded_only_by_a_fresh_upstream_proof() {
    let tree = Tree::new();
    tree.relay_covered().await;
    let mut leaf = tree.leaf().await;
    let head = tree.master.read().latest_tx_id().unwrap();
    assert!(leaf.covers(head).await);

    // With the upstream answering, the leaf's challenge is proven.
    let proof = leaf.challenge([0x11; 16], Duration::from_secs(5)).await;
    assert!(
        matches!(proof, Some(Outcome::ProofAccepted { .. })),
        "{proof:?}"
    );
    assert!(leaf.applier.freshness(leaf.now()).within(5_000));

    // The upstream stops answering. The relay still holds its earlier proof,
    // but that proof predates this challenge, so it bounds nothing.
    tree.answering.store(false, Ordering::SeqCst);
    let unanswered = leaf
        .challenge([0x22; 16], Duration::from_millis(1_500))
        .await;
    assert_eq!(unanswered, None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_relay_route_refuses_a_child_scope_outside_its_own() {
    let tree = Tree::new();
    write_other(&mut tree.master.lock());
    tree.relay_covered().await;
    // The relay HOLDS the other graph, but it is not in the relay's scope.
    write_other(&mut tree.relay.lock());
    let wider = tree.request(&[BOARD, OTHER]);
    let response = stream(State(tree.relay.clone()), axum::Json(wider)).await;
    assert!(response.status().is_client_error(), "{}", response.status());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn many_leaves_share_one_master_stream() {
    let tree = Tree::new();
    tree.relay_covered().await;
    let head = tree.master.read().latest_tx_id().unwrap();
    let mut leaves = Vec::new();
    for _ in 0..3 {
        leaves.push(tree.leaf().await);
    }
    for leaf in &mut leaves {
        assert!(leaf.covers(head).await);
        assert_eq!(board(&leaf.store), board(&tree.master.read()));
    }
    assert_eq!(tree.opens.load(Ordering::SeqCst), 1);
}

const OTHER: &str = "urn:graph:other";

fn write_other(store: &mut Store) {
    let datum = quipu::store::Datum {
        entity: store.intern("urn:item:o").unwrap(),
        attribute: store.intern("urn:p:title").unwrap(),
        value: quipu::types::Value::Str("other".into()),
        valid_from: super::T.into(),
        valid_to: None,
        op: quipu::types::Op::Assert,
    };
    let g = store.graph_create(OTHER).unwrap();
    store
        .transact_to_graph(&[datum], super::T, None, None, g)
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_relay_follows_its_upstream_over_http() {
    let tree = Tree::over_http().await;
    tree.relay_covered().await;
    let mut leaf = tree.leaf().await;
    let head = tree.master.read().latest_tx_id().unwrap();
    assert!(leaf.covers(head).await);
    write(&mut tree.master.lock(), "urn:item:3", "three");
    let head = tree.master.read().latest_tx_id().unwrap();
    assert!(
        leaf.covers(head).await,
        "the commit did not cross the HTTP hop"
    );
    assert_eq!(board(&leaf.store), board(&tree.master.read()));
    // The upstream challenge travels over HTTP too.
    let proof = leaf.challenge([0x33; 16], Duration::from_secs(5)).await;
    assert!(
        matches!(proof, Some(Outcome::ProofAccepted { .. })),
        "{proof:?}"
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn protected_http_source_replicates_and_proves_freshness_through_relay() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("credential");
    std::fs::write(&file, "private-relay-tree-fixture\n").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    let tree = Tree::over_http_authenticated(Some(file)).await;
    tree.relay_covered().await;
    let mut leaf = tree.leaf().await;
    let head = tree.master.read().latest_tx_id().unwrap();
    assert!(leaf.covers(head).await);
    write(
        &mut tree.master.lock(),
        "urn:item:authenticated",
        "authenticated",
    );
    let head = tree.master.read().latest_tx_id().unwrap();
    assert!(leaf.covers(head).await);
    assert_eq!(board(&leaf.store), board(&tree.master.read()));
    let proof = leaf.challenge([0x47; 16], Duration::from_secs(5)).await;
    assert!(
        matches!(proof, Some(Outcome::ProofAccepted { .. })),
        "{proof:?}"
    );
}
