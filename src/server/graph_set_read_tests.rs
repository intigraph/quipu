//! aegis-n0wc.5.2: authenticated graph-set reads at `POST /graph-sets/query`,
//! including reads at a routed `/update` receipt's composition cut.
use super::*;
use crate::{SharedStore, StoreHandle};
use axum::{body::Body, http::Request};
use quipu::{
    Store,
    graph_catalog::{Budget, GraphSet, GraphSetCatalog, Route as CatalogRoute},
    read_authorization::{
        ComposedReadReservation, OwnerReadPolicy, ReadGrant, profile::ScopedReadConfig,
    },
    store::graph_owner::GraphSetOwner,
};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{fs, path::Path};
use tower::ServiceExt as _;

const P: &str = "https://example.test/project/p";
const PROV: &str = "https://example.test/project/p#seeds-provenance";
const EPH: &str = "https://example.test/project/p#seeds-ephemeral";
const Q: &str = "https://example.test/project/q";
const LEG: &str = "https://example.test/legacy/l";
const BASE: &str = "BASE <http://localhost/update>\n";
const TOKEN: &[u8] = b"private-fixture-token";
const CREATE: &str = "INSERT DATA { \
    GRAPH <https://example.test/project/p> { <urn:i> <urn:rev> 1 ; <urn:title> \"t\" } \
    GRAPH <https://example.test/project/p#seeds-provenance> { <urn:w1> <urn:of> <urn:i> } }";

fn edit(from: u32, to: u32, write: &str) -> String {
    format!(
        "DELETE {{ GRAPH <{P}> {{ <urn:i> <urn:rev> ?r }} }} \
         INSERT {{ GRAPH <{P}> {{ <urn:i> <urn:rev> {to} }} GRAPH <{PROV}> {{ <{write}> <urn:of> <urn:i> }} }} \
         WHERE {{ GRAPH <{P}> {{ <urn:i> <urn:rev> ?r FILTER(?r = {from}) }} }}"
    )
}

struct Fx {
    dir: tempfile::TempDir,
    root: std::path::PathBuf,
    legacy: SharedStore,
    router: &'static GraphSetRouter,
    catalog: Arc<GraphSetCatalog>,
    admission: ComposedReadAdmission,
    files: Vec<PathBuf>,
    config: ReadConfig,
    policy: OwnerReadPolicy,
    grant: ReadGrant,
}

fn adopted(root: &Path, file: &str, graphs: &[&str]) -> Store {
    let path = root.join(file);
    {
        let store = Store::open(path.to_str().unwrap()).unwrap();
        for g in graphs {
            store.graph_create(g).unwrap();
        }
    }
    Store::adopt(path.to_str().unwrap()).unwrap();
    Store::open(path.to_str().unwrap()).unwrap()
}

fn route(graph: &str, file: &str, store: &Store) -> CatalogRoute {
    CatalogRoute {
        graph: graph.into(),
        file_id: file.into(),
        store_id: store.store_id().unwrap(),
        log_epoch: store.log_epoch().unwrap(),
        generation: 4,
        policy: "policy-v1".into(),
    }
}

