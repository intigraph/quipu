use super::*;
use crate::QuipuConfig;
use crate::config_load::ConfigFile;
use std::io::Write;

const FULL: &str = r#"
[quipu.ingest_pilot]
enabled = true
graph = "https://example.org/graph/code"
root = "/srv/owners"
inventory = "/srv/owners/inventory"
profile = "pilot"
endpoint = "https://witness.example.org"
bucket = "witness"
region = "us-east-1"
prefix = "lineage"
timeout_secs = 5
installed_policy = "policy-v1"
credentials_file = "/srv/owners/witness.json"
catalog_file = "/srv/owners/catalog.json"
memory_mib = 512
"#;

fn parse(toml_str: &str) -> Result<QuipuConfig, toml::de::Error> {
    toml::from_str::<ConfigFile>(toml_str).map(|f| f.quipu)
}

#[test]
fn absent_table_is_none_and_silent() {
    let cfg = QuipuConfig::default();
    assert!(cfg.ingest_pilot.is_none());
    assert!(cfg.unwired_warnings().is_empty());
    let cfg = parse("[quipu]\n").unwrap();
    assert!(cfg.ingest_pilot.is_none());
}

#[test]
fn full_table_parses_every_field() {
    let pilot = parse(FULL).unwrap().ingest_pilot.unwrap();
    assert!(pilot.enabled);
    assert_eq!(pilot.graph, "https://example.org/graph/code");
    assert_eq!(pilot.timeout_secs, 5);
    assert_eq!(pilot.memory_mib, 512);
    assert!(pilot.witness_ca_file.is_none());
}

#[test]
fn unknown_or_missing_keys_refuse_at_load() {
    let typo = FULL.replace("memory_mib", "memory_mb");
    assert!(
        parse(&typo).is_err(),
        "a misspelled key must not be dropped"
    );
    let missing = FULL.replace("catalog_file = \"/srv/owners/catalog.json\"\n", "");
    assert!(parse(&missing).is_err(), "every pin is required");
    let secret = format!("{FULL}secret_access_key = \"inline\"\n");
    assert!(parse(&secret).is_err(), "secrets never live in TOML");
}

#[test]
fn enabled_pilot_warns_only_where_it_cannot_start() {
    let cfg = parse(FULL).unwrap();
    let warned = cfg
        .unwired_warnings()
        .iter()
        .any(|w| w.contains("ingest_pilot"));
    assert_eq!(
        warned,
        cfg!(not(all(unix, feature = "ownership-witness-s3")))
    );
    let off = parse(&FULL.replace("enabled = true", "enabled = false")).unwrap();
    assert!(off.unwired_warnings().is_empty());
}

#[cfg(unix)]
fn file_with_mode(dir: &Path, name: &str, body: &str, mode: u32) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::File::create(&path)
        .unwrap()
        .write_all(body.as_bytes())
        .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    path
}

#[cfg(unix)]
#[test]
fn credentials_require_an_owner_only_file_and_never_echo() {
    let dir = tempfile::tempdir().unwrap();
    let mut pilot = parse(FULL).unwrap().ingest_pilot.unwrap();
    let body = r#"{"access_key_id":"KEY-SENTINEL","secret_access_key":"SECRET-SENTINEL"}"#;
    pilot.credentials_file = file_with_mode(dir.path(), "ok.json", body, 0o600);
    assert_eq!(
        pilot.read_credentials().unwrap(),
        ("KEY-SENTINEL".into(), "SECRET-SENTINEL".into())
    );
    pilot.credentials_file = file_with_mode(dir.path(), "open.json", body, 0o640);
    let error = pilot.read_credentials().unwrap_err().to_string();
    assert!(error.contains("unsafe permissions"), "{error}");
    let malformed = r#"{"access_key_id":"KEY-SENTINEL","secret":"SECRET-SENTINEL"}"#;
    pilot.credentials_file = file_with_mode(dir.path(), "bad.json", malformed, 0o600);
    let error = pilot.read_credentials().unwrap_err().to_string();
    assert!(!error.contains("SENTINEL"), "{error}");
    pilot.credentials_file = dir.path().join("absent.json");
    assert!(pilot.read_credentials().is_err());
}

#[cfg(unix)]
#[test]
fn witness_ca_is_optional_and_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let mut pilot = parse(FULL).unwrap().ingest_pilot.unwrap();
    assert!(pilot.read_witness_ca().unwrap().is_none());
    pilot.witness_ca_file = Some(file_with_mode(dir.path(), "ca.der", "der", 0o644));
    assert_eq!(pilot.read_witness_ca().unwrap().unwrap(), b"der");
    let big = "x".repeat(64 * 1024 + 1);
    pilot.witness_ca_file = Some(file_with_mode(dir.path(), "big.der", &big, 0o644));
    assert!(pilot.read_witness_ca().is_err());
    pilot.witness_ca_file = Some(file_with_mode(dir.path(), "w.der", "der", 0o666));
    assert!(
        pilot.read_witness_ca().is_err(),
        "a writable anchor is refused"
    );
}

const READS: &str = r#"
[quipu.ingest_pilot.reads]
policy_file = "/etc/quipu/read-policy.db"
owner = "urn:owner"
registry_file = "/etc/quipu/read-registry.json"
timeout_secs = 10
concurrency = 1
runtime_mib = 16
view_mib = 16
query_mib = 16
max_response_mib = 1
"#;

#[test]
fn reads_table_is_optional_and_explicit() {
    // aegis-4qv.8: absent reads keeps the knot-only pilot exactly as before.
    assert!(parse(FULL).unwrap().ingest_pilot.unwrap().reads.is_none());
    let reads = parse(&format!("{FULL}{READS}"))
        .unwrap()
        .ingest_pilot
        .unwrap()
        .reads
        .unwrap();
    assert_eq!(reads.owner, "urn:owner");
    assert_eq!(reads.concurrency, 1);
    assert_eq!(reads.timeout_secs, 10);
    // Every read key is required, and a typo cannot drop a pin.
    let missing = READS.replace("owner = \"urn:owner\"\n", "");
    assert!(parse(&format!("{FULL}{missing}")).is_err());
    assert!(parse(&format!("{FULL}{READS}graphs = [\"urn:x\"]\n")).is_err());
}
