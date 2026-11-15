//! Real disk reconnects retain pending source evidence without origin admission.
use super::carrier_tests::{proof, request};
use super::*;
use crate::share::{ShareDestination, ShareOptions, share};

fn pair(
    root: &std::path::Path,
    blocked: bool,
) -> (crate::share::ShareManifest, crate::share::ShareManifest) {
    let mut relay = Store::open_in_memory().unwrap();
    let opts = |proof: &Proof, parent| ShareOptions {
        no_shapes: true,
        queries: Some(Vec::new()),
        destination: ShareDestination::Internal,
        parent_share: parent,
        fluent_proofs: vec![serde_json::to_string(proof).unwrap()],
        ..Default::default()
    };
    let base = share(
        &relay,
        root.join("base").to_str().unwrap(),
        &opts(&proof(crate::schema::ROOT_GRAPH_IRI), None),
    )
    .unwrap();
    if blocked {
        crate::rdf::ingest_rdf(
            &mut relay,
            b"<urn:blocked> <urn:forbidden> \"value\" .".as_slice(),
            oxrdfio::RdfFormat::NTriples,
            None,
            "2026-11-08T00:00:00Z",
            None,
            None,
        )
        .unwrap();
    }
    let incoming = share(
        &relay,
        root.join("incoming").to_str().unwrap(),
        &opts(&proof("urn:graph-a"), Some(base.share_id.clone())),
    )
    .unwrap();
    (base, incoming)
}

#[test]
fn disk_reconnect_retains_both_carriers_without_adopting_or_rewriting_them() {
    let root = tempfile::tempdir().unwrap();
    let (base, incoming) = pair(root.path(), false);
    let (mut receiver, _) = super::tests::loaded_shapes(crate::schema::ROOT_GRAPH_IRI);
    let dir = root.path().join("incoming");
    let result = crate::share_merge::merge(
        &mut receiver,
        &dir,
        "2026-11-08T01:00:00Z",
        Some("urn:relay-actor"),
    )
    .unwrap();
    assert_eq!(result.outcome, "merged");
    for manifest in [&base, &incoming] {
        assert_eq!(
            crate::share_fluent::imported_proofs(&receiver, &manifest.share_id).unwrap(),
            Some(manifest.fluent_proofs.clone())
        );
    }
    assert!(receiver.lookup("urn:carried-event").unwrap().is_none());
    let tx = receiver.latest_tx_id().unwrap();
    let replay =
        crate::share_merge::merge(&mut receiver, &dir, "2026-11-08T02:00:00Z", None).unwrap();
    assert_eq!(replay.outcome, "already-applied");
    assert_eq!(receiver.latest_tx_id().unwrap(), tx);
    assert_eq!(
        crate::share_fluent::imported_proofs(&receiver, &incoming.share_id).unwrap(),
        Some(incoming.fluent_proofs)
    );
}

