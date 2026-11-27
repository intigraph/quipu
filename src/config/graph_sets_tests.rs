use super::*;
use crate::QuipuConfig;
use crate::config_load::ConfigFile;
use serde_json::{Value, json};
use std::path::Path;

const SET_GRAPH: &str = "https://example.org/app/data";
const SET_PROV: &str = "https://example.org/app/data#provenance";
const PILOT_GRAPH: &str = "https://example.org/graph/code";

fn refusal(result: crate::Result<GraphSetCatalog>) -> crate::Error {
    match result {
        Err(e) => e,
        Ok(_) => panic!("preflight admitted the catalog"),
    }
}

fn parse(toml_str: &str) -> Result<QuipuConfig, toml::de::Error> {
    toml::from_str::<ConfigFile>(toml_str).map(|f| f.quipu)
}

fn table(root: &Path) -> String {
    format!(
        "[quipu.graph_sets]\nenabled = true\ncatalog_file = \"{}\"\nroot = \"{}\"\n\
         installed_policy = \"policy-v1\"\nmemory_mib = 512\n\
         inventory = \"{}\"\nprofile = \"sets\"\nendpoint = \"https://witness.example.org\"\n\
         bucket = \"witness\"\nregion = \"us-east-1\"\ntimeout_secs = 5\n\
         credentials_file = \"{}\"\n\
         [quipu.graph_sets.prefixes]\n\"{SET_GRAPH}\" = \"lineage/app\"\n",
        root.join("graph-sets.json").display(),
        root.display(),
        root.join("inventory").display(),
        root.join("witness.json").display(),
    )
}

fn route(graph: &str, file_id: &str, store_id: &str) -> Value {
    json!({
        "graph": graph, "file_id": file_id, "store_id": store_id,
        "log_epoch": "epoch", "generation": 3, "policy": "policy-v1"
    })
}

fn budget() -> Value {
    json!({"max_owners": 1, "queued_per_owner": 1, "readers_per_owner": 1,
           "max_queued": 1, "max_connections": 3})
}

fn set_doc(members: &[&str], file_id: &str, store_id: &str) -> Value {
    json!({
        "version": 1, "revision": 7, "legacy_store_id": "store-legacy", "max_graphs": 8,
        "budget": budget(),
        "sets": [{"owner": route(SET_GRAPH, file_id, store_id), "graphs": members}],
        "legacy": {"participant": route(crate::schema::ROOT_GRAPH_IRI, "legacy", "store-legacy"),
                   "graphs": [crate::schema::ROOT_GRAPH_IRI]}
    })
}

fn pilot_doc(graph: &str, file_id: &str, store_id: &str) -> Value {
    json!({
        "version": 1, "revision": 7, "legacy_store_id": "store-legacy", "budget": budget(),
        "routes": [route(graph, file_id, store_id)],
        "legacy_participant": route(crate::schema::ROOT_GRAPH_IRI, "legacy", "store-legacy")
    })
}

fn write(path: &Path, doc: &Value) {
    std::fs::write(path, doc.to_string()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
}

fn pilot(root: &Path, enabled: bool, graph: &str) -> IngestPilotConfig {
    IngestPilotConfig {
        enabled,
        graph: graph.into(),
        root: root.into(),
        inventory: root.join("inventory"),
        profile: "pilot".into(),
        endpoint: "https://witness.example.org".into(),
        bucket: "witness".into(),
        region: "us-east-1".into(),
        prefix: "lineage".into(),
        timeout_secs: 5,
        witness_ca_file: None,
        installed_policy: "policy-v1".into(),
        credentials_file: root.join("witness.json"),
        catalog_file: root.join("pilot.json"),
        memory_mib: 512,
        reads: None,
    }
}

/// Graph-set table plus an ingest pilot, both in `dir`.
fn setup(set: &Value, pilot_catalog: &Value) -> (tempfile::TempDir, GraphSetsConfig) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    write(&root.join("graph-sets.json"), set);
    write(&root.join("pilot.json"), pilot_catalog);
    let cfg = parse(&table(&root)).unwrap().graph_sets.unwrap();
    (dir, cfg)
}

#[test]
fn absent_table_is_none_and_silent() {
    let cfg = QuipuConfig::default();
    assert!(cfg.graph_sets.is_none());
    assert!(cfg.unwired_warnings().is_empty());
    assert!(parse("[quipu]\n").unwrap().graph_sets.is_none());
}

