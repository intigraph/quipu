use super::{SERIAL, WRITER, fixture, ready, request, spawn};
use std::net::SocketAddr;
use std::time::Duration;

// Exercise the public listener with real signatures, not bearer-only requests.
// The pool must neither consume a signed read twice nor silently serve it on
// the primary. Each accepted request is also replayed through the public route.
#[test]
fn signed_reads_and_writes_cross_the_full_reader_pool_without_replaying_nonces() {
    exercise_signed_pool(false);
    exercise_signed_pool(true);
}

fn exercise_signed_pool(strict: bool) {
    use base64::Engine as _;
    use quipu::session_attestation::{
        AttestationEnvelope, SessionBinding, SignedBinding, WRITE_V2, WriteBinding, body_sha256,
        canonical_message,
    };
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use std::collections::BTreeSet;

    let _serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (root, address, worker) = fixture(2);
    let config_path = root.path().join(".bobbin/config.toml");
    let config = std::fs::read_to_string(&config_path)
        .unwrap()
        .replace("read_pool_size=0", "read_pool_size=4")
        .replace(
            "memory_budget_mib=256",
            "memory_budget_mib=768\nheavy_read_operations=['owner-rows']\nheavy_read_permits=1",
        );
    let config = if strict {
        // Strict keys and replication-only credentials intentionally cannot
        // coexist. Keep both supported profiles, never weaken the startup guard.
        let config = config
            .lines()
            .filter(|line| !line.starts_with("replication_credentials_file="))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "{config}\n[quipu.server.session_keys]\nrequired=true\nonboarding_read_paths=['/health']\n"
        )
    } else {
        config
    };
    std::fs::write(config_path, config).unwrap();
    let key = Ed25519KeyPair::from_seed_unchecked(&[73; 32]).unwrap();
    let now = quipu::time::epoch_secs();
    let mut binding = SessionBinding::new(
        "urn:fixture:signed-reader",
        "signed-pool-fixture",
        hex::encode(key.public_key().as_ref()),
        "urn:fixture:introducer",
        now - 10,
        now + 3600,
    )
    .unwrap();
    binding.allow_write = true;
    let store = quipu::Store::open(root.path().join("store.db").to_str().unwrap()).unwrap();
    store.attestation_register(&binding).unwrap();
    let audience = store.store_id().unwrap();
    drop(store);
    let mut server = spawn(root.path(), address, false);
    ready(&mut server, root.path());
    assert_eq!(
        request(address, "GET", "/health", WRITER, "")
            .unwrap()
            .header("x-quipu-read-processes-alive"),
        Some("2")
    );

    let sign = |path: &str, body: &str, nonce: usize| {
        let mut envelope = AttestationEnvelope {
            version: WRITE_V2.into(),
            key_id: binding.key_id.clone(),
            session: binding.session.clone(),
            introducer: binding.introducer.clone(),
            issued_at_epoch: quipu::time::epoch_secs(),
            nonce: format!("{nonce:032x}"),
            signature: String::new(),
            audience: Some(audience.clone()),
        };
        let hash = body_sha256(body.as_bytes());
        envelope.signature = hex::encode(
            key.sign(&canonical_message(
                &envelope,
                &SignedBinding::Write(WriteBinding {
                    method: "POST",
                    path,
                    content_type: "application/json",
                    body_sha256: &hash,
                    audience: Some(&audience),
                }),
            ))
            .as_ref(),
        );
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&envelope).unwrap())
    };
    let send =
        |target: SocketAddr, path: &str, body: &str, signature: &str, operation: Option<&str>| {
            let agent = ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(3))
                .redirects(0)
                .build();
            let mut call = agent
                .post(&format!("http://{target}{path}"))
                .set("Content-Type", "application/json")
                .set("x-quipu-attestation", signature);
            if let Some(operation) = operation {
                call = call.set("x-quipu-operation", operation);
            }
            match call.send_string(body) {
                Ok(response) | Err(ureq::Error::Status(_, response)) => response,
                Err(error) => panic!("private fixture transport failed: {error}"),
            }
        };
    let write = r#"{"turtle":"<urn:signed:item> <urn:signed:value> \"present\" ."}"#;
    let signature = sign("/knot", write, 1);
    let response = send(address, "/knot", write, &signature, None);
    assert_eq!(
        response.status(),
        200,
        "signed write must succeed on primary"
    );
    assert!(response.header("x-quipu-read-worker-pid").is_none());
    assert_eq!(
        send(address, "/knot", write, &signature, None).status(),
        401
    );

    let query = r#"{"query":"SELECT ?v WHERE { <urn:signed:item> <urn:signed:value> ?v }"}"#;
    let mut seen = BTreeSet::new();
    for nonce in 2..8 {
        let signature = sign("/query", query, nonce);
        let response = send(address, "/query", query, &signature, None);
        assert_eq!(
            response.status(),
            200,
            "fresh signed read must not be reverified as a replay by its child"
        );
        let pid: u32 = response
            .header("x-quipu-read-worker-pid")
            .expect("must cross a real worker")
            .parse()
            .unwrap();
        assert_ne!(pid, server.child.id());
        seen.insert(pid);
        let body: serde_json::Value =
            serde_json::from_str(&response.into_string().unwrap()).unwrap();
        assert_eq!(body["count"], 1);
        assert!(body.to_string().contains("present"));
        assert_eq!(
            send(address, "/query", query, &signature, None).status(),
            401
        );
    }
    assert_eq!(
        seen.len(),
        2,
        "every configured worker must actually serve a signed read"
    );
    let signature = sign("/query", query, 8);
    let heavy = send(address, "/query", query, &signature, Some("owner-rows"));
    assert_eq!(heavy.status(), 200);
    assert!(
        seen.contains(
            &heavy
                .header("x-quipu-read-worker-pid")
                .unwrap()
                .parse::<u32>()
                .unwrap()
        )
    );
    assert_eq!(
        send(address, "/query", query, &signature, Some("owner-rows")).status(),
        401
    );
    // Exact-byte binding must still refuse altered bodies, and never fall back.
    let signature = sign("/query", query, 9);
    assert_eq!(
        send(address, "/query", "{}", &signature, None).status(),
        401
    );
    // A public caller cannot turn a serialized identity into trusted evidence,
    // even while also supplying the existing transport bearer.
    let spoof = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
        serde_json::json!({"principal":binding.agent,"credential_id":binding.key_id,
            "auth_class":"attested_session"})
        .to_string(),
    );
    let public = ureq::post(&format!("http://{address}/query"))
        .set("Content-Type", "application/json")
        .set("Authorization", &format!("Bearer {WRITER}"))
        .set("x-quipu-read-worker-identity", &spoof)
        .set("x-quipu-attestation", "invalid-attestation")
        .send_string(query);
    assert!(matches!(public, Err(ureq::Error::Status(401, _))));
    if strict {
        let unsigned = ureq::post(&format!("http://{address}/query"))
            .set("Content-Type", "application/json")
            .set("Authorization", &format!("Bearer {WRITER}"))
            .set("x-quipu-read-worker-identity", &spoof)
            .send_string(query);
        assert!(matches!(unsigned, Err(ureq::Error::Status(401, _))));
    }
    // Concurrent copies must still be arbitrated once by the primary ledger.
    let signature = sign("/query", query, 10);
    let mut statuses = std::thread::scope(|scope| {
        let one = scope.spawn(|| send(address, "/query", query, &signature, None).status());
        let two = scope.spawn(|| send(address, "/query", query, &signature, None).status());
        vec![one.join().unwrap(), two.join().unwrap()]
    });
    statuses.sort_unstable();
    assert_eq!(statuses, [200, 401]);
    // Corrupt the signature without changing any declared identity.
    let mut bad: serde_json::Value = serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(sign("/query", query, 11))
            .unwrap(),
    )
    .unwrap();
    bad["signature"] = serde_json::Value::String("00".repeat(64));
    let bad =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(&bad).unwrap());
    assert_eq!(send(address, "/query", query, &bad, None).status(), 401);
    // A signature for /query cannot be applied to another read endpoint.
    assert_eq!(
        send(address, "/ask", query, &sign("/query", query, 12), None).status(),
        401
    );
    for port in [worker.port(), worker.port() + 1] {
        let target = SocketAddr::new(worker.ip(), port);
        assert_eq!(
            send(
                target,
                "/query",
                query,
                &sign("/query", query, usize::from(port)),
                None
            )
            .status(),
            403
        );
    }
    let store = quipu::Store::open(root.path().join("store.db").to_str().unwrap()).unwrap();
    store.attestation_revoke(&binding.session).unwrap();
    assert_eq!(
        send(address, "/query", query, &sign("/query", query, 13), None).status(),
        401
    );
}