#[test]
fn normal_merge_gate_failure_rolls_back_pending_carriers_and_rdf_together() {
    let root = tempfile::tempdir().unwrap();
    let (base, incoming) = pair(root.path(), true);
    let mut receiver = Store::open_in_memory().unwrap();
    receiver
        .load_shapes(
            "deny",
            r#"
        @prefix sh: <http://www.w3.org/ns/shacl#> .
        <urn:deny> a sh:NodeShape; sh:targetNode <urn:blocked>;
          sh:property [ sh:path <urn:forbidden>; sh:maxCount 0 ] .
    "#,
            "2026-11-08T00:00:00Z",
        )
        .unwrap();
    let tx = receiver.latest_tx_id().unwrap();
    let result = crate::share_merge::merge(
        &mut receiver,
        &root.path().join("incoming"),
        "2026-11-08T01:00:00Z",
        None,
    );
    assert!(
        result.is_err(),
        "actual receiver SHACL must refuse the ordinary graph write"
    );
    assert_eq!(receiver.latest_tx_id().unwrap(), tx);
    assert!(receiver.lookup("urn:carried-event").unwrap().is_none());
    for manifest in [&base, &incoming] {
        assert!(
            crate::share_fluent::imported_proofs(&receiver, &manifest.share_id)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn hash_valid_overbound_carrier_refuses_the_merge_reader() {
    let r = request(&[]);
    let mut manifest = r.manifest;
    manifest.fluent_proofs = vec!["{}".into(); 65];
    manifest.share_id =
        crate::share::sha256(&crate::share::manifest_bytes(&manifest, false).unwrap());
    assert!(
        crate::share_merge::share_from_parts(
            std::path::Path::new("carrier-test"),
            manifest,
            &r.export_ntriples,
            r.shapes_turtle
        )
        .is_err()
    );
}

#[test]
fn pending_proofs_are_snapshot_and_journal_content_but_local_auth_is_not() {
    use crate::share_import::import_share;
    use crate::transaction_auth::with_identity;
    let mut store = Store::open_in_memory().unwrap();
    let first = request(&[proof(crate::schema::ROOT_GRAPH_IRI)]);
    import_share(&mut store, &first, "2026-11-08T00:00:00Z", None).unwrap();
    crate::store_stream::initialize(&store).unwrap();
    let second = request(&[proof("urn:graph-a")]);
    with_identity(Some(super::carrier_tests::importer()), || {
        import_share(&mut store, &second, "2026-11-08T01:00:00Z", None)
    })
    .unwrap();
    let auth: i64 = store
        .conn
        .query_row("SELECT count(*) FROM transaction_auth", [], |r| r.get(0))
        .unwrap();
    assert!(
        auth > 0,
        "real authenticated receiver transaction is non-vacuous"
    );
    let batch = crate::store_stream::read_revision(&store, 0, 1000).unwrap();
    assert!(
        batch
            .changes
            .iter()
            .any(|c| c.table == "share_fluent_carriers")
    );
    assert!(
        !batch
            .changes
            .iter()
            .any(|c| c.table == "transaction_auth" || c.table == "fluent_origin_receipts")
    );
    let mut bytes = Vec::new();
    let snapshot =
        crate::store_stream::write_snapshot(&store, &mut bytes, 4 * 1024 * 1024).unwrap();
    assert!(
        !snapshot
            .header
            .catalog
            .iter()
            .any(|t| t.name == "transaction_auth" || t.name == "fluent_origin_receipts")
    );
    let table = snapshot
        .header
        .catalog
        .iter()
        .find(|t| t.name == "share_fluent_carriers")
        .unwrap();
    let column = table
        .columns
        .iter()
        .position(|c| c.name == "manifest")
        .unwrap();
    let mut recovered = std::collections::BTreeMap::new();
    for line in bytes.split(|c| *c == b'\n').filter(|line| !line.is_empty()) {
        let value: serde_json::Value = serde_json::from_slice(line).unwrap();
        if value.get("table").and_then(|v| v.as_str()) != Some("share_fluent_carriers") {
            continue;
        }
        let row: crate::store_replica::SnapshotRow = serde_json::from_value(value).unwrap();
        let manifest: crate::share::ShareManifest =
            serde_json::from_str(row.cells[column].value.as_str().unwrap()).unwrap();
        recovered.insert(manifest.share_id, manifest.fluent_proofs);
    }
    for r in [&first, &second] {
        assert_eq!(
            recovered.get(&r.manifest.share_id),
            Some(&r.manifest.fluent_proofs)
        );
    }
    assert!(store.lookup("urn:carried-event").unwrap().is_none());
}

#[test]
fn decision_merge_retains_carriers_only_on_apply_not_dry_run() {
    let root = tempfile::tempdir().unwrap();
    let (base, incoming) = pair(root.path(), false);
    let mut receiver = Store::open_in_memory().unwrap();
    let dir = root.path().join("incoming");
    let file = crate::share_merge_decisions::emit(&receiver, &dir).unwrap();
    assert!(file.rows.is_empty());
    let bytes = serde_json::to_vec(&file).unwrap();
    let tx = receiver.latest_tx_id().unwrap();
    crate::share_merge_decisions::dry_run(&mut receiver, &dir, &file, &bytes, "urn:reviewer")
        .unwrap();
    assert_eq!(receiver.latest_tx_id().unwrap(), tx);
    for manifest in [&base, &incoming] {
        assert!(
            crate::share_fluent::imported_proofs(&receiver, &manifest.share_id)
                .unwrap()
                .is_none()
        );
    }
    crate::share_merge_decisions::apply(
        &mut receiver,
        &dir,
        &file,
        &bytes,
        "urn:reviewer",
        "2026-11-08T01:00:00Z",
        None,
    )
    .unwrap();
    for manifest in [&base, &incoming] {
        assert_eq!(
            crate::share_fluent::imported_proofs(&receiver, &manifest.share_id).unwrap(),
            Some(manifest.fluent_proofs.clone())
        );
    }
    assert!(receiver.lookup("urn:carried-event").unwrap().is_none());
}
