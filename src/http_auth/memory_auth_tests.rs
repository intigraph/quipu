use super::*;
use crate::crew_credentials::CredentialRegistry;
use crate::http_auth::{AccessDecision, authorize_bearers, authorize_http_bearers};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn entry() -> Value {
    json!({"credential_id":"memory-fixture", "principal":"urn:example:alpha",
        "audience":"quipu-memory", "token_sha256":format!("{:x}",Sha256::digest(b"memory-fixture")),
        "expires_at_epoch_secs":100,
        "memory_scope":{"graph":"urn:example:memory", "deployment":"urn:example:deployment",
            "project":"urn:example:project"}})
}

fn registry(rows: Vec<Value>) -> Result<CredentialRegistry, &'static str> {
    CredentialRegistry::parse(
        &serde_json::to_vec(&json!({"version":1,"credentials":Value::Array(rows)})).unwrap(),
        "quipu",
    )
}

#[test]
fn memory_scope_requires_explicit_expiry_and_valid_exact_boundary() {
    for field in ["expires_at_epoch_secs", "memory_scope"] {
        let mut row = entry();
        row.as_object_mut().unwrap().remove(field);
        assert!(registry(vec![row]).is_err());
        let mut row = entry();
        row[field] = Value::Null;
        assert!(registry(vec![row]).is_err());
    }
    for bad in [json!(0), json!(-1), json!(true), json!("100")] {
        let mut row = entry();
        row["expires_at_epoch_secs"] = bad;
        assert!(registry(vec![row]).is_err());
    }
    for key in ["graph", "deployment", "project"] {
        let mut row = entry();
        row["memory_scope"][key] = json!("not an IRI");
        assert!(registry(vec![row]).is_err());
    }
    let mut row = entry();
    row["audience"] = json!("quipu");
    assert!(registry(vec![row]).is_err());
}

#[test]
fn memory_token_never_resolves_as_writer_or_replication_credential() {
    let registry = registry(vec![entry()]).unwrap();
    assert!(
        registry
            .authenticate(Some("Bearer memory-fixture"))
            .is_none()
    );
    let policy = BearerPolicy::new(Some("shared-fixture".into()), None, None, 1).unwrap();
    assert!(
        policy
            .clone()
            .with_replication_only(registry.clone())
            .is_err()
    );
    let policy = policy.with_named(registry);
    assert!(
        policy
            .named_principal(Some("Bearer memory-fixture"))
            .is_none()
    );
    assert_eq!(
        authorize_bearers(false, false, &policy, Some("Bearer memory-fixture"), 1).decision,
        AccessDecision::Unauthorized
    );
}

#[test]
fn exact_http_operations_expiry_and_no_open_fallback() {
    for shared in [None, Some("shared-fixture".into())] {
        let policy = BearerPolicy::new(shared, None, None, 1)
            .unwrap()
            .with_named(registry(vec![entry()]).unwrap());
        for path in ["/memory/commit", "/memory/projection", "/memory/receipt"] {
            assert_eq!(
                authorize_http_bearers(
                    path,
                    "POST",
                    false,
                    &policy,
                    Some("Bearer memory-fixture"),
                    99
                )
                .decision,
                AccessDecision::Allow
            );
            for now in [100, 101] {
                assert_eq!(
                    authorize_http_bearers(
                        path,
                        "POST",
                        false,
                        &policy,
                        Some("Bearer memory-fixture"),
                        now
                    )
                    .decision,
                    AccessDecision::Unauthorized
                );
            }
            assert_eq!(
                authorize_http_bearers(
                    path,
                    "GET",
                    false,
                    &policy,
                    Some("Bearer memory-fixture"),
                    99
                )
                .decision,
                AccessDecision::Unauthorized
            );
        }
        for path in [
            "/query",
            "/stats",
            "/episode",
            "/update",
            "/knot",
            "/render",
            "/memory/read",
            "/memory/export",
            "/share/asof",
            "/replicate/stream",
        ] {
            for now in [99, 100] {
                assert_eq!(
                    authorize_http_bearers(
                        path,
                        "POST",
                        false,
                        &policy,
                        Some("Bearer memory-fixture"),
                        now
                    )
                    .decision,
                    AccessDecision::Unauthorized
                );
            }
        }
        assert_eq!(
            authorize_http_bearers(
                "/memory/commit",
                "POST",
                true,
                &policy,
                Some("Bearer memory-fixture"),
                99
            )
            .decision,
            AccessDecision::ReadOnly
        );
        assert_eq!(
            authorize_http_bearers("/query", "POST", false, &policy, None, 99).decision,
            AccessDecision::Allow
        );
    }
}

#[test]
fn removing_then_reissuing_registry_entry_changes_only_its_authority() {
    let legacy = json!({"credential_id":"legacy", "principal":"urn:example:legacy",
        "audience":"quipu", "token_sha256":format!("{:x}",Sha256::digest(b"legacy-fixture"))});
    let before = registry(vec![legacy.clone(), entry()]).unwrap();
    let revoked = registry(vec![legacy.clone()]).unwrap();
    let after = registry(vec![legacy, entry()]).unwrap();
    for value in [&before, &revoked, &after] {
        assert_eq!(
            value
                .authenticate(Some("Bearer legacy-fixture"))
                .unwrap()
                .iri,
            "urn:example:legacy"
        );
    }
    assert!(
        before
            .memory_credential(Some("Bearer memory-fixture"))
            .is_some()
    );
    assert!(
        revoked
            .memory_credential(Some("Bearer memory-fixture"))
            .is_none()
    );
    assert!(
        after
            .memory_credential(Some("Bearer memory-fixture"))
            .is_some()
    );
}
