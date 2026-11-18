use super::*;
use crate::graph_catalog::{Budget, Catalog, Route, WriteRequest};
use crate::store::graph_owner::Owner;
use serde_json::json;
use std::sync::Arc;

const GRAPH: &str = "https://example.test/graphs/ingest";
const TAG: &str = "snapshot:code:repo";
const TS: &str = "2026-11-11T00:00:00Z";
const CODE: &str = "<urn:slice:file> <urn:slice:defines> <urn:slice:symbol> . \
    <urn:slice:file> <urn:slice:path> \"src/lib.rs\" . \
    <urn:slice:file> <urn:slice:changed> \"2026-11-01\"^^<http://www.w3.org/2001/XMLSchema#date> . \
    <urn:slice:file> <urn:slice:note> \"hello\"@en .";

fn snapshot(store: &mut Store, key: &str, turtle: &str) {
    crate::tool_knot(
        store,
        &json!({"turtle": turtle, "replace_snapshot": true, "snapshot": key, "source": "slice-test"}),
    )
    .unwrap();
}

fn rows(store: &Store, graph: Option<&str>, prefix: &str) -> Vec<String> {
    let mut input = json!({
        "query": format!(
            "SELECT ?s ?p ?o WHERE {{ ?s ?p ?o FILTER(STRSTARTS(STR(?s), \"{prefix}\")) }}"
        )
    });
    if let Some(graph) = graph {
        input["graph"] = json!(graph);
    }
    let out = crate::tool_query(store, &input).unwrap();
    let mut rows: Vec<String> = out["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect();
    rows.sort();
    rows
}

fn owner(root: &std::path::Path) -> Owner {
    let path = root.join("ingest.db");
    let path = path.to_str().unwrap();
    Store::open(path).unwrap().graph_create(GRAPH).unwrap();
    Store::adopt(path).unwrap();
    let store = Store::open(path).unwrap();
    let route = Route {
        graph: GRAPH.into(),
        file_id: "ingest".into(),
        store_id: store.store_id().unwrap(),
        log_epoch: store.log_epoch().unwrap(),
        generation: 4,
        policy: "policy-v1".into(),
    };
    let catalog = Arc::new(
        Catalog::new(
            7,
            "legacy-store",
            vec![route],
            Budget {
                max_owners: 1,
                queued_per_owner: 1,
                readers_per_owner: 1,
                max_queued: 1,
                max_connections: 4,
            },
        )
        .unwrap(),
    );
    Owner::prepare(catalog, root, GRAPH, store, "policy-v1").unwrap()
}

#[test]
fn moved_slice_reads_back_identically_and_stays_producer_owned() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut legacy = Store::open(root.join("legacy.db").to_str().unwrap()).unwrap();
    snapshot(&mut legacy, "code:repo", CODE);
    snapshot(
        &mut legacy,
        "docs",
        "<urn:slice:doc> <urn:slice:path> \"README\" .",
    );

    // Immutable baseline, taken before anything moves.
    let baseline_rows = rows(&legacy, None, "urn:slice:file");
    let docs_rows = rows(&legacy, None, "urn:slice:doc");
    assert_eq!(baseline_rows.len(), 4);
    let slice = legacy
        .export_routed_slice(TAG, crate::schema::ROOT_GRAPH_IRI)
        .unwrap();
    assert_eq!(slice.len(), 4);

    let owner = owner(&root);
    owner.import_slice(&slice, TAG, TS).unwrap();
    legacy.fence_routed_graph(GRAPH, 7, 4).unwrap();
    let retracted = legacy.cut_over_routed_source(TAG, GRAPH, 7, 4, TS).unwrap();
    assert_eq!(retracted, slice.len(), "copied set equals retracted set");

    // Parity: canonical fact multiset and SPARQL result rows.
    let moved = owner.read(|s| s.export_routed_slice(TAG, GRAPH)).unwrap();
    assert_eq!(slice_multiset(&moved), slice_multiset(&slice));
    let owner_rows = owner
        .read(|s| Ok(rows(s, Some(GRAPH), "urn:slice:file")))
        .unwrap();
    assert_eq!(owner_rows, baseline_rows, "routed read equals the baseline");

    // Controls: moved facts left ROOT; the unmoved producer stayed in ROOT and
    // never reached the owner.
    assert!(rows(&legacy, None, "urn:slice:file").is_empty());
    assert_eq!(rows(&legacy, None, "urn:slice:doc"), docs_rows);
    assert!(
        owner
            .read(|s| Ok(rows(s, Some(GRAPH), "urn:slice:doc")))
            .unwrap()
            .is_empty()
    );

    // A second import would double the slice.
    assert!(owner.import_slice(&slice, TAG, TS).is_err());

    // The producer's next replace in the owner replaces the copied facts.
    let request = WriteRequest {
        graph: Some(GRAPH),
        catalog_revision: 7,
        owner_generation: Some(4),
        policy: "policy-v1",
        signed: false,
        cross_graph_context: false,
    };
    owner
        .knot(
            &request,
            &json!({
                "graph": GRAPH,
                "turtle": "<urn:slice:file> <urn:slice:path> \"src/main.rs\" .",
                "replace_snapshot": true,
                "snapshot": "code:repo",
                "source": "slice-test",
            }),
        )
        .unwrap();
    let after = owner
        .read(|s| Ok(rows(s, Some(GRAPH), "urn:slice:file")))
        .unwrap();
    assert_eq!(after.len(), 1, "replace closed the copied facts: {after:?}");
    assert!(after[0].contains("src/main.rs"), "{after:?}");
}

#[test]
fn import_refuses_root_and_a_bad_tag() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut legacy = Store::open(root.join("legacy.db").to_str().unwrap()).unwrap();
    snapshot(&mut legacy, "code:repo", CODE);
    let slice = legacy
        .export_routed_slice(TAG, crate::schema::ROOT_GRAPH_IRI)
        .unwrap();
    assert!(
        legacy
            .import_routed_slice(&slice, TAG, crate::schema::ROOT_GRAPH_IRI, TS)
            .is_err()
    );
    let owner = owner(&root);
    assert!(owner.import_slice(&slice, "code:repo", TS).is_err());
    assert!(owner.import_slice(&slice, "snapshot:", TS).is_err());
}
