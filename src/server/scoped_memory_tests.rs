//! Synthetic paired controls on the real protected router; no listener or keys.
use super::*;
use tower::ServiceExt;

fn memory_fixture() -> Fixture {
    let mut fixture = Fixture::new("hidden-other-graph");
    // Memory commits reuse the configured Artifact vocabulary IRI.
    // The generic query fixture has no namespace; populate this fixture with one.
    fixture.source.lock().set_base_ns("https://example.org/");
    fixture.profile.memory_rpc = Some(quipu::config::MemoryFilesConfig {
        enabled: true,
        graph: "urn:a".into(),
        deployment: "urn:deployment".into(),
        project: "urn:project".into(),
    });
    for (crew, text) in [("urn:p", "owner-p-private"), ("urn:q", "owner-q-private")] {
        let input = serde_json::from_value(json!({
            "owner":{"deployment":"urn:deployment","project":"urn:project","crew":crew},
            "path":"MEMORY.md","text":text,"expected_snapshot":null,"expected_file":null,
            "idempotency_key":crew,"harness":"synthetic","account":"fixture"
        }))
        .unwrap();
        let mut store = fixture.source.lock();
        let graph = store.registered_graph_id("urn:a").unwrap().unwrap();
        quipu::memory_files::commit(
            &mut store,
            graph,
            &input,
            quipu::memory_files::Authority::Local,
            "2026-01-01T00:00:00Z",
        )
        .unwrap();
    }
    fixture
}

fn owner(crew: &str) -> Value {
    json!({"deployment":"urn:deployment","project":"urn:project","crew":crew})
}

