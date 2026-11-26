use super::*;
use serde_json::{Value, json};

fn document() -> Value {
    json!({
        "version": 1,
        "revision": 7,
        "legacy_store_id": "legacy-store",
        "budget": {
            "max_owners": 1,
            "queued_per_owner": 1,
            "readers_per_owner": 1,
            "max_queued": 1,
            "max_connections": 3
        },
        "routes": [{
            "graph": "https://example.org/graph/code",
            "file_id": "ingest",
            "store_id": "owner-store",
            "log_epoch": "epoch-1",
            "generation": 4,
            "policy": "policy-v1"
        }],
        "legacy_participant": {
            "graph": crate::schema::ROOT_GRAPH_IRI,
            "file_id": "legacy",
            "store_id": "legacy-store",
            "log_epoch": "epoch-0",
            "generation": 1,
            "policy": "legacy-policy"
        }
    })
}

fn load(doc: &Value) -> Result<Catalog> {
    Catalog::from_json(doc.to_string().as_bytes())
}

#[test]
fn exact_document_builds_the_validated_catalog() {
    let catalog = load(&document()).unwrap();
    assert_eq!(catalog.revision(), 7);
    let route = catalog.routes().next().unwrap();
    assert_eq!(route.graph, "https://example.org/graph/code");
    assert_eq!(route.generation, 4);
    let legacy = catalog.legacy_participant().unwrap();
    assert_eq!(legacy.file_id, "legacy");
    assert_eq!(catalog.budget().readers_per_owner, 1);
}

#[test]
fn unknown_missing_or_versioned_fields_refuse() {
    let mut doc = document();
    doc["routes"][0]["path"] = json!("/etc/passwd");
    assert!(load(&doc).is_err(), "unknown route field");
    let mut doc = document();
    doc.as_object_mut().unwrap().remove("legacy_participant");
    assert!(load(&doc).is_err(), "legacy participant is mandatory");
    let mut doc = document();
    doc["version"] = json!(2);
    assert!(load(&doc).is_err(), "unsupported version");
    assert!(Catalog::from_json(b"{not json").is_err());
}

#[test]
fn pins_that_disagree_refuse_whole() {
    let mut doc = document();
    doc["legacy_participant"]["store_id"] = json!("another-store");
    assert!(
        load(&doc).is_err(),
        "legacy participant must match the legacy store"
    );
    let mut doc = document();
    doc["revision"] = json!(0);
    assert!(load(&doc).is_err(), "revision zero");
    let mut doc = document();
    doc["legacy_participant"]["file_id"] = json!("ingest");
    assert!(load(&doc).is_err(), "one file cannot be two participants");
    let mut doc = document();
    doc["routes"][0]["policy"] = json!("");
    assert!(load(&doc).is_err(), "policy pin required");
    let mut doc = document();
    doc["routes"][0]["file_id"] = json!("../escape");
    assert!(load(&doc).is_err(), "file id is opaque, never a path");
}

#[cfg(unix)]
#[test]
fn load_file_refuses_writable_or_oversized_files() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("catalog.json");
    std::fs::write(&path, document().to_string()).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(Catalog::load_file(&path).unwrap().revision(), 7);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o664)).unwrap();
    assert!(Catalog::load_file(&path).is_err(), "group-writable catalog");
    std::fs::write(&path, " ".repeat(64 * 1024 + 1)).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(Catalog::load_file(&path).is_err(), "oversized catalog");
    assert!(Catalog::load_file(dir.path()).is_err(), "a directory");
}