#[test]
fn full_table_parses_and_incomplete_or_unknown_fields_refuse() {
    let root = Path::new("/srv/owners");
    let cfg = parse(&table(root)).unwrap().graph_sets.unwrap();
    assert!(cfg.enabled);
    assert_eq!(cfg.root, root);
    assert_eq!(cfg.memory_mib, 512);
    assert_eq!(
        cfg.prefixes.get(SET_GRAPH).map(String::as_str),
        Some("lineage/app")
    );
    assert!(
        cfg.witness_ca_file.is_none(),
        "the trust anchor is optional"
    );
    let unknown = table(root).replace(
        "memory_mib = 512\n",
        "memory_mib = 512\nprefix = \"https://example.org/app/\"\n",
    );
    assert!(parse(&unknown).is_err(), "no prefix membership knob exists");
    let missing = table(root).replace("memory_mib = 512\n", "");
    assert!(parse(&missing).is_err());
    let no_prefixes = table(root)
        .split("[quipu.graph_sets.prefixes]")
        .next()
        .unwrap()
        .to_owned();
    assert!(parse(&no_prefixes).is_err(), "prefixes are required");
}

#[test]
fn disjoint_catalogs_pass_preflight() {
    let (dir, cfg) = setup(
        &set_doc(&[SET_GRAPH, SET_PROV], "app", "store-app"),
        &pilot_doc(PILOT_GRAPH, "ingest", "store-ingest"),
    );
    let pilot = pilot(&dir.path().canonicalize().unwrap(), true, PILOT_GRAPH);
    let catalog = cfg.preflight(Some(&pilot)).unwrap();
    assert_eq!(
        catalog.owner_graphs(SET_PROV).unwrap(),
        [SET_GRAPH, SET_PROV]
    );
    assert!(cfg.preflight(None).is_ok());
}

#[test]
fn a_graph_claimed_by_both_tables_refuses() {
    // The pilot's own graph is a graph-set member.
    let (dir, cfg) = setup(
        &set_doc(&[SET_GRAPH, SET_PROV, PILOT_GRAPH], "app", "store-app"),
        &pilot_doc(PILOT_GRAPH, "ingest", "store-ingest"),
    );
    let root = dir.path().canonicalize().unwrap();
    let err = refusal(cfg.preflight(Some(&pilot(&root, true, PILOT_GRAPH))));
    assert!(err.to_string().contains("also a graph-set member"), "{err}");
    // A pilot catalog routing a set member (even under another selected name).
    let (dir, cfg) = setup(
        &set_doc(&[SET_GRAPH, SET_PROV], "app", "store-app"),
        &pilot_doc(SET_PROV, "ingest", "store-ingest"),
    );
    let root = dir.path().canonicalize().unwrap();
    let err = refusal(cfg.preflight(Some(&pilot(&root, true, PILOT_GRAPH))));
    assert!(err.to_string().contains("claimed by two catalogs"), "{err}");
}

#[test]
fn one_physical_file_or_lineage_claimed_by_both_tables_refuses() {
    let (dir, cfg) = setup(
        &set_doc(&[SET_GRAPH, SET_PROV], "shared", "store-app"),
        &pilot_doc(PILOT_GRAPH, "shared", "store-ingest"),
    );
    let root = dir.path().canonicalize().unwrap();
    let err = refusal(cfg.preflight(Some(&pilot(&root, true, PILOT_GRAPH))));
    assert!(err.to_string().contains("physical owner file"), "{err}");
    let (dir, cfg) = setup(
        &set_doc(&[SET_GRAPH, SET_PROV], "app", "store-shared"),
        &pilot_doc(PILOT_GRAPH, "ingest", "store-shared"),
    );
    let root = dir.path().canonicalize().unwrap();
    let err = refusal(cfg.preflight(Some(&pilot(&root, true, PILOT_GRAPH))));
    assert!(err.to_string().contains("physical owner file"), "{err}");
}

#[test]
fn a_disabled_pilot_is_not_consulted() {
    let (dir, cfg) = setup(
        &set_doc(&[SET_GRAPH, SET_PROV, PILOT_GRAPH], "app", "store-app"),
        &pilot_doc(PILOT_GRAPH, "app", "store-app"),
    );
    let root = dir.path().canonicalize().unwrap();
    assert!(
        cfg.preflight(Some(&pilot(&root, false, PILOT_GRAPH)))
            .is_ok()
    );
}

