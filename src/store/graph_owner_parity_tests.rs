//! Consumer parity across the cutover through the strict composed reader:
//! the same query over ROOT plus the routed graph answers identically before
//! and after a producer's facts move, with controls that fail if routing were
//! ignored or only one file were read.
use super::*;
use crate::{
    read_authorization::{ComposedReadAdmission, ReadOperation},
    schema::ROOT_GRAPH_IRI,
};

const TAG: &str = "snapshot:code:repo";
const CODE: &str = "<urn:parity:file> <urn:parity:defines> <urn:parity:symbol> . \
    <urn:parity:file> <urn:parity:path> \"src/lib.rs\" . \
    <urn:parity:file> <urn:parity:changed> \"2026-11-01\"^^<http://www.w3.org/2001/XMLSchema#date> . \
    <urn:parity:file> <urn:parity:note> \"hello\"@en .";
const DOCS: &str = "<urn:parity:doc> <urn:parity:mentions> <urn:parity:symbol> .";

fn snapshot(store: &mut Store, key: &str, turtle: &str) {
    crate::tool_knot(
        store,
        &json!({"turtle": turtle, "replace_snapshot": true, "snapshot": key, "source": "parity"}),
    )
    .unwrap();
}

/// Sorted W3C bindings of `query` at one strict common cut.
fn answer(
    legacy: &Owner,
    routed: &Owner,
    admission: &ComposedReadAdmission,
    query: &str,
) -> Vec<String> {
    let (policy, identity) = composition_authority(&[ROOT_GRAPH_IRI, &routed.route.graph]);
    let policy = policy.snapshot().unwrap();
    let deadline = strict_deadline();
    Owner::read_strict(&[routed, legacy], deadline, |reads| {
        let view = policy
            .authorize(&identity, ReadOperation::Query, None, 2)?
            .open_composed_cut(reads, admission.try_reserve()?, 2, deadline)?;
        let out: Value = serde_json::from_slice(&view.query(query, false, 2)?).unwrap();
        let mut rows: Vec<String> = out["results"]["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .map(ToString::to_string)
            .collect();
        rows.sort();
        Ok(rows)
    })
    .unwrap()
}

#[test]
fn composed_consumer_query_is_unchanged_by_the_cutover() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let (mut legacy_store, mut pin) = fixture(root, "legacy");
    let (routed_store, route) = fixture(root, "routed");
    pin.graph = ROOT_GRAPH_IRI.into();
    snapshot(&mut legacy_store, "code:repo", CODE);
    snapshot(&mut legacy_store, "docs", DOCS);
    let catalog = Arc::new(
        Catalog::new(
            7,
            &pin.store_id,
            vec![route.clone()],
            Budget {
                max_owners: 1,
                queued_per_owner: 1,
                readers_per_owner: 1,
                max_queued: 1,
                max_connections: 3,
            },
        )
        .unwrap()
        .with_legacy_participant(pin)
        .unwrap(),
    );
    let admission =
        ComposedReadAdmission::new(catalog.clone(), 512, 3, 1, composition_reservation()).unwrap();
    let legacy = Owner::prepare_with_memory(
        catalog.clone(),
        root,
        ROOT_GRAPH_IRI,
        legacy_store,
        "policy-v1",
        admission.allocation(),
    )
    .unwrap();
    let routed = Owner::prepare_with_memory(
        catalog,
        root,
        &route.graph,
        routed_store,
        "policy-v1",
        admission.allocation(),
    )
    .unwrap();

    let g = &route.graph;
    let filter = "FILTER(STRSTARTS(STR(?s), \"urn:parity:\"))";
    let consumer = format!(
        "SELECT ?s ?p ?o FROM <{ROOT_GRAPH_IRI}> FROM NAMED <{g}> \
         WHERE {{ {{ ?s ?p ?o }} UNION {{ GRAPH <{g}> {{ ?s ?p ?o }} }} {filter} }}"
    );
    let root_only =
        format!("SELECT ?s ?p ?o FROM <{ROOT_GRAPH_IRI}> WHERE {{ ?s ?p ?o {filter} }}");
    let routed_only = format!(
        "SELECT ?s ?p ?o FROM <{ROOT_GRAPH_IRI}> FROM NAMED <{g}> WHERE {{ GRAPH <{g}> {{ ?s ?p ?o }} {filter} }}"
    );

    let before = answer(&legacy, &routed, &admission, &consumer);
    assert_eq!(before.len(), 5, "{before:?}");
    assert!(answer(&legacy, &routed, &admission, &routed_only).is_empty());

    // Move: export, import, fence, cut the producer over.
    let slice = legacy
        .writer
        .lock()
        .export_routed_slice(TAG, ROOT_GRAPH_IRI)
        .unwrap();
    routed
        .import_slice(&slice, TAG, "2026-11-11T00:00:00Z")
        .unwrap();
    {
        let mut writer = legacy.writer.lock();
        writer.fence_routed_graph(g, 7, 4).unwrap();
        assert_eq!(
            writer
                .cut_over_routed_source(TAG, g, 7, 4, "2026-11-11T00:00:00Z")
                .unwrap(),
            4
        );
    }

    let after = answer(&legacy, &routed, &admission, &consumer);
    assert_eq!(after, before, "consumer answer changed across the cutover");
    // Controls: each file alone no longer gives the consumer's answer.
    assert_eq!(
        answer(&legacy, &routed, &admission, &root_only).len(),
        1,
        "ROOT keeps only docs"
    );
    assert_eq!(
        answer(&legacy, &routed, &admission, &routed_only).len(),
        4,
        "routed holds the code"
    );
}
