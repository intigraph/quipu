use super::*;
use crate::crew_credentials::CredentialRegistry;
use sha2::{Digest, Sha256};

fn registry() -> CredentialRegistry {
    let hash = format!("{:x}", Sha256::digest(b"private-replication-fixture"));
    CredentialRegistry::parse(
        serde_json::json!({"version":1,"credentials":[{
            "credential_id":"standby-fixture", "principal":"urn:replica:fixture",
            "audience":"quipu", "token_sha256":hash
        }]})
        .to_string()
        .as_bytes(),
        "quipu",
    )
    .unwrap()
}

fn policy() -> BearerPolicy {
    BearerPolicy::new(Some("private-writer-fixture".into()), None, None, 1)
        .unwrap()
        .with_replication_only(registry())
        .unwrap()
}

const HEADER: Option<&str> = Some("Bearer private-replication-fixture");

#[test]
fn dedicated_replication_identity_admits_only_three_exact_post_operations() {
    let policy = policy();
    for path in ["/share/asof", "/replicate/stream", "/replicate/challenge"] {
        let result = authorize_http_bearers(path, "POST", false, &policy, HEADER, 2);
        assert_eq!(result.decision, AccessDecision::Allow, "{path}");
        assert_eq!(result.generation, Some(AuthGeneration::Named));
        for method in ["GET", "HEAD", "PUT", "PATCH", "DELETE", "OPTIONS"] {
            assert_eq!(
                authorize_http_bearers(path, method, false, &policy, HEADER, 2).decision,
                AccessDecision::Unauthorized,
                "{method} {path}"
            );
        }
        assert_eq!(
            authorize_http_bearers(path, "POST", true, &policy, HEADER, 2).decision,
            AccessDecision::ReadOnly
        );
    }
    assert_eq!(
        policy.replication_principal(HEADER).unwrap().iri,
        "urn:replica:fixture"
    );
    assert!(policy.named_principal(HEADER).is_none());
}

#[test]
fn replication_identity_cannot_write_read_or_fall_back_to_anonymous() {
    let policy = policy();
    for path in crate::http_auth::WRITE_ENDPOINTS
        .iter()
        .chain(crate::http_auth::READ_ENDPOINTS.iter())
        .copied()
        .chain(["/unknown", "/replicate/stream/", "/replicate/anything"])
    {
        if ["/share/asof", "/replicate/stream", "/replicate/challenge"].contains(&path) {
            continue;
        }
        for method in ["POST", "GET", "HEAD", "DELETE"] {
            assert_eq!(
                authorize_http_bearers(path, method, false, &policy, HEADER, 2).decision,
                AccessDecision::Unauthorized,
                "{method} {path}"
            );
        }
    }
    for write in [true, false] {
        assert_eq!(
            authorize_bearers(write, false, &policy, HEADER, 2).decision,
            AccessDecision::Unauthorized
        );
    }
}

#[test]
fn default_and_ordinary_write_auth_remain_separate_from_replication() {
    let policy = policy();
    for path in [
        "/share/asof",
        "/replicate/stream",
        "/replicate/challenge",
        "/update",
    ] {
        for header in [None, Some("Bearer unknown-fixture")] {
            assert_eq!(
                authorize_http_bearers(path, "POST", false, &policy, header, 2).decision,
                AccessDecision::Unauthorized
            );
        }
        assert_eq!(
            authorize_http_bearers(
                path,
                "POST",
                false,
                &policy,
                Some("Bearer private-writer-fixture"),
                2
            )
            .decision,
            AccessDecision::Allow
        );
    }
    assert_eq!(
        authorize_http_bearers("/query", "POST", false, &policy, None, 2).decision,
        AccessDecision::Allow
    );
    let default = BearerPolicy::new(Some("private-writer-fixture".into()), None, None, 1).unwrap();
    assert_eq!(
        authorize_http_bearers("/replicate/stream", "POST", false, &default, HEADER, 2).decision,
        AccessDecision::Unauthorized
    );
}

#[test]
fn a_replication_registry_cannot_also_supply_a_writer_credential() {
    let named = BearerPolicy::new(Some("private-writer-fixture".into()), None, None, 1)
        .unwrap()
        .with_named(registry());
    assert!(named.with_replication_only(registry()).is_err());
    let shared =
        BearerPolicy::new(Some("private-replication-fixture".into()), None, None, 1).unwrap();
    assert!(shared.with_replication_only(registry()).is_err());
    let previous = BearerPolicy::new(
        Some("private-writer-fixture".into()),
        Some("private-replication-fixture".into()),
        Some(100),
        1,
    )
    .unwrap();
    assert!(previous.with_replication_only(registry()).is_err());
    assert!(
        policy()
            .with_replication_only(CredentialRegistry::default())
            .is_err()
    );
}
