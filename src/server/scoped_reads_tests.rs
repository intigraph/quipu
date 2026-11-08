//! Paired private fixtures for the actual scoped HTTP dispatch boundary.
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    response::Response,
};
use quipu::read_authorization::{
    OwnerReadPolicy, ReadGrant, ReadOperation, profile::ScopedReadConfig,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, sync::Arc};

struct Fixture {
    _dir: tempfile::TempDir,
    profile: ScopedReadConfig,
    registry: String,
    source: super::super::SharedStore,
    policy: OwnerReadPolicy,
}

impl Fixture {
    fn router(&self) -> axum::Router {
        self.router_with_shared(None)
    }

    /// The same real guard stack, optionally with the legacy shared writer
    /// bearer configured, as an operator might leave it on a scoped instance.
    fn router_with_shared(&self, shared: Option<&str>) -> axum::Router {
        let app = axum::Router::new()
            .route(
                "/query",
                axum::routing::post(|| async { "unscoped handler" }),
            )
            .route(
                "/graphs",
                axum::routing::get(|| async { "unscoped registry" }),
            )
            .fallback(|| async { StatusCode::IM_A_TEAPOT });
        let app = super::guard(
            app,
            Some((self.profile.clone(), self.registry.clone())),
            self.source.clone(),
        );
        let registry = quipu::crew_credentials::CredentialRegistry::parse(
            &std::fs::read(&self.registry).unwrap(),
            "quipu",
        )
        .unwrap();
        let policy = quipu::http_auth::BearerPolicy::new(
            shared.map(str::to_owned),
            None,
            None,
            quipu::time::epoch_secs(),
        )
        .unwrap()
        .with_named(registry);
        crate::auth_guard::layer(
            app,
            crate::auth_guard::Config {
                auth_policy: policy,
                session_keys: Default::default(),
                attest_store: self.source.clone(),
                replica_config: None,
                read_only: true,
                is_relay: false,
                scoped_enabled: true,
            },
        )
    }

    /// The production route table behind the production protection stack.
    /// `scoped: false` is the control: same routes, guard not configured.
    fn real_app(&self, scoped: bool) -> axum::Router {
        let registry = quipu::crew_credentials::CredentialRegistry::parse(
            &std::fs::read(&self.registry).unwrap(),
            "quipu",
        )
        .unwrap();
        let policy =
            quipu::http_auth::BearerPolicy::new(None, None, None, quipu::time::epoch_secs())
                .unwrap()
                .with_named(registry);
        crate::protected_app(
            crate::api_routes(Default::default()),
            &crate::process_pool::in_process(),
            scoped.then(|| (self.profile.clone(), self.registry.clone())),
            self.source.clone(),
            crate::auth_guard::Config {
                auth_policy: policy,
                session_keys: Default::default(),
                attest_store: self.source.clone(),
                replica_config: None,
                read_only: true,
                is_relay: false,
                scoped_enabled: scoped,
            },
        )
        .with_state(self.source.clone())
    }