/// Set A = {P, PROV, EPH} in tracker.db, set B = {Q} in other.db, LEG and
/// ROOT legacy. One read grant covers every graph, so selection refusals are
/// reached only after authorization.
fn fx() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let tracker = adopted(&root, "tracker.db", &[P, PROV, EPH]);
    let other = adopted(&root, "other.db", &[Q]);
    let legacy = Store::open(root.join("legacy.db").to_str().unwrap()).unwrap();
    for g in [P, PROV, EPH, Q, LEG] {
        legacy.graph_create(g).unwrap();
    }
    let (a, b) = (route(P, "tracker", &tracker), route(Q, "other", &other));
    let catalog = Arc::new(
        GraphSetCatalog::new(
            7,
            &legacy.store_id().unwrap(),
            vec![
                GraphSet {
                    owner: a,
                    graphs: vec![P.into(), PROV.into(), EPH.into()],
                },
                GraphSet {
                    owner: b,
                    graphs: vec![Q.into()],
                },
            ],
            Budget {
                max_owners: 2,
                queued_per_owner: 1,
                readers_per_owner: 1,
                max_queued: 2,
                max_connections: 8,
            },
            8,
        )
        .unwrap(),
    );
    for g in [P, PROV, EPH, Q] {
        legacy.fence_routed_graph(g, 7, 4).unwrap();
    }
    let admission = ComposedReadAdmission::for_graph_sets(
        catalog.clone(),
        1024,
        3,
        1,
        ComposedReadReservation {
            runtime_bytes: 16 << 20,
            view_bytes: 16 << 20,
            query_bytes: 256 << 20,
            max_response_bytes: 1 << 20,
        },
    )
    .unwrap();
    let owners = vec![
        GraphSetOwner::prepare(
            catalog.clone(),
            &root,
            P,
            tracker,
            "policy-v1",
            admission.allocation(),
        )
        .unwrap(),
        GraphSetOwner::prepare(
            catalog.clone(),
            &root,
            Q,
            other,
            "policy-v1",
            admission.allocation(),
        )
        .unwrap(),
    ];
    let router: &'static GraphSetRouter = Box::leak(Box::new(
        GraphSetRouter::admit(catalog.clone(), &legacy, owners).unwrap(),
    ));
    let files = ["tracker", "other", "legacy"]
        .iter()
        .map(|f| root.join(format!("{f}.db")))
        .collect();
    let registry = root.join("registry.json");
    fs::write(&registry, registry_body()).unwrap();
    let policy_path = root.join("policy.db");
    let mut policy = OwnerReadPolicy::open(policy_path.to_str().unwrap(), "urn:owner").unwrap();
    let now = quipu::time::epoch_secs();
    let grant = ReadGrant {
        version: 1,
        id: "urn:grant:sets".into(),
        principal: "urn:test:reads".into(),
        graphs: [quipu::schema::ROOT_GRAPH_IRI, P, PROV, EPH, Q, LEG]
            .map(String::from)
            .into(),
        operations: [ReadOperation::Query].into(),
        valid_from: now - 1,
        valid_until: now + 300,
        max_offline_seconds: 30,
        issuer: "urn:owner".into(),
        revision: 1,
        revoked: false,
    };
    policy
        .apply(&grant, "graph-set read fixture grant")
        .unwrap();
    let config = ReadConfig {
        policy: ScopedReadConfig {
            policy_path: policy_path.to_str().unwrap().into(),
            owner: "urn:owner".into(),
        },
        registry: registry.to_str().unwrap().into(),
        timeout: Duration::from_secs(10),
    };
    Fx {
        dir,
        root,
        legacy: Arc::new(StoreHandle::writer_only(legacy)),
        router,
        catalog,
        admission,
        files,
        config,
        policy,
        grant,
    }
}

fn registry_body() -> String {
    json!({"version":1,"credentials":[{
        "credential_id":"reads-key", "principal":"urn:test:reads", "audience":"quipu",
        "token_sha256":hex::encode(Sha256::digest(TOKEN))
    }]})
    .to_string()
}

fn evidence(class: &str) -> quipu::transaction_auth::Identity {
    quipu::transaction_auth::Identity {
        principal: "urn:test:reads".into(),
        credential_id: Some("reads-key".into()),
        auth_class: class.into(),
        attestation: Default::default(),
    }
}

impl Fx {
    fn app(&self) -> Router {
        routes(
            self.router,
            self.files.clone(),
            self.config.clone(),
            self.admission.clone(),
        )
        .unwrap()
    }

    fn write(&self, update: &str) -> Value {
        crate::update::update_route::routed_update_for_test(
            self.router,
            &self.legacy,
            &format!("{BASE}{update}"),
            "seeds",
        )
    }
}

fn request(uri: &str, input: &Value) -> Request<Body> {
    Request::post(uri)
        .header("content-type", "application/json")
        .header(
            "authorization",
            format!("Bearer {}", std::str::from_utf8(TOKEN).unwrap()),
        )
        .body(Body::from(input.to_string()))
        .unwrap()
}

