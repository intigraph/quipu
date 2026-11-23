//! Source controls for the explicit factory. No real witness is contacted.
use super::*;
use std::time::Duration;

fn config(f: &Fixture) -> EnrolledPilotConfig {
    EnrolledPilotConfig {
        root: f.root.clone(),
        inventory: f.root.join("inventory"),
        profile: "fixture".into(),
        endpoint: "https://witness.example.test".into(),
        bucket: "fixture".into(),
        region: "test-1".into(),
        prefix: "existing-lineage".into(),
        timeout: Duration::from_secs(2),
        witness_ca_der: None,
        installed_policy: "policy-v1".into(),
        allocation: crate::memory_budget::MemoryAllocation::for_connections(1024, 4).unwrap(),
    }
}

fn enabled() -> IngestPilot {
    IngestPilot {
        enabled: true,
        graph: Some(GRAPH.into()),
    }
}

#[test]
fn disabled_factory_ignores_invalid_config_and_never_resolves_credentials() {
    let f = fixture();
    let mut c = config(&f);
    c.inventory = PathBuf::from("relative");
    c.endpoint.clear();
    assert!(
        IngestPilot::default()
            .admit_enrolled(
                catalog_for(&f.route),
                &f.legacy,
                &c,
                || panic!("disabled credential callback"),
                |_| panic!("disabled writer configuration"),
            )
            .unwrap()
            .is_none()
    );
    assert!(!f.root.join("inventory").exists());
}

#[test]
fn missing_legacy_fence_refuses_before_credential_or_owner_startup() {
    let f = fixture();
    assert!(
        enabled()
            .admit_enrolled(
                catalog_for(&f.route),
                &f.legacy,
                &config(&f),
                || panic!("unfenced credential callback"),
                |_| panic!("unfenced writer configuration"),
            )
            .is_err()
    );
}

#[test]
fn invalid_profile_transport_timeout_and_policy_refuse_before_credentials() {
    let f = fixture();
    f.legacy.fence_routed_graph(GRAPH, 7, 4).unwrap();
    let base = config(&f);
    let mut cases = vec![];
    let mut c = base.clone();
    c.profile.clear();
    cases.push(c);
    let mut c = base.clone();
    c.endpoint = "http://witness.example.test".into();
    cases.push(c);
    let mut c = base.clone();
    c.timeout = Duration::ZERO;
    cases.push(c);
    let mut c = base.clone();
    c.timeout = Duration::from_secs(31);
    cases.push(c);
    let mut c = base.clone();
    c.installed_policy = "another-policy".into();
    cases.push(c);
    for c in cases {
        assert!(
            enabled()
                .admit_enrolled(
                    catalog_for(&f.route),
                    &f.legacy,
                    &c,
                    || panic!("invalid configuration resolved credentials"),
                    |_| panic!("invalid configuration opened owner"),
                )
                .is_err()
        );
    }
}

#[cfg(unix)]
#[path = "graph_router_enrolled_tls_tests.rs"]
mod tls;