#[test]
fn a_graph_claimed_by_two_sets_or_a_policy_mismatch_refuses() {
    let mut two = set_doc(&[SET_GRAPH, SET_PROV], "app", "store-app");
    two["sets"].as_array_mut().unwrap().push(json!({
        "owner": route("https://example.org/other", "other", "store-other"),
        "graphs": ["https://example.org/other", SET_PROV]
    }));
    two["budget"]["max_owners"] = json!(2);
    two["budget"]["max_queued"] = json!(2);
    two["budget"]["max_connections"] = json!(6);
    let mut disjoint = two.clone();
    disjoint["sets"][1]["graphs"] = json!(["https://example.org/other"]);
    let (_dir, cfg) = setup(&two, &pilot_doc(PILOT_GRAPH, "ingest", "store-ingest"));
    let err = refusal(cfg.preflight(None));
    assert!(
        err.to_string().contains("duplicate graph-set membership"),
        "{err}"
    );
    let (_dir, cfg) = setup(&disjoint, &pilot_doc(PILOT_GRAPH, "ingest", "store-ingest"));
    cfg.preflight(None)
        .expect("positive twin: two disjoint sets");

    let (_dir, mut cfg) = setup(
        &set_doc(&[SET_GRAPH, SET_PROV], "app", "store-app"),
        &pilot_doc(PILOT_GRAPH, "ingest", "store-ingest"),
    );
    cfg.installed_policy = "policy-v2".into();
    let err = refusal(cfg.preflight(None));
    assert!(err.to_string().contains("installed policy"), "{err}");
}

#[test]
fn enabled_startup_refuses_in_one_line_naming_the_reason() {
    // Valid catalog, no conflicting pilot: the fixed one-line refusal.
    let (dir, cfg) = setup(
        &set_doc(&[SET_GRAPH, SET_PROV], "app", "store-app"),
        &pilot_doc(PILOT_GRAPH, "ingest", "store-ingest"),
    );
    let root = dir.path().canonicalize().unwrap();
    let reason = cfg
        .startup_refusal(Some(&pilot(&root, true, PILOT_GRAPH)))
        .expect("enabled graph_sets must refuse startup");
    assert_eq!(reason, STARTUP_REFUSAL);
    assert_eq!(reason.lines().count(), 1);
    assert!(reason.contains("graph_sets") && reason.contains("refusing"));

    // A preflight refusal names its own reason, still on one line.
    let mut bad = cfg.clone();
    bad.installed_policy = "policy-v2".into();
    let reason = bad.startup_refusal(None).unwrap();
    assert!(reason.contains("installed policy"), "{reason}");
    assert_eq!(reason.lines().count(), 1);

    // Disabled: no refusal and nothing is read (the catalog path is absent).
    let mut off = cfg;
    off.enabled = false;
    off.catalog_file = root.join("does-not-exist.json");
    assert!(off.startup_refusal(None).is_none());
}

/// aegis-n0wc.5.2: `[quipu.graph_sets.reads]` is optional (absent = no read
/// mount), takes the pilot's read keys, and refuses unknown or missing keys.
#[test]
fn reads_table_is_optional_and_takes_the_pilot_read_keys() {
    let root = Path::new("/srv/owners");
    assert!(
        parse(&table(root))
            .unwrap()
            .graph_sets
            .unwrap()
            .reads
            .is_none()
    );
    let reads = "[quipu.graph_sets.reads]\npolicy_file = \"/srv/read/policy.db\"\n\
                 owner = \"urn:owner\"\nregistry_file = \"/srv/read/registry.json\"\n\
                 timeout_secs = 10\nconcurrency = 1\nruntime_mib = 16\nview_mib = 16\n\
                 query_mib = 64\nmax_response_mib = 1\n";
    let cfg = parse(&format!("{}{reads}", table(root)))
        .unwrap()
        .graph_sets
        .unwrap();
    let parsed = cfg.reads.expect("reads table");
    assert_eq!(parsed.concurrency, 1);
    assert_eq!(parsed.owner, "urn:owner");
    let unknown = format!("{}{reads}principal = \"urn:x\"\n", table(root));
    assert!(parse(&unknown).is_err(), "unknown read key refuses");
    let missing = format!("{}{}", table(root), reads.replace("concurrency = 1\n", ""));
    assert!(parse(&missing).is_err(), "missing read key refuses");
}
