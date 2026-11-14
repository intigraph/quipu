//! Qualify physical-copy boundaries without inventing a logical graph remap.
use super::*;
use crate::pack::{PackOptions, pack, pack_to_bytes};
use crate::transaction_auth::with_identity;

const AT: &str = "2026-11-08T02:00:00Z";

#[test]
fn raw_native_fork_refuses_atomically_instead_of_rewriting_source_scope() {
    let (mut store, graph) = super::tests::loaded_event(crate::schema::ROOT_GRAPH_IRI, "urn:event");
    let before = writer::read_log(&store, graph, "urn:fluent", Policy::Sum).unwrap();
    let facts = store.current_facts_in_graph(graph).unwrap();
    let tx = store.latest_tx_id().unwrap();
    let result = with_identity(Some(super::carrier_tests::importer()), || {
        store.fork_create("native-copy", tx, AT, Some("urn:forged-source-actor"))
    });
    assert!(
        result.is_err(),
        "a physical fork cannot grant a new logical source scope"
    );
    assert_eq!(store.latest_tx_id().unwrap(), tx);
    assert_eq!(store.current_facts_in_graph(graph).unwrap(), facts);
    assert!(store.fork_lookup("native-copy").unwrap().is_none());
    assert_eq!(
        writer::read_log(&store, graph, "urn:fluent", Policy::Sum).unwrap(),
        before
    );
    let permits: i64 = store
        .conn
        .query_row("SELECT count(*) FROM fluent_write_permits", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(permits, 0);
}

#[test]
fn native_pendant_refuses_without_destroying_raw_history_or_shipping_partial_file() {
    let (store, graph) = super::tests::loaded_event(crate::schema::ROOT_GRAPH_IRI, "urn:event");
    let before = writer::read_log(&store, graph, "urn:fluent", Policy::Sum).unwrap();
    let tx = store.latest_tx_id().unwrap();
    let opts = PackOptions {
        destination: crate::share::ShareDestination::Internal,
        ..Default::default()
    };
    assert!(pack_to_bytes(&store, crate::schema::ROOT_GRAPH_IRI, &opts, AT).is_err());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("native.pendant.db");
    assert!(
        pack(
            &store,
            crate::schema::ROOT_GRAPH_IRI,
            path.to_str().unwrap(),
            &opts,
            AT
        )
        .is_err()
    );
    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert!(
        leftovers.is_empty(),
        "a refused producer must remove its building file and SQLite siblings: {leftovers:?}"
    );
    assert_eq!(store.latest_tx_id().unwrap(), tx);
    assert_eq!(
        writer::read_log(&store, graph, "urn:fluent", Policy::Sum).unwrap(),
        before
    );
    // Positive control: this same producer and destination can carry ordinary data.
    let mut ordinary = Store::open_in_memory().unwrap();
    crate::rdf::ingest_rdf(
        &mut ordinary,
        b"<urn:ordinary> <urn:value> \"ok\" .".as_slice(),
        oxrdfio::RdfFormat::NTriples,
        None,
        AT,
        None,
        None,
    )
    .unwrap();
    pack(
        &ordinary,
        crate::schema::ROOT_GRAPH_IRI,
        path.to_str().unwrap(),
        &opts,
        AT,
    )
    .unwrap();
    let (_, _, valid) = crate::pack::verify(path.to_str().unwrap()).unwrap();
    assert!(valid);
}

#[test]
fn pending_carrier_survives_fork_drop_and_disk_reopen_without_event_adoption() {
    let mut store = Store::open_in_memory().unwrap();
    let request = super::carrier_tests::request(&[super::carrier_tests::proof(
        crate::schema::ROOT_GRAPH_IRI,
    )]);
    crate::share_import::import_share(&mut store, &request, AT, None).unwrap();
    let tx = store.latest_tx_id().unwrap();
    let fork = store.fork_create("pending-only", tx, AT, None).unwrap();
    assert!(store.current_facts_in_graph(fork.g).unwrap().is_empty());
    store.fork_drop("pending-only", AT).unwrap();
    assert_eq!(
        crate::share_fluent::imported_proofs(&store, &request.manifest.share_id).unwrap(),
        Some(request.manifest.fluent_proofs.clone())
    );
    assert!(store.lookup("urn:carried-event").unwrap().is_none());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pending.db");
    std::fs::write(&path, store.serialize_db().unwrap()).unwrap();
    drop(store);
    let store = Store::open(path.to_str().unwrap()).unwrap();
    assert_eq!(
        crate::share_fluent::imported_proofs(&store, &request.manifest.share_id).unwrap(),
        Some(request.manifest.fluent_proofs)
    );
    assert!(store.lookup("urn:carried-event").unwrap().is_none());
    assert_eq!(
        store.fork_lookup("pending-only").unwrap().unwrap().status,
        "dropped"
    );
}