    fn new(hidden: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("policy.db");
        let mut policy = OwnerReadPolicy::open(path.to_str().unwrap(), "urn:owner").unwrap();
        let now = quipu::time::epoch_secs();
        for (principal, graphs) in [("urn:p", vec!["urn:a", "urn:c"]), ("urn:q", vec!["urn:b"])] {
            for operation in [
                ReadOperation::Query,
                ReadOperation::Export,
                ReadOperation::Metadata,
            ] {
                policy
                    .apply(
                        &ReadGrant {
                            version: 1,
                            id: format!("urn:grant:{principal}:{operation:?}"),
                            principal: principal.into(),
                            graphs: graphs.iter().map(|s| (*s).into()).collect(),
                            operations: [operation].into(),
                            valid_from: now - 1,
                            valid_until: now + 3600,
                            max_offline_seconds: 300,
                            issuer: "urn:owner".into(),
                            revision: 1,
                            revoked: false,
                        },
                        "private HTTP fixture",
                    )
                    .unwrap();
            }
        }
        let credentials: Vec<_> = [("urn:p", "p"), ("urn:q", "q"), ("urn:unbound", "u")]
            .into_iter()
            .map(|(principal, token)| {
                json!({"principal":principal,"credential_id":token,"audience":"quipu",
                "token_sha256":format!("{:x}",Sha256::digest(token.as_bytes()))})
            })
            .collect();
        let registry = dir.path().join("credentials.json");
        std::fs::write(
            &registry,
            json!({"version":1,"credentials":credentials}).to_string(),
        )
        .unwrap();
        let mut source = quipu::Store::open_in_memory().unwrap();
        for (graph, value) in [
            ("urn:b", hidden),
            ("urn:a", "allowed"),
            ("urn:c", "context"),
        ] {
            let g = source.graph_create(graph).unwrap();
            let text = format!("<urn:entity:{value}> <urn:p> \"{value}\" .");
            quipu::rdf::ingest_rdf_to_graph(
                &mut source,
                text.as_bytes(),
                oxrdfio::RdfFormat::NTriples,
                None,
                "2026-01-01T00:00:00Z",
                None,
                None,
                g,
            )
            .unwrap();
        }
        Self {
            profile: ScopedReadConfig {
                policy_path: path.to_string_lossy().into(),
                owner: "urn:owner".into(),
            },
            registry: registry.to_string_lossy().into(),
            source: Arc::new(super::super::StoreHandle::writer_only(source)),
            policy,
            _dir: dir,
        }
    }

    async fn call(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        input: Value,
    ) -> (StatusCode, Vec<u8>) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("x-quipu-agent", "urn:p")
            .header("x-actor", "urn:p");
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let body = if input.is_null() {
            Body::empty()
        } else {
            Body::from(input.to_string())
        };
        let request = request.body(body).unwrap();
        // Existing named verification, then the same owned identity scope used
        // by run_named. The scoped handler independently re-verifies current
        // credential evidence before opening the source view.
        let bytes = std::fs::read(&self.registry).unwrap();
        let registry = quipu::crew_credentials::CredentialRegistry::parse(&bytes, "quipu").unwrap();
        let header = token.map(|token| format!("Bearer {token}"));
        let identity =
            registry
                .authenticate(header.as_deref())
                .map(|p| quipu::transaction_auth::Identity {
                    principal: p.iri,
                    credential_id: Some(p.credential_id),
                    auth_class: "named_bearer".into(),
                    attestation: Default::default(),
                });
        let response: Response = super::super::auth::run_identified(
            identity,
            super::handle(
                self.profile.clone(),
                self.registry.clone(),
                self.source.clone(),
                request,
            ),
        )
        .await;
        let status = response.status();
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap()
            .to_vec();
        (status, body)
    }
}

#[tokio::test]
async fn scoped_reads_two_stores_http_queries_exports_acquisition_and_metadata_match() {
    let left = Fixture::new("one");
    let right = Fixture::new("denied_other");
    for (method, path, input) in [
        (
            "POST",
            "/query",
            json!({"query":"SELECT ?g ?s ?v WHERE { GRAPH ?g { ?s <urn:p> ?v } } ORDER BY ?g ?s"}),
        ),
        ("POST", "/export", json!({})),
        ("POST", "/acquire", json!({})),
        ("GET", "/graphs", Value::Null),
    ] {
        let a = left.call(method, path, Some("p"), input.clone()).await;
        let b = right.call(method, path, Some("p"), input).await;
        assert_eq!(
            a.0,
            StatusCode::OK,
            "{path}: {}",
            String::from_utf8_lossy(&a.1)
        );
        assert_eq!(a, b, "{path}");
        assert!(!String::from_utf8_lossy(&a.1).contains("denied_other"));
    }
}

