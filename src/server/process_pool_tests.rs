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
        "/feed/ws",
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

#[test]
fn only_validated_internal_worker_omits_primary_replication_authority() {
    let mut config = quipu::config::ServerConfig::default();
    config.auth_token = Some("writer-fixture".into());
    config.replication_credentials_file = Some("primary-registry.json".into());
    config.replica = Some(quipu::config::ReplicaConfig {
        primary_url: "http://127.0.0.1:3030".into(),
        expected_store_id: Some("urn:store:fixture".into()),
    });
    let mut pool = ProcessPool {
        children: Vec::new(),
        routing: None,
        internal_worker: false,
    };
    // Primary and external replicas retain the configured authority and its refusal.
    assert!(matches!(pool.auth_config(&config), Cow::Borrowed(_)));
    assert_eq!(
        pool.auth_config(&config).replication_credentials_file,
        config.replication_credentials_file
    );
    pool.internal_worker = true;
    let worker = pool.auth_config(&config);
    assert!(worker.replication_credentials_file.is_none());
    assert!(worker.replica.is_some());
    assert_eq!(
        worker.replica.as_ref().unwrap().expected_store_id,
        config.replica.as_ref().unwrap().expected_store_id
    );
    assert_eq!(worker.auth_token, config.auth_token);
    assert_eq!(worker.read_only, config.read_only);
    assert!(config.replication_credentials_file.is_some());
}