#[tokio::test]
async fn populated_owner_only_memory_and_alternate_routes_fail_closed() {
    let fixture = memory_fixture();
    for path in ["/memory/read", "/memory/export"] {
        let (status, bytes) = fixture.call("POST", path, Some("p"), owner("urn:p")).await;
        assert_eq!(status, StatusCode::OK);
        let body = String::from_utf8(bytes).unwrap();
        if path == "/memory/export" {
            assert!(body.contains("owner-p-private"));
        } else {
            // Read returns the owner manifest, not file plaintext.
            let manifest: Value = serde_json::from_str(&body).unwrap();
            assert_eq!(manifest["snapshot"]["owner"]["crew"], "urn:p");
            assert_eq!(
                manifest["snapshot"]["files"]["MEMORY.md"]["bytes"],
                "owner-p-private".len()
            );
        }
        assert!(!body.contains("owner-q-private"));
        for token in [None, Some("q"), Some("u")] {
            assert_ne!(
                fixture.call("POST", path, token, owner("urn:p")).await.0,
                StatusCode::OK
            );
        }
        // q has a legitimate grant, but to another graph; own identity alone is insufficient.
        assert_eq!(
            fixture
                .call("POST", path, Some("q"), owner("urn:q"))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        let mut foreign = owner("urn:p");
        foreign["project"] = "urn:foreign".into();
        assert_eq!(
            fixture.call("POST", path, Some("p"), foreign).await.0,
            StatusCode::FORBIDDEN
        );
    }
    for (method, path, body) in [
        (
            "POST",
            "/query",
            json!({"query":"SELECT * WHERE { ?s ?p ?o }"}),
        ),
        ("POST", "/export", json!({})),
        ("GET", "/graphs", Value::Null),
        ("POST", "/memory/projection", owner("urn:p")),
        ("POST", "/memory/commit", json!({})),
        ("GET", "/events/stream", Value::Null),
        ("POST", "/memory/read?resume=old", owner("urn:p")),
    ] {
        assert_eq!(
            fixture.call(method, path, Some("p"), body).await.0,
            StatusCode::FORBIDDEN,
            "{path}"
        );
    }
}

fn export_grant(revision: u64, revoked: bool, until: u64) -> ReadGrant {
    ReadGrant {
        version: 1,
        id: "urn:grant:urn:p:Export".into(),
        principal: "urn:p".into(),
        graphs: ["urn:a".into(), "urn:c".into()].into(),
        operations: [ReadOperation::Export].into(),
        valid_from: 1,
        valid_until: until,
        max_offline_seconds: 300,
        issuer: "urn:owner".into(),
        revision,
        revoked,
    }
}

#[tokio::test]
async fn buffered_response_holds_revocation_then_revoked_and_expired_reads_deny() {
    let mut fixture = memory_fixture();
    let request = Request::builder()
        .method("POST")
        .uri("/memory/read")
        .header("authorization", "Bearer p")
        .header("content-type", "application/json")
        .body(Body::from(owner("urn:p").to_string()))
        .unwrap();
    let response = fixture.real_app(true).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let now = quipu::time::epoch_secs();
    let revoked = export_grant(2, true, now + 3600);
    // No false successful ACK while old bytes remain unconsumed.
    assert!(fixture.policy.apply(&revoked, "held-body revoke").is_err());
    drop(response);
    fixture
        .policy
        .apply(&revoked, "released-body revoke")
        .unwrap();
    assert_eq!(
        fixture
            .call("POST", "/memory/read", Some("p"), owner("urn:p"))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    fixture
        .policy
        .apply(&export_grant(3, false, now - 1), "expired reissue")
        .unwrap();
    assert_eq!(
        fixture
            .call("POST", "/memory/export", Some("p"), owner("urn:p"))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    fixture
        .policy
        .apply(&export_grant(4, false, now + 3600), "fresh reissue")
        .unwrap();
    assert_eq!(
        fixture
            .call("POST", "/memory/read", Some("p"), owner("urn:p"))
            .await
            .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn expired_or_backward_clock_buffer_emits_no_private_frame() {
    let fixture = memory_fixture();
    let now = quipu::time::epoch_secs();
    for window in [(now - 2, now - 1), (now + 1, now + 10)] {
        let permit = quipu::read_authorization::publication::PublicationPermit::acquire(
            &fixture.profile.policy_path,
        )
        .unwrap();
        let response = super::super::protected_body(
            "application/json".into(),
            b"secret".to_vec(),
            permit,
            window,
        );
        assert!(to_bytes(response.into_body(), 1024).await.is_err());
    }
}

#[tokio::test]
async fn exact_admin_needs_explicit_read_grant_shared_writer_is_not_admin() {
    let mut fixture = memory_fixture();
    let mut registry: Value =
        serde_json::from_slice(&std::fs::read(&fixture.registry).unwrap()).unwrap();
    registry["credentials"].as_array_mut().unwrap().push(json!({
        "principal":"urn:owner","credential_id":"admin","audience":"quipu",
        "token_sha256":format!("{:x}", Sha256::digest(b"admin"))
    }));
    std::fs::write(&fixture.registry, registry.to_string()).unwrap();
    assert_eq!(
        fixture
            .call("POST", "/memory/read", Some("admin"), owner("urn:p"))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let mut grant = export_grant(1, false, quipu::time::epoch_secs() + 3600);
    grant.id = "urn:admin:memory".into();
    grant.principal = "urn:owner".into();
    fixture.policy.apply(&grant, "explicit admin read").unwrap();
    assert_eq!(
        fixture
            .call("POST", "/memory/read", Some("admin"), owner("urn:p"))
            .await
            .0,
        StatusCode::OK
    );
    let request = Request::builder()
        .method("POST")
        .uri("/memory/read")
        .header("authorization", "Bearer shared")
        .header("content-type", "application/json")
        .body(Body::from(owner("urn:p").to_string()))
        .unwrap();
    let response = fixture
        .router_with_shared(Some("shared"))
        .oneshot(request)
        .await
        .unwrap();
    assert_ne!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn same_graph_grants_still_isolate_populated_owner_records() {
    let mut fixture = memory_fixture();
    let mut grant = export_grant(2, false, quipu::time::epoch_secs() + 3600);
    grant.id = "urn:grant:urn:q:Export".into();
    grant.principal = "urn:q".into();
    fixture
        .policy
        .apply(&grant, "q shares graph but not owner")
        .unwrap();
    for path in ["/memory/read", "/memory/export"] {
        assert_eq!(
            fixture
                .call("POST", path, Some("q"), owner("urn:p"))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        let (status, bytes) = fixture.call("POST", path, Some("q"), owner("urn:q")).await;
        assert_eq!(status, StatusCode::OK);
        let body = String::from_utf8(bytes).unwrap();
        assert!(!body.contains("owner-p-private"));
        if path == "/memory/export" {
            assert!(body.contains("owner-q-private"));
        }
    }
}

#[tokio::test]
async fn valid_signed_session_remains_query_capable_but_is_not_a_private_memory_identity() {
    use base64::Engine as _;
    use quipu::session_attestation::{
        AttestationEnvelope, SessionBinding, SignedBinding, WRITE_V2, WriteBinding, body_sha256,
        canonical_message,
    };
    use ring::signature::{Ed25519KeyPair, KeyPair};
    let mut fixture = memory_fixture();
    let now = quipu::time::epoch_secs();
    let key = Ed25519KeyPair::from_seed_unchecked(&[93; 32]).unwrap();
    let mut binding = SessionBinding::new(
        "urn:p",
        "private-memory-fixture",
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
    let signed = |path: &str, body: Value, nonce: &str| {
        let body = body.to_string();
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
                path,
                content_type: "application/json",
                body_sha256: &hash,
                audience: Some(&audience),
            }),
        );
        envelope.signature = hex::encode(key.sign(&message).as_ref());
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&envelope).unwrap());
        Request::post(path)
            .header("content-type", "application/json")
            .header(crate::auth::write_attest::HEADER, header)
            .header("authorization", "Bearer p")
            .body(Body::from(body))
            .unwrap()
    };
    // Same enrolled signing identity actually works in the existing query adapter.
    let private = fixture.profile.memory_rpc.take();
    let response = fixture
        .router()
        .oneshot(signed(
            "/query",
            json!({"query":"ASK { ?s ?p ?o }"}),
            "3123456789abcdef0123456789abcdef",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    fixture.profile.memory_rpc = private;
    let response = fixture
        .router()
        .oneshot(signed(
            "/memory/export",
            owner("urn:p"),
            "4123456789abcdef0123456789abcdef",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    // Ordinary verified named identity plus the same Export grant still succeeds.
    assert_eq!(
        fixture
            .call("POST", "/memory/export", Some("p"), owner("urn:p"))
            .await
            .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn generic_reserved_memory_ingest_stays_forbidden_beside_owner_read() {
    let fixture = memory_fixture();
    let request = Request::post("/load")
        .header("authorization", "Bearer p")
        .header("content-type", "text/turtle")
        .body(Body::from(
            "<urn:quipu:memory:owner:forged> <https://quipu.dev/memory#head> \"forged\" .",
        ))
        .unwrap();
    let response = fixture.real_app(true).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        fixture
            .call("POST", "/memory/read", Some("p"), owner("urn:p"))
            .await
            .0,
        StatusCode::OK
    );
}