#[tokio::test]
async fn scoped_reads_identity_scope_method_alias_and_unknown_refusals_are_paired() {
    let fixture = Fixture::new("secret");
    for token in [None, Some("invalid"), Some("u")] {
        assert_eq!(
            fixture
                .call("POST", "/query", token, json!({"query":"ASK { ?s ?p ?o }"}))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    for path in ["/query", "/export", "/acquire"] {
        for graphs in [
            vec!["urn:b"],
            vec!["urn:a", "urn:b"],
            vec!["urn:missing"],
            vec![quipu::schema::ROOT_GRAPH_IRI],
        ] {
            let mut input = json!({"graphs":graphs});
            if path == "/query" {
                input["query"] = json!("ASK { ?s ?p ?o }");
            }
            let result = fixture.call("POST", path, Some("p"), input).await;
            assert_eq!(result.0, StatusCode::FORBIDDEN, "{path}");
            assert_eq!(
                serde_json::from_slice::<Value>(&result.1).unwrap(),
                json!({"error":"scoped read denied"})
            );
        }
    }
    for path in [
        "/ask",
        "/search",
        "/entity",
        "/stats",
        "/mcp",
        "/share/asof",
        "/replicate/stream",
        "/replicate/challenge",
        "/mapping/propose",
        "/mapping/decide",
        "/mapping/apply",
    ] {
        assert_eq!(
            fixture.call("POST", path, Some("p"), json!({})).await.0,
            StatusCode::FORBIDDEN,
            "{path}"
        );
    }
    // Follower readiness is owner metadata, not a principal-scoped read.
    assert_eq!(
        quipu::read_authorization::routes::classify("/replica/ready", "GET"),
        quipu::read_authorization::routes::RouteClass::OwnerOnly
    );
    let ready = fixture
        .call("GET", "/replica/ready", Some("p"), Value::Null)
        .await;
    assert_eq!(ready.0, StatusCode::FORBIDDEN);
    assert_eq!(
        serde_json::from_slice::<Value>(&ready.1).unwrap(),
        json!({"error":"scoped read denied"})
    );
    for method in ["GET", "HEAD", "PUT", "PATCH", "DELETE", "OPTIONS"] {
        assert_eq!(
            fixture
                .call(method, "/query", Some("p"), json!({"query":"ASK {}"}))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        fixture
            .call(
                "POST",
                "/query",
                Some("p"),
                json!({"query":"ASK {}","graph":"urn:b"})
            )
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        fixture
            .call(
                "POST",
                "/query?graph=urn:b",
                Some("p"),
                json!({"query":"ASK {}"})
            )
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        fixture.call("GET", "/health", None, Value::Null).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn scoped_reads_concurrent_owned_views_error_cleanup_and_current_revocation() {
    let mut fixture = Fixture::new("secret");
    let query = json!({"query":"SELECT ?v WHERE { ?s <urn:p> ?v } ORDER BY ?v"});
    let (p, q) = tokio::join!(
        fixture.call("POST", "/query", Some("p"), query.clone()),
        fixture.call("POST", "/query", Some("q"), query.clone())
    );
    assert_eq!(p.0, StatusCode::OK);
    assert_eq!(q.0, StatusCode::OK);
    assert!(!String::from_utf8_lossy(&p.1).contains("secret"));
    assert!(!String::from_utf8_lossy(&q.1).contains("allowed"));
    assert_eq!(
        fixture
            .call(
                "POST",
                "/query",
                Some("p"),
                json!({"query":"invalid query"})
            )
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        fixture
            .call("POST", "/query", Some("q"), query.clone())
            .await,
        q
    );
    let now = quipu::time::epoch_secs();
    fixture
        .policy
        .apply(
            &ReadGrant {
                version: 1,
                id: "urn:grant:urn:p:Query".into(),
                principal: "urn:p".into(),
                graphs: BTreeSet::from(["urn:a".into(), "urn:c".into()]),
                operations: [ReadOperation::Query].into(),
                valid_from: now - 1,
                valid_until: now + 3600,
                max_offline_seconds: 300,
                issuer: "urn:owner".into(),
                revision: 2,
                revoked: true,
            },
            "private revoke",
        )
        .unwrap();
    assert_eq!(
        fixture
            .call("POST", "/query", Some("p"), query.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(fixture.call("POST", "/query", Some("q"), query).await, q);
}

#[tokio::test]
async fn scoped_reads_real_guard_blocks_raw_handlers_and_fallback() {
    use tower::ServiceExt;
    let fixture = Fixture::new("secret");
    for (path, token, status) in [
        ("/query", Some("p"), StatusCode::OK),
        ("/query", None, StatusCode::FORBIDDEN),
        ("/new-unclassified", Some("p"), StatusCode::FORBIDDEN),
        ("/mcp", Some("p"), StatusCode::FORBIDDEN),
    ] {
        let mut req = Request::post(path).header("content-type", "application/json");
        if let Some(token) = token {
            req = req.header("authorization", format!("Bearer {token}"));
        }
        let response = fixture
            .router()
            .oneshot(
                req.body(Body::from(json!({"query":"ASK { ?s ?p ?o }"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status, "{path}");
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        assert!(!String::from_utf8_lossy(&body).contains("unscoped"));
    }
}

#[tokio::test]
async fn scoped_reads_cancelled_and_panicking_blocking_views_do_not_change_other_principals() {
    let fixture = Fixture::new("secret");
    let query = json!({"query":"SELECT ?v WHERE { ?s <urn:p> ?v } ORDER BY ?v"});
    let expected = fixture
        .call("POST", "/query", Some("q"), query.clone())
        .await;
    assert_eq!(expected.0, StatusCode::OK);
    for panic_after_release in [false, true] {
        let source = fixture.source.clone();
        let profile = fixture.profile.clone();
        let registry = fixture.registry.clone();
        let (ready, started) = tokio::sync::oneshot::channel();
        let (release, released) = std::sync::mpsc::channel();
        let (finished, done) = tokio::sync::oneshot::channel();
        let task = super::super::base::spawn_tracked(move || {
            let registry = quipu::crew_credentials::CredentialRegistry::parse(
                &std::fs::read(registry).unwrap(),
                "quipu",
            )
            .unwrap();
            let identity = quipu::read_authorization::ReadIdentity::authenticate_named(
                &registry,
                Some("Bearer p"),
            )
            .unwrap();
            let now = quipu::time::epoch_secs();
            let view = profile
                .open_policy()
                .unwrap()
                .snapshot()
                .unwrap()
                .authorize(&identity, ReadOperation::Query, None, now)
                .unwrap()
                .open(&source.read(), now)
                .unwrap();
            ready.send(()).unwrap();
            released
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                if panic_after_release {
                    panic!("private view unwind control");
                }
                let bytes = view
                    .query("SELECT ?v WHERE { ?s <urn:p> ?v }", true, now)
                    .unwrap();
                assert!(!String::from_utf8_lossy(&bytes).contains("secret"));
            }));
            finished.send(result.is_err()).unwrap();
            if let Err(panic) = result {
                std::panic::resume_unwind(panic);
            }
        });
        tokio::time::timeout(std::time::Duration::from_secs(10), started)
            .await
            .unwrap()
            .unwrap();
        // A running blocking task cannot be stopped by abort. Its owned P view
        // must remain bounded while Q executes, and must disappear on unwind.
        if !panic_after_release {
            task.abort();
        }
        let during = fixture
            .call("POST", "/query", Some("q"), query.clone())
            .await;
        release.send(()).unwrap();
        assert_eq!(done.await.unwrap(), panic_after_release);
        let joined = task.await;
        if panic_after_release {
            assert!(joined.unwrap_err().is_panic());
        } else {
            joined.unwrap();
        }
        assert_eq!(during, expected);
        assert_eq!(
            fixture
                .call("POST", "/query", Some("q"), query.clone())
                .await,
            expected
        );
    }
}

#[tokio::test]
async fn scoped_reads_actual_signed_adapter_binds_identity_and_refuses_replay_or_bad_signature() {
    use base64::Engine as _;
    use quipu::session_attestation::{
        AttestationEnvelope, SessionBinding, SignedBinding, WRITE_V2, WriteBinding, body_sha256,
        canonical_message,
    };
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use tower::ServiceExt;
    let fixture = Fixture::new("secret");
    let now = quipu::time::epoch_secs();
    let key = Ed25519KeyPair::from_seed_unchecked(&[92; 32]).unwrap();
    let mut binding = SessionBinding::new(
        "urn:p",
        "scoped-fixture",
        hex::encode(key.public_key().as_ref()),
        "urn:owner",
        now - 10,
        now + 120,
    )
    .unwrap();
    binding.allow_write = true;
    fixture
        .source
        .read()
        .attestation_register(&binding)
        .unwrap();
    let audience = fixture.source.read().store_id().unwrap();
    let body = json!({"query":"ASK { <urn:entity:allowed> <urn:p> \"allowed\" }"}).to_string();
    let signed = |nonce: &str, bad: bool| {
        let mut envelope = AttestationEnvelope {
            version: WRITE_V2.into(),
            key_id: binding.key_id.clone(),
            session: binding.session.clone(),
            introducer: binding.introducer.clone(),
            issued_at_epoch: now,
            nonce: nonce.into(),
            signature: String::new(),
            audience: Some(audience.clone()),
        };
        let hash = body_sha256(body.as_bytes());
        let message = canonical_message(
            &envelope,
            &SignedBinding::Write(WriteBinding {
                method: "POST",
                path: "/query",
                content_type: "application/json",
                body_sha256: &hash,
                audience: Some(&audience),
            }),
        );
        envelope.signature = hex::encode(key.sign(&message).as_ref());
        if bad {
            envelope.signature = "00".repeat(64);
        }
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&envelope).unwrap());
        Request::post("/query")
            .header("content-type", "application/json")
            .header(crate::auth::write_attest::HEADER, header)
            .header("authorization", "Bearer p")
            .body(Body::from(body.clone()))
            .unwrap()
    };
    let nonce = "0123456789abcdef0123456789abcdef";
    let response = fixture
        .router()
        .oneshot(signed(nonce, false))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let answer = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert!(
        serde_json::from_slice::<Value>(&answer).unwrap()["boolean"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(
        fixture
            .router()
            .oneshot(signed(nonce, false))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        fixture
            .router()
            .oneshot(signed("1123456789abcdef0123456789abcdef", true))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    fixture
        .source
        .read()
        .attestation_revoke(&binding.session)
        .unwrap();
    assert_eq!(
        fixture
            .router()
            .oneshot(signed("2123456789abcdef0123456789abcdef", false))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        fixture
            .call(
                "POST",
                "/query",
                Some("q"),
                json!({"query":"ASK { ?s ?p ?o }"})
            )
            .await
            .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn scoped_reads_unavailable_policy_is_unknown_and_credentials_revoke_without_fallback() {
    let mut fixture = Fixture::new("secret");
    let query = json!({"query":"ASK { ?s ?p ?o }"});
    let original_path = fixture.profile.policy_path.clone();
    // A malformed owner authority must not reuse a cached grant or open reads.
    fixture.profile.policy_path = fixture
        ._dir
        .path()
        .join("missing.db")
        .to_string_lossy()
        .into();
    let response = fixture
        .call("POST", "/query", Some("p"), query.clone())
        .await;
    assert_eq!(response.0, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        serde_json::from_slice::<Value>(&response.1).unwrap()["outcome"],
        "UNKNOWN"
    );
    fixture.profile.policy_path = original_path;
    std::fs::write(
        &fixture.registry,
        json!({"version":1,"credentials":[]}).to_string(),
    )
    .unwrap();
    assert_eq!(
        fixture.call("POST", "/query", Some("p"), query).await.0,
        StatusCode::FORBIDDEN
    );
}

/// End-to-end regression guard: with the legacy shared writer bearer
/// configured, a request presenting it while copying a granted principal's
/// agent/actor headers is refused, and a named reader on the same instance is
/// still served.
///
/// Two independent defenses hold here, so disabling either alone does not
/// change this outcome (measured with a temporary mutant that accepted the
/// shared class: still refused). The shared bearer always authenticates as
/// the fixed non-IRI principal `legacy-shared-bearer`, never a header value,
/// so no read grant can name it. The class refusal itself is proven at unit
/// level by `invalid_identity_no_grant_expiry_and_operation_intersection_deny`.
#[tokio::test]
async fn scoped_reads_refuse_the_legacy_shared_bearer_over_http() {
    use tower::ServiceExt;
    const SHARED: &str = "fixture-shared-writer-bearer";
    let fixture = Fixture::new("secret");
    for (token, status) in [(SHARED, StatusCode::FORBIDDEN), ("p", StatusCode::OK)] {
        for path in ["/query", "/graphs"] {
            let request = if path == "/query" {
                Request::post(path)
                    .header("content-type", "application/json")
                    // Copied caller claims naming a principal that holds a grant.
                    .header("x-quipu-agent", "urn:p")
                    .header("x-actor", "urn:p")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::from(json!({"query":"ASK { ?s ?p ?o }"}).to_string()))
            } else {
                Request::get(path)
                    .header("x-quipu-agent", "urn:p")
                    .header("x-actor", "urn:p")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
            }
            .unwrap();
            let response = fixture
                .router_with_shared(Some(SHARED))
                .oneshot(request)
                .await
                .unwrap();
            assert_eq!(response.status(), status, "{path} with {token}");
            let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
            let body = String::from_utf8_lossy(&body);
            assert!(!body.contains("unscoped"), "{path}: raw handler reached");
            assert!(!body.contains("secret"), "{path}: hidden data disclosed");
        }
    }
}

async fn send(app: &axum::Router, method: &str, path: &str, body: Body) -> (StatusCode, String) {
    use tower::ServiceExt;
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", "Bearer p")
        .header("content-type", "application/json")
        .body(body)
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&body).into_owned())
}

/// G2 part 3: the guard tests above use a two-route fixture. This drives every
/// census path and method through `api_routes` + `protected_app`, the exact
/// assembly `main` serves, so a route registered outside the guard or a
/// reordered stack is caught here rather than in production.
#[tokio::test]
async fn scoped_reads_guard_covers_the_real_assembled_route_table() {
    use quipu::read_authorization::routes::{RouteClass, census, classify};
    let fixture = Fixture::new("secret");

    // Control: the same real routes without a scoped profile serve the real
    // handlers, which list the graph the principal holds no grant for.
    let open = fixture.real_app(false);
    let (status, body) = send(&open, "GET", "/graphs", Body::empty()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains("urn:b"),
        "control must reach the real handler: {body}"
    );

    let app = fixture.real_app(true);
    let mut probed = 0;
    for template in census()
        .iter()
        .copied()
        .chain(["/mcp", "/mcp/", "/new-unclassified"])
    {
        let path = template.replace("{iri}", "urn%3Aentity%3Asecret");
        for method in ["GET", "HEAD", "POST", "PUT", "DELETE", "PATCH"] {
            let class = classify(&path, method);
            let input = match class {
                RouteClass::Scoped(ReadOperation::Query) => {
                    json!({"query":"SELECT ?g ?s ?v WHERE { GRAPH ?g { ?s <urn:p> ?v } }"})
                        .to_string()
                }
                RouteClass::Scoped(ReadOperation::Metadata) => String::new(),
                _ => "{}".to_owned(),
            };
            let (status, body) = send(&app, method, &path, Body::from(input)).await;
            probed += 1;
            assert!(
                !body.contains("secret") && !body.contains("urn:b"),
                "{method} {path} exposed denied data: {body}"
            );
            match class {
                RouteClass::PublicStatic => {
                    assert_eq!(status, StatusCode::OK, "{method} {path}");
                    if method == "GET" {
                        let value: Value = serde_json::from_str(&body).unwrap();
                        assert_eq!(
                            value,
                            json!({"service":"quipu","profile":"scoped-current/v1"}),
                            "{path} must be the guard's static answer, not the real handler"
                        );
                    }
                }
                RouteClass::Scoped(_) => {
                    assert_eq!(status, StatusCode::OK, "{method} {path}: {body}");
                }
                _ => {
                    assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}: {body}");
                    if method == "HEAD" {
                        // HEAD responses carry no body; the status is the evidence.
                        continue;
                    }
                    let value: Value = serde_json::from_str(&body).unwrap();
                    // Authentication is outer: on this read-only scoped
                    // instance it refuses write routes before the scoped
                    // guard. Either refusal means no handler ran.
                    let read_only_write =
                        class == RouteClass::Write && value["reason"] == "server_is_read_only";
                    assert!(
                        read_only_write || value == json!({"error":"scoped read denied"}),
                        "{method} {path}: refused by something other than auth or the scoped guard: {value}"
                    );
                }
            }
        }
    }
    assert!(probed > 6 * 100, "census unexpectedly small: {probed}");
    let (_, graphs) = send(&app, "GET", "/graphs", Body::empty()).await;
    assert!(
        graphs.contains("urn:a") && graphs.contains("urn:c"),
        "{graphs}"
    );
}
