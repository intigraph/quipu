use super::*;
#[test]
fn process_routing_never_sends_writes_or_streams_to_readers() {
    for path in quipu::http_auth::WRITE_ENDPOINTS {
        for method in ["GET", "POST", "DELETE"] {
            assert!(!routed(path, method), "{method} {path}");
        }
    }
    for path in [
        "/events/stream",
        "/export",
        "/acquire/composed",
        "/acquire/eligibility",
        "/metrics",
        "/health",
        "/unknown",
    ] {
        assert!(!routed(path, "GET"));
    }
    for path in ["/query", "/search", "/resolve"] {
        assert!(routed(path, "POST"));
        assert!(!routed(path, "DELETE"));
    }
}
#[test]
#[cfg(target_os = "linux")]
fn process_validation_refuses_unsupported_or_recursive_topology() {
    let mut config = QuipuConfig::default();
    config.server.read_processes = 2;
    config.server.memory_budget_mib = Some(1024);
    assert!(validate(&config, &[]).is_ok());
    config.server.read_process_base_port = 3030;
    assert!(validate(&config, &[]).is_err());
    config.server.read_process_base_port = 65535;
    assert!(validate(&config, &[]).is_err());
    config.server.read_process_base_port = 3031;
    config.server.read_only = true;
    assert!(validate(&config, &[]).is_err());
}

#[test]
#[cfg(not(target_os = "linux"))]
fn process_validation_refuses_unsupported_platform() {
    let mut config = QuipuConfig::default();
    config.server.read_processes = 2;
    config.server.memory_budget_mib = Some(1024);
    assert!(validate(&config, &[]).is_err());
}
