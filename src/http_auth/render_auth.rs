//! Protected read computation, distinct from graph-write classification.
use super::{
    AccessDecision, AuthGeneration, Authorization, BearerPolicy, authorize_bearers,
    is_write_request, requires_read_identity,
};

/// HTTP rendering is a protected computation, not a graph write. Keep the
/// read-only handle and read-only-server support while requiring a real bearer.
pub fn authorize_http_bearers(
    path: &str,
    method: &str,
    read_only: bool,
    policy: &BearerPolicy,
    auth_header: Option<&str>,
    now_epoch_secs: u64,
) -> Authorization {
    // A presented replication credential cannot fall through to an open-read,
    // shared-write or ordinary named-write policy on any other operation.
    if policy.replication_principal(auth_header).is_some() {
        let allowed = method == "POST"
            && matches!(
                path,
                "/share/asof" | "/replicate/stream" | "/replicate/challenge"
            );
        return Authorization {
            decision: if read_only {
                AccessDecision::ReadOnly
            } else if allowed {
                AccessDecision::Allow
            } else {
                AccessDecision::Unauthorized
            },
            generation: (allowed && !read_only).then_some(AuthGeneration::Named),
        };
    }
    let write = is_write_request(path, method);
    let protected_read = path == "/render";
    let authorization = authorize_bearers(
        write || protected_read || requires_read_identity(path),
        write && read_only,
        policy,
        auth_header,
        now_epoch_secs,
    );
    if protected_read && authorization.generation == Some(AuthGeneration::OpenWrite) {
        return Authorization {
            decision: AccessDecision::Unauthorized,
            generation: None,
        };
    }
    authorization
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renderer_adapter_preserves_memory_identity_routes() {
        let policy = BearerPolicy::new(Some("test-secret".into()), None, None, 1).unwrap();
        for path in ["/memory/projection", "/memory/receipt"] {
            for read_only in [false, true] {
                for header in [None, Some("Bearer wrong")] {
                    assert_eq!(
                        authorize_http_bearers(path, "POST", read_only, &policy, header, 2)
                            .decision,
                        AccessDecision::Unauthorized
                    );
                }
                assert_eq!(
                    authorize_http_bearers(
                        path,
                        "POST",
                        read_only,
                        &policy,
                        Some("Bearer test-secret"),
                        2
                    )
                    .decision,
                    AccessDecision::Allow
                );
            }
        }
    }
    #[test]
    fn rendering_requires_real_bearer_without_becoming_a_write() {
        let policy = BearerPolicy::new(Some("test-secret".into()), None, None, 1).unwrap();
        for read_only in [false, true] {
            assert_eq!(
                authorize_http_bearers("/render", "POST", read_only, &policy, None, 2).decision,
                AccessDecision::Unauthorized
            );
            assert_eq!(
                authorize_http_bearers(
                    "/render",
                    "POST",
                    read_only,
                    &policy,
                    Some("Bearer wrong"),
                    2
                )
                .decision,
                AccessDecision::Unauthorized
            );
            assert_eq!(
                authorize_http_bearers(
                    "/render",
                    "POST",
                    read_only,
                    &policy,
                    Some("Bearer test-secret"),
                    2
                )
                .decision,
                AccessDecision::Allow
            );
            assert_eq!(
                authorize_http_bearers("/query", "POST", read_only, &policy, None, 2).decision,
                AccessDecision::Allow
            );
        }
        let open = BearerPolicy::new(None, None, None, 1).unwrap();
        assert_eq!(
            authorize_http_bearers("/render", "POST", false, &open, None, 2).decision,
            AccessDecision::Unauthorized
        );
        assert!(!is_write_request("/render", "POST"));
    }
}

#[cfg(test)]
#[path = "replication_auth_tests.rs"]
mod replication_tests;
