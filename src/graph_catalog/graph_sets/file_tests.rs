use super::*;
use serde_json::{Value, json};

const P: &str = "https://example.org/project/p";

fn route(graph: &str, file_id: &str) -> Value {
    json!({
        "graph": graph,
        "file_id": file_id,
        "store_id": format!("store-{file_id}"),
        "log_epoch": format!("epoch-{file_id}"),
        "generation": 3,
        "policy": "policy-v1"
    })
}

/// A tracker-shaped set: project data, ephemeral, provenance, archive and an
/// operations log share one file because one write spans them atomically.
fn members() -> Vec<String> {
    [
        P.to_string(),
        format!("{P}#seeds-ephemeral"),
        format!("{P}#seeds-provenance"),
        format!("{P}#seeds-archive"),
        format!("{P}/operations-log"),
    ]
    .into()
}

fn document() -> Value {
    json!({
        "version": 1,
        "revision": 7,
        "legacy_store_id": "store-legacy",
        "max_graphs": 8,
        "budget": {
            "max_owners": 2,
            "queued_per_owner": 1,
            "readers_per_owner": 1,
            "max_queued": 2,
            "max_connections": 6
        },
        "sets": [
            {"owner": route(P, "tracker"), "graphs": members()},
            {"owner": route("https://example.org/graph/hot", "hot"),
             "graphs": ["https://example.org/graph/hot"]}
        ],
        "legacy": {
            "participant": route(crate::schema::ROOT_GRAPH_IRI, "legacy"),
            "graphs": [crate::schema::ROOT_GRAPH_IRI, "https://example.org/graph/reference"]
        }
    })
}

fn load(doc: &Value) -> Result<GraphSetCatalog> {
    GraphSetCatalog::from_json(doc.to_string().as_bytes())
}

#[test]
fn exact_document_builds_one_physical_owner_per_set() {
    let catalog = load(&document()).unwrap();
    assert_eq!(catalog.physical().revision(), 7);
    assert_eq!(catalog.physical().routes().count(), 2);
    let mut expected = members();
    expected.sort();
    for member in &expected {
        assert_eq!(catalog.owner_graphs(member).unwrap(), expected);
    }
    assert_eq!(
        catalog
            .owner_graphs("https://example.org/graph/reference")
            .unwrap(),
        [
            "https://example.org/graph/reference",
            crate::schema::ROOT_GRAPH_IRI
        ]
    );
    let plan = catalog
        .plan_read(&[P, "https://example.org/graph/hot"])
        .unwrap();
    assert_eq!(plan.len(), 2, "two files, two snapshots");
}

#[test]
fn unknown_missing_or_versioned_fields_refuse() {
    let mut doc = document();
    doc["sets"][0]["prefix"] = json!(format!("{P}/messages/"));
    assert!(load(&doc).is_err(), "no prefix membership field exists");
    let mut doc = document();
    doc["sets"][0]["owner"]["path"] = json!("/etc/passwd");
    assert!(load(&doc).is_err(), "unknown route field");
    let mut doc = document();
    doc.as_object_mut().unwrap().remove("legacy");
    assert!(load(&doc).is_err(), "legacy membership is mandatory");
    let mut doc = document();
    doc.as_object_mut().unwrap().remove("max_graphs");
    assert!(load(&doc).is_err(), "membership bound is mandatory");
    let mut doc = document();
    doc["version"] = json!(2);
    assert!(load(&doc).is_err(), "future version");
    assert!(GraphSetCatalog::from_json(b"{").is_err(), "malformed");
}

#[test]
fn a_single_graph_catalog_document_is_not_a_graph_set_document() {
    let single = json!({
        "version": 1,
        "revision": 7,
        "legacy_store_id": "store-legacy",
        "budget": document()["budget"].clone(),
        "routes": [route(P, "tracker")],
        "legacy_participant": route(crate::schema::ROOT_GRAPH_IRI, "legacy")
    });
    assert!(load(&single).is_err());
}

#[test]
fn membership_violations_refuse_the_whole_document() {
    // Owner graph absent from its own membership.
    let mut doc = document();
    doc["sets"][0]["graphs"] = json!([format!("{P}#seeds-ephemeral")]);
    assert!(load(&doc).is_err());
    // One logical graph in two sets (would split an atomic write).
    let mut doc = document();
    doc["sets"][1]["graphs"] = json!(["https://example.org/graph/hot", P]);
    assert!(load(&doc).is_err());
    // A set member also kept on the legacy file.
    let mut doc = document();
    doc["legacy"]["graphs"] = json!([crate::schema::ROOT_GRAPH_IRI, P]);
    assert!(load(&doc).is_err());
    // ROOT cannot move into a routed set.
    let mut doc = document();
    doc["sets"][1]["graphs"] = json!([
        "https://example.org/graph/hot",
        crate::schema::ROOT_GRAPH_IRI
    ]);
    assert!(load(&doc).is_err());
    // Legacy membership must name ROOT explicitly.
    let mut doc = document();
    doc["legacy"]["graphs"] = json!(["https://example.org/graph/reference"]);
    assert!(load(&doc).is_err());
    // Not an IRI.
    let mut doc = document();
    doc["sets"][0]["graphs"][1] = json!("not an iri");
    assert!(load(&doc).is_err());
    // Bound counts logical members, not files.
    let mut doc = document();
    doc["max_graphs"] = json!(7);
    assert!(load(&doc).is_err());
    // Two sets cannot alias one file.
    let mut doc = document();
    doc["sets"][1]["owner"]["file_id"] = json!("tracker");
    assert!(load(&doc).is_err());
}

#[test]
fn file_loader_refuses_group_writable_directory_and_oversized_files() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("graph-sets.json");
    std::fs::write(&path, document().to_string()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(GraphSetCatalog::load_file(&path).is_ok());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o664)).unwrap();
        assert!(GraphSetCatalog::load_file(&path).is_err(), "group writable");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            GraphSetCatalog::load_file(temp.path()).is_err(),
            "directory"
        );
    }
    let big = temp.path().join("big.json");
    std::fs::write(&big, vec![b' '; (MAX_BYTES + 1) as usize]).unwrap();
    assert!(GraphSetCatalog::load_file(&big).is_err(), "oversized");
}