/// One request as an identified named bearer; returns status, headers, body.
async fn call(
    app: &Router,
    class: Option<&str>,
    req: Request<Body>,
) -> (StatusCode, HeaderMap, Value) {
    let response = crate::auth::run_identified(class.map(evidence), app.clone().oneshot(req))
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    (
        status,
        headers,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn read(app: &Router, input: Value) -> (StatusCode, HeaderMap, Value) {
    call(
        app,
        Some("named_bearer"),
        request("/graph-sets/query", &input),
    )
    .await
}

fn ask_title() -> Value {
    json!({"graphs":[P, PROV],
        "query":format!("ASK {{ GRAPH <{P}> {{ <urn:i> <urn:title> \"t\" }} }}")})
}

fn with_cut(mut input: Value, token: &str) -> Value {
    input["cut"] = json!(token);
    input
}

/// T4: a routed writer reads its own write at its receipt's cut, and the
/// answer names the cut it was read at.
#[tokio::test]
async fn t4_a_routed_writer_reads_its_own_write_at_its_receipt_cut() {
    let f = fx();
    let app = f.app();
    let receipt = f.write(CREATE);
    let token = receipt["owner"]["cut"]
        .as_str()
        .expect("receipt cut")
        .to_owned();
    let (status, headers, body) = read(&app, with_cut(ask_title(), &token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["boolean"], true);
    assert_eq!(headers[CUT_HEADER], token.as_str());
    assert_eq!(headers["cache-control"], "private, no-store");
    // Without a cut it answers at head and still names the cut it read.
    let (status, headers, _) = read(&app, ask_title()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[CUT_HEADER], token.as_str());
}

/// T5: after a second write the first receipt's cut is behind head: 409
/// naming both, never an answer at head presented as the old cut.
#[tokio::test]
async fn t5_a_cut_behind_head_refuses_409_naming_requested_and_head() {
    let f = fx();
    let app = f.app();
    let first = f.write(CREATE)["owner"]["cut"].as_str().unwrap().to_owned();
    let second = f.write(&edit(1, 2, "urn:w2"))["owner"]["cut"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(first, second);
    let (status, headers, body) = read(&app, with_cut(ask_title(), &first)).await;
    assert_eq!(headers["cache-control"], "private, no-store");
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["kind"], "behind-head");
    assert_eq!(body["requested"], first.as_str());
    assert_eq!(body["head"], second.as_str());
    let (status, _, _) = read(&app, with_cut(ask_title(), &second)).await;
    assert_eq!(status, StatusCode::OK);
}

/// T6: a cut naming another file refuses with admit's kind; a malformed
/// token is a 400.
#[tokio::test]
async fn t6_foreign_and_malformed_cuts_refuse_with_their_kind() {
    let f = fx();
    let app = f.app();
    let token = f.write(CREATE)["owner"]["cut"].as_str().unwrap().to_owned();
    let cut = CompositionCut::from_token(&token).unwrap();
    let mut value = serde_json::to_value(&cut).unwrap();
    value["members"][P]["store_id"] = json!("another-file");
    let foreign: CompositionCut = serde_json::from_value(value).unwrap();
    let (status, _, body) = read(&app, with_cut(ask_title(), &foreign.to_token())).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["kind"], "foreign-store");
    let (status, _, body) = read(&app, with_cut(ask_title(), "qcut1.not-base64!")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["kind"], "malformed");
    // A cut over a different selection than the request's graphs refuses.
    let one = json!({"graphs":[P], "query":"ASK {}"});
    let (status, _, body) = read(&app, with_cut(one, &token)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["kind"], "extra-member");
}

/// T7: ROOT, a legacy member, or two owners is not one routed owner: 409
/// after authorization, nothing read, no legacy fallback.
#[tokio::test]
async fn t7_a_selection_outside_one_routed_owner_refuses_409() {
    let f = fx();
    let app = f.app();
    f.write(CREATE);
    for graphs in [
        json!([quipu::schema::ROOT_GRAPH_IRI]),
        json!([LEG]),
        json!([P, Q]),
        json!([P, LEG]),
    ] {
        let (status, _, body) = read(&app, json!({"graphs":graphs, "query":"ASK {}"})).await;
        assert_eq!(status, StatusCode::CONFLICT, "{graphs}: {body}");
        assert_eq!(body["error"], "graph-set read spans files");
    }
}

/// T3: every authentication or authorization failure is the same 403 body,
/// including when the request carries a cut (S6: no head leaks before auth).
#[tokio::test]
async fn t3_auth_refusals_are_one_403_and_precede_any_cut() {
    let f = fx();
    let app = f.app();
    let token = f.write(CREATE)["owner"]["cut"].as_str().unwrap().to_owned();
    let behind = {
        f.write(&edit(1, 2, "urn:w2"));
        with_cut(ask_title(), &token)
    };
    let denied = json!({"error":"composed read denied"});
    let mut cases: Vec<(&str, Option<&str>, Request<Body>)> = vec![
        ("no identity", None, request("/graph-sets/query", &behind)),
        (
            "not a named bearer",
            Some("shared_bearer"),
            request("/graph-sets/query", &behind),
        ),
        (
            "uri query",
            Some("named_bearer"),
            request("/graph-sets/query?x=1", &behind),
        ),
        (
            "empty graphs",
            Some("named_bearer"),
            request("/graph-sets/query", &json!({"graphs":[],"query":"ASK {}"})),
        ),
        (
            "duplicate graphs",
            Some("named_bearer"),
            request(
                "/graph-sets/query",
                &json!({"graphs":[P,P],"query":"ASK {}"}),
            ),
        ),
        (
            "ungranted graph",
            Some("named_bearer"),
            request(
                "/graph-sets/query",
                &json!({"graphs":["urn:excluded"],"query":"ASK {}"}),
            ),
        ),
        (
            "oversize cut",
            Some("named_bearer"),
            request(
                "/graph-sets/query",
                &with_cut(ask_title(), &"x".repeat(MAX_CUT_BYTES + 1)),
            ),
        ),
    ];
    let mut two = request("/graph-sets/query", &behind);
    two.headers_mut().append(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer second"),
    );
    cases.push(("two authorization headers", Some("named_bearer"), two));
    for (name, class, req) in cases {
        let (status, _, body) = call(&app, class, req).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{name}: {body}");
        assert_eq!(body, denied, "{name}: one body, no oracle");
    }
    // A credential removed from the registry denies, cut or not.
    fs::write(&f.config.registry, r#"{"version":1,"credentials":[]}"#).unwrap();
    let (status, _, body) = read(&app, behind.clone()).await;
    assert_eq!((status, body), (StatusCode::FORBIDDEN, denied.clone()));
    fs::write(&f.config.registry, registry_body()).unwrap();
    // A revoked grant denies, cut or not.
    let mut policy = f.policy;
    let mut revoked = f.grant;
    revoked.revision = 2;
    revoked.revoked = true;
    policy.apply(&revoked, "revoke fixture grant").unwrap();
    let (status, _, body) = read(&app, behind).await;
    assert_eq!((status, body), (StatusCode::FORBIDDEN, denied));
}

/// T2: an unusable read authority refuses the mount; nothing is created.
#[tokio::test]
async fn t2_an_unusable_read_authority_refuses_to_mount() {
    let f = fx();
    let mount = |config: ReadConfig| {
        routes(f.router, f.files.clone(), config, f.admission.clone()).is_err()
    };
    let mut absent = f.config.clone();
    let absent_path = f.root.join("absent-policy.db");
    absent.policy.policy_path = absent_path.to_str().unwrap().into();
    assert!(mount(absent), "absent policy file");
    assert!(
        !absent_path.exists(),
        "the mount never creates a policy file"
    );
    for data in ["tracker", "other", "legacy"] {
        let mut shared = f.config.clone();
        shared.policy.policy_path = f.root.join(format!("{data}.db")).to_str().unwrap().into();
        assert!(mount(shared), "policy on data file {data}");
    }
    let mut no_registry = f.config.clone();
    no_registry.registry = f.root.join("absent.json").to_str().unwrap().into();
    assert!(mount(no_registry), "absent registry");
    for secs in [0, 31] {
        let mut timeout = f.config.clone();
        timeout.timeout = Duration::from_secs(secs);
        assert!(mount(timeout), "timeout {secs}s");
    }
    let reads = quipu::config::PilotReadsConfig {
        policy_file: f.config.policy.policy_path.clone().into(),
        owner: "urn:owner".into(),
        registry_file: f.config.registry.clone().into(),
        timeout_secs: 10,
        concurrency: 0,
        runtime_mib: 16,
        view_mib: 16,
        query_mib: 256,
        max_response_mib: 1,
    };
    assert!(
        crate::graph_sets_startup::read_admission(&f.catalog, 1024, &reads).is_err(),
        "startup must reject zero read scratch concurrency"
    );
    assert!(!mount(f.config.clone()), "the fixture authority mounts");
    drop(f.dir);
}

/// S1 (no policy file): a policy file removed after mount never answers; the
/// per-request separation check makes the read UNKNOWN.
#[tokio::test]
async fn s1_a_policy_file_removed_after_mount_never_answers() {
    let f = fx();
    let app = f.app();
    f.write(CREATE);
    assert_eq!(read(&app, ask_title()).await.0, StatusCode::OK);
    let policy = PathBuf::from(&f.config.policy.policy_path);
    fs::rename(&policy, f.root.join("moved-policy.db")).unwrap();
    let (status, _, body) = read(&app, ask_title()).await;
    assert_ne!(status, StatusCode::OK, "{body}");
    assert!(
        !policy.exists(),
        "a request never recreates the policy file"
    );
}

/// S2: a policy path that now names an owner data file never answers.
#[tokio::test]
async fn s2_a_policy_path_swapped_onto_a_data_inode_never_answers() {
    let f = fx();
    let app = f.app();
    f.write(CREATE);
    let policy = PathBuf::from(&f.config.policy.policy_path);
    fs::rename(&policy, f.root.join("moved-policy.db")).unwrap();
    fs::hard_link(f.root.join("tracker.db"), &policy).unwrap();
    let (status, _, body) = read(&app, ask_title()).await;
    assert_ne!(status, StatusCode::OK, "{body}");
}

/// S5 (time-of-check/time-of-use): authority that changes AFTER the cut
/// opened and the query ran, but BEFORE the recheck, refuses the read. The
/// barrier runs at exactly that point, synchronously; no sleeps. The positive
/// control is the same barrier changing nothing.
#[tokio::test]
async fn s5_authority_changed_between_open_and_recheck_refuses_the_read() {
    let f = fx();
    f.write(CREATE);
    let barrier_app = |barrier: Arc<dyn Fn() + Send + Sync>| {
        routes_with_recheck_barrier(
            f.router,
            f.files.clone(),
            f.config.clone(),
            f.admission.clone(),
            barrier,
        )
        .unwrap()
    };
    let ran = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    // Positive control: the barrier runs and changes nothing -> 200.
    let counted = ran.clone();
    let control = barrier_app(Arc::new(move || {
        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }));
    let (status, _, body) = read(&control, ask_title()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["boolean"], true);
    assert_eq!(
        ran.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "barrier ran"
    );
    // Registry credential removed at the barrier -> refused, no answer body.
    let registry = f.config.registry.clone();
    let counted = ran.clone();
    let unregistered = barrier_app(Arc::new(move || {
        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        fs::write(&registry, r#"{"version":1,"credentials":[]}"#).unwrap();
    }));
    let (status, _, body) = read(&unregistered, ask_title()).await;
    assert_eq!(
        ran.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "barrier ran"
    );
    assert_ne!(status, StatusCode::OK, "{body}");
    assert!(body.get("boolean").is_none(), "no answer escaped: {body}");
    fs::write(&f.config.registry, registry_body()).unwrap();
    assert_eq!(
        read(&control, ask_title()).await.0,
        StatusCode::OK,
        "restored"
    );
    // Grant revoked at the barrier -> refused, no answer body.
    let path = f.config.policy.policy_path.clone();
    let revoked = std::sync::Mutex::new(Some(f.grant));
    let counted = ran.clone();
    let revoking = barrier_app(Arc::new(move || {
        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if let Some(mut grant) = revoked.lock().unwrap().take() {
            grant.revision = 2;
            grant.revoked = true;
            OwnerReadPolicy::open(&path, "urn:owner")
                .unwrap()
                .apply(&grant, "revoke at the recheck barrier")
                .unwrap();
        }
    }));
    let (status, _, body) = read(&revoking, ask_title()).await;
    assert_eq!(
        ran.load(std::sync::atomic::Ordering::SeqCst),
        4,
        "barrier ran"
    );
    assert_ne!(status, StatusCode::OK, "{body}");
    assert!(body.get("boolean").is_none(), "no answer escaped: {body}");
}

impl Fx {
    /// The app with a barrier that revokes the fixture grant at the first
    /// recheck, counting barrier runs.
    fn revoking_app(&self, ran: Arc<std::sync::atomic::AtomicUsize>) -> Router {
        let path = self.config.policy.policy_path.clone();
        let grant = std::sync::Mutex::new(Some(self.grant.clone()));
        routes_with_recheck_barrier(
            self.router,
            self.files.clone(),
            self.config.clone(),
            self.admission.clone(),
            Arc::new(move || {
                ran.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if let Some(mut grant) = grant.lock().unwrap().take() {
                    grant.revision = 2;
                    grant.revoked = true;
                    OwnerReadPolicy::open(&path, "urn:owner")
                        .unwrap()
                        .apply(&grant, "revoke before the answer")
                        .unwrap();
                }
            }),
        )
        .unwrap()
    }
}

/// S7 (#1375 security review): every non-answer outcome (Spans, BadCut,
/// CutRefused, Behind) rechecks authority before it is sent. Revoked between
/// admit and answer, each is the one 403 body: no cut, head, kind or graphs.
/// The control is each case's own unrevoked answer.
#[tokio::test]
async fn s7_a_caller_revoked_before_a_refusal_learns_no_cut_or_head() {
    let denied = json!({"error":"composed read denied"});
    type Case = fn(&Fx) -> (Value, StatusCode, &'static str);
    let cases: [(&str, Case); 4] = [
        ("spans", |_| {
            (
                json!({"graphs":[P, Q], "query":"ASK {}"}),
                StatusCode::CONFLICT,
                "graph-set read spans files",
            )
        }),
        ("bad cut", |_| {
            (
                with_cut(ask_title(), "qcut1.not-base64!"),
                StatusCode::BAD_REQUEST,
                "composition cut refused",
            )
        }),
        ("cut refused", |f| {
            let token = f.write(&edit(1, 2, "urn:w2"))["owner"]["cut"]
                .as_str()
                .unwrap()
                .to_owned();
            (
                with_cut(json!({"graphs":[P], "query":"ASK {}"}), &token),
                StatusCode::CONFLICT,
                "composition cut refused",
            )
        }),
        ("behind", |f| {
            let first = f.write(&edit(1, 2, "urn:w2"))["owner"]["cut"]
                .as_str()
                .unwrap()
                .to_owned();
            f.write(&edit(2, 3, "urn:w3"));
            (
                with_cut(ask_title(), &first),
                StatusCode::CONFLICT,
                "historical cut needs vector mode (not implemented)",
            )
        }),
    ];
    for (name, case) in cases {
        let f = fx();
        f.write(CREATE);
        let (input, refused, error) = case(&f);
        let (status, headers, body) = read(&f.app(), input.clone()).await;
        assert_eq!(
            (status, body["error"].as_str()),
            (refused, Some(error)),
            "{name} control: {body}"
        );
        assert!(headers.get(CUT_HEADER).is_none(), "{name}");
        let ran = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (status, headers, body) = read(&f.revoking_app(ran.clone()), input).await;
        assert_eq!(
            ran.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "{name}: the recheck ran before the refusal"
        );
        assert_eq!(status, StatusCode::FORBIDDEN, "{name}: {body}");
        assert_eq!(body, denied, "{name}: no cut, head, kind or graphs");
        assert!(headers.get(CUT_HEADER).is_none(), "{name}");
    }
}

/// S5 tightened: a grant revoked at the answer's recheck is the 403 denial,
/// not a 503 UNKNOWN (#1375 review nit: the closure no longer collapses a
/// policy decision into "unavailable").
#[tokio::test]
async fn s5b_a_revoked_answer_is_the_403_denial() {
    let f = fx();
    f.write(CREATE);
    let ran = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (status, headers, body) = read(&f.revoking_app(ran.clone()), ask_title()).await;
    assert_eq!(ran.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        (status, body),
        (
            StatusCode::FORBIDDEN,
            json!({"error":"composed read denied"})
        )
    );
    assert!(headers.get(CUT_HEADER).is_none());
}

/// T3b (#1375 review finding 3): no Authorization header at all, a query
/// over 64 KiB, and a body over the 128 KiB limit all refuse with no answer.
#[tokio::test]
async fn t3b_missing_authorization_and_oversize_requests_refuse() {
    let f = fx();
    let app = f.app();
    f.write(CREATE);
    let denied = json!({"error":"composed read denied"});
    let mut bare = request("/graph-sets/query", &ask_title());
    bare.headers_mut().remove(header::AUTHORIZATION);
    let (status, _, body) = call(&app, Some("named_bearer"), bare).await;
    assert_eq!(
        (status, body),
        (StatusCode::FORBIDDEN, denied.clone()),
        "no header"
    );
    let long = json!({"graphs":[P], "query": format!("ASK {{}} #{}", "x".repeat(65_536))});
    assert!(long.to_string().len() < 131_072, "under the body limit");
    let (status, _, body) = read(&app, long).await;
    assert_eq!(
        (status, body),
        (StatusCode::FORBIDDEN, denied),
        "query over 64 KiB"
    );
    let huge = json!({"graphs":[P], "query": "x".repeat(131_072)});
    let (status, headers, body) = read(&app, huge).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    assert!(headers.get(CUT_HEADER).is_none() && body.get("boolean").is_none());
}

/// W1 (#1375 review finding 2): the production wiring. Merged by
/// `graph_sets_startup::mount_reads` and wrapped in the real `protected_app`, the route
/// needs a bearer and answers a named one; `None` mounts nothing.
#[tokio::test]
async fn w1_the_mounted_reads_sit_behind_the_real_auth_layer() {
    let f = fx();
    f.write(CREATE);
    let protected = |reads: Option<Router>| {
        let registry =
            quipu::crew_credentials::CredentialRegistry::parse(registry_body().as_bytes(), "quipu")
                .unwrap();
        let policy =
            quipu::http_auth::BearerPolicy::new(None, None, None, quipu::time::epoch_secs())
                .unwrap()
                .with_named(registry);
        crate::protected_app(
            crate::graph_sets_startup::mount_reads(axum::Router::new(), reads),
            &crate::process_pool::in_process(),
            None,
            f.legacy.clone(),
            crate::auth_guard::Config {
                auth_policy: policy,
                session_keys: Default::default(),
                attest_store: f.legacy.clone(),
                replica_config: None,
                read_only: false,
                is_relay: false,
                scoped_enabled: false,
            },
        )
        .with_state(f.legacy.clone())
    };
    let send = |app: Router, req: Request<Body>| async move {
        let response = app.oneshot(req).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null),
        )
    };
    let app = protected(Some(f.app()));
    let mut anonymous = request("/graph-sets/query", &ask_title());
    anonymous.headers_mut().remove(header::AUTHORIZATION);
    let (status, body) = send(app.clone(), anonymous).await;
    assert_ne!(status, StatusCode::OK, "{body}");
    assert!(body.get("boolean").is_none(), "no anonymous answer: {body}");
    let (status, body) = send(app, request("/graph-sets/query", &ask_title())).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the named bearer reaches the read: {body}"
    );
    assert_eq!(body["boolean"], true);
    let (status, _) = send(protected(None), request("/graph-sets/query", &ask_title())).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "absent reads mount nothing");
}
