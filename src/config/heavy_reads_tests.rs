use super::ServerConfig;

#[test]
fn heavy_policy_defaults_inert_and_rejects_unusable_reserves() {
    let mut config = ServerConfig::default();
    assert!(config.heavy_read_operations.is_empty());
    assert_eq!(config.heavy_read_permits, 1);
    assert!(config.validate_heavy_reads(0).is_ok());
    config.heavy_read_operations = vec!["owner-rows".into()];
    assert!(config.validate_heavy_reads(4).is_ok());
    for readers in [0, 1] {
        assert!(config.validate_heavy_reads(readers).is_err());
    }
    for permits in [0, 4, usize::MAX] {
        config.heavy_read_permits = permits;
        assert!(config.validate_heavy_reads(4).is_err());
    }
    config.heavy_read_permits = 1;
    config.read_processes = 2;
    assert!(config.validate_heavy_reads(4).is_err());
    config.read_processes = 0;
    for operations in [
        vec!["unknown".into()],
        vec!["owner-rows".into(), "owner-rows".into()],
    ] {
        config.heavy_read_operations = operations;
        assert!(config.validate_heavy_reads(4).is_err());
    }
}

#[test]
fn heavy_policy_round_trips_real_configuration_table() {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join(".bobbin")).unwrap();
    std::fs::write(
        project.path().join(".bobbin/config.toml"),
        "[quipu.server]\nheavy_read_operations = ['owner-rows']\nheavy_read_permits = 2\n",
    )
    .unwrap();
    let config = super::QuipuConfig::load(project.path());
    assert_eq!(config.server.heavy_read_operations, ["owner-rows"]);
    assert_eq!(config.server.heavy_read_permits, 2);
    assert!(config.server.validate_heavy_reads(3).is_ok());
    assert!(config.server.validate_heavy_reads(2).is_err());
}
