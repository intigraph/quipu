//! Pending bytes travel; original acceptance receipts never become remote grants.
use super::*;
use crate::share::{ShareDestination, ShareOptions, share_payload};

fn payload(request: &Pending) -> crate::share::SharePayload {
    share_payload(
        &Store::open_in_memory().unwrap(),
        &ShareOptions {
            no_shapes: true,
            queries: Some(Vec::new()),
            destination: ShareDestination::Internal,
            observable_configs: vec![String::from_utf8(request.bytes().to_vec()).unwrap()],
            ..Default::default()
        },
        crate::share::SHARE_PAYLOAD_MAX_BYTES,
    )
    .unwrap()
}

#[test]
fn pending_config_storage_is_exact_idempotent_and_survives_reopen_without_admission() {
    let request = pending("urn:derived", &["urn:base"]);
    let mut raw = request.bytes().to_vec();
    raw.push(b' ');
    let request = Pending::decode(&raw).unwrap();
    let store = loaded();
    let before = fingerprint(&store);
    request.retain(&store).unwrap();
    let changes = store.conn.total_changes();
    request.retain(&store).unwrap();
    assert_eq!(store.conn.total_changes(), changes);
    assert_eq!(fingerprint(&store), before);
    assert!(store.lookup("urn:derived").unwrap().is_none());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pending.db");
    std::fs::write(&path, store.serialize_db().unwrap()).unwrap();
    drop(store);
    let store = Store::open(path.to_str().unwrap()).unwrap();
    assert_eq!(
        Pending::load(&store, &request.id())
            .unwrap()
            .unwrap()
            .bytes(),
        raw
    );
    assert!(request.receipt(&store, 0).unwrap().is_none());
    store
        .conn
        .execute(
            "UPDATE fluent_pending_configs SET request=X'00' WHERE id=?1",
            [request.id()],
        )
        .unwrap();
    assert!(Pending::load(&store, &request.id()).is_err());
}

#[test]
fn pending_config_share_identity_binds_bytes_and_ordinary_import_keeps_them_pending() {
    let request = pending("urn:derived", &["urn:base"]);
    let source = payload(&request);
    let mut imported: crate::share_import::ShareImportRequest =
        serde_json::from_value(serde_json::json!({
            "manifest": source.manifest, "export_ntriples": source.files["export.nt"],
            "shapes_turtle": source.files["shapes.ttl"], "actor":"root", "source":"urn:relay"
        }))
        .unwrap();
    let mut receiver = loaded();
    crate::share_import::import_share(&mut receiver, &imported, AT, Some("root")).unwrap();
    assert_eq!(
        Pending::load(&receiver, &request.id())
            .unwrap()
            .unwrap()
            .bytes(),
        request.bytes()
    );
    assert!(receiver.lookup("urn:derived").unwrap().is_none());
    assert!(request.receipt(&receiver, 0).unwrap().is_none());
    let approval = approve(&receiver, &request);
    assert!(
        accept(&mut receiver, &request, &approval).is_err(),
        "transport never installs config scope"
    );
    imported.manifest.observable_configs.clear();
    assert!(
        crate::share_import::verify_share(&imported).is_err(),
        "stripping requests changes share identity"
    );
    let relay = Store::open_in_memory().unwrap();
    request.retain(&relay).unwrap();
    let ordinary = share_payload(
        &relay,
        &ShareOptions {
            no_shapes: true,
            queries: Some(Vec::new()),
            destination: ShareDestination::Internal,
            ..Default::default()
        },
        crate::share::SHARE_PAYLOAD_MAX_BYTES,
    )
    .unwrap();
    assert!(
        serde_json::to_value(&ordinary.manifest)
            .unwrap()
            .get("observable_configs")
            .is_none(),
        "no auto-discovery or legacy field"
    );
}

#[test]
fn pending_config_pendant_hash_materialization_and_tamper_controls() {
    let request = pending("urn:derived", &["urn:base"]);
    let carrier = payload(&request).manifest;
    let (manifest, bytes) = crate::pack::pack_to_bytes_with_carriers(
        &Store::open_in_memory().unwrap(),
        crate::schema::ROOT_GRAPH_IRI,
        &crate::pack::PackOptions {
            destination: ShareDestination::Internal,
            ..Default::default()
        },
        AT,
        std::slice::from_ref(&carrier),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pending.db");
    std::fs::write(&path, &bytes).unwrap();
    assert!(crate::pack::verify(path.to_str().unwrap()).unwrap().2);
    let destination = dir.path().join("ordinary.db");
    assert!(
        crate::pack::unpack_verified(
            path.to_str().unwrap(),
            destination.to_str().unwrap(),
            &Default::default(),
            AT
        )
        .is_err()
    );
    assert!(!destination.exists());
    let mut receiver = loaded();
    let report = crate::pack::materialize_pending_carriers(
        path.to_str().unwrap(),
        &mut receiver,
        ShareDestination::Internal,
    )
    .unwrap();
    assert_eq!(report.content_hash, manifest.content_hash);
    assert_eq!(
        Pending::load(&receiver, &request.id())
            .unwrap()
            .unwrap()
            .bytes(),
        request.bytes()
    );
    assert!(receiver.lookup("urn:derived").unwrap().is_none());
    assert!(request.receipt(&receiver, 0).unwrap().is_none());
    let changes = receiver.conn.total_changes();
    crate::pack::materialize_pending_carriers(
        path.to_str().unwrap(),
        &mut receiver,
        ShareDestination::Internal,
    )
    .unwrap();
    assert_eq!(receiver.conn.total_changes(), changes);
    for action in ["strip", "edit"] {
        std::fs::write(&path, &bytes).unwrap();
        let conn = rusqlite::Connection::open(&path).unwrap();
        if action == "strip" {
            conn.execute("DELETE FROM share_fluent_carriers", [])
                .unwrap();
        } else {
            let mut changed = carrier.clone();
            changed.observable_configs[0].push(' ');
            conn.execute(
                "UPDATE share_fluent_carriers SET manifest=?1",
                [
                    String::from_utf8(crate::share::manifest_bytes(&changed, true).unwrap())
                        .unwrap(),
                ],
            )
            .unwrap();
        }
        drop(conn);
        assert!(!matches!(
            crate::pack::verify(path.to_str().unwrap()),
            Ok((_, _, true))
        ));
    }
}

#[test]
fn pending_config_actual_receipt_preserves_raw_approval_and_is_sql_immutable() {
    let mut store = loaded();
    let request = pending("urn:derived", &["urn:base"]);
    let approval = approve(&store, &request);
    let mut wire = approval.bytes().unwrap();
    wire.push(b' ');
    let approval = Approval::decode(&wire).unwrap();
    govern(&mut store, &approval);
    let tx = accept(&mut store, &request, &approval).unwrap();
    let receipt = request.receipt(&store, 0).unwrap().unwrap();
    assert_eq!(receipt.tx, tx);
    assert_eq!(receipt.request, request.bytes());
    assert_eq!(receipt.approval, wire);
    assert_eq!(store.transaction_auth(tx).unwrap(), Some(identity()));
    assert_eq!(accept(&mut store, &request, &approval).unwrap(), tx);
    for sql in [
        "UPDATE fluent_config_receipts SET approval=X'00'",
        "DELETE FROM fluent_config_receipts",
    ] {
        assert!(store.conn.execute(sql, []).is_err());
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipt.db");
    std::fs::write(&path, store.serialize_db().unwrap()).unwrap();
    drop(store);
    let independent = rusqlite::Connection::open(&path).unwrap();
    assert!(
        independent
            .execute("UPDATE fluent_config_receipts SET request=X'00'", [])
            .is_err()
    );
    drop(independent);
    let mut store = Store::open(path.to_str().unwrap()).unwrap();
    store.set_base_ns(crate::namespace::DEFAULT_BASE_NS);
    assert_eq!(request.receipt(&store, 0).unwrap().unwrap(), receipt);
    assert_eq!(accept(&mut store, &request, &approval).unwrap(), tx);
}

#[test]
fn pending_config_failed_poststate_rolls_back_descriptor_and_original_receipt() {
    let mut store = loaded();
    store.load_shapes("deny-derived",r#"@prefix sh: <http://www.w3.org/ns/shacl#> . <urn:deny> a sh:NodeShape; sh:targetNode <urn:derived>; sh:property [ sh:path <https://w3id.org/intigraph/quipu/ns#computedBy>; sh:maxCount 0 ] ."#,AT).unwrap();
    let request = pending("urn:derived", &["urn:base"]);
    let approval = approve(&store, &request);
    govern(&mut store, &approval);
    let before = fingerprint(&store);
    assert!(accept(&mut store, &request, &approval).is_err());
    assert_eq!(fingerprint(&store), before);
    assert!(request.receipt(&store, 0).unwrap().is_none());
    let count: i64 = store
        .conn
        .query_row("SELECT count(*) FROM fluent_write_permits", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn pending_config_snapshot_and_journal_transfer_pending_but_exclude_actual_receipt() {
    let mut store = loaded();
    crate::store_stream::initialize(&store).unwrap();
    let request = pending("urn:derived", &["urn:base"]);
    request.retain(&store).unwrap();
    let approval = approve(&store, &request);
    govern(&mut store, &approval);
    accept(&mut store, &request, &approval).unwrap();
    assert!(
        request.receipt(&store, 0).unwrap().is_some(),
        "non-vacuous receipt exclusion"
    );
    let batch = crate::store_stream::read_revision(&store, 0, 1000).unwrap();
    assert!(
        batch
            .changes
            .iter()
            .any(|c| c.table == "fluent_pending_configs")
    );
    assert!(
        !batch
            .changes
            .iter()
            .any(|c| c.table == "fluent_config_receipts" || c.table == "transaction_auth")
    );
    let mut bytes = Vec::new();
    let snapshot =
        crate::store_stream::write_snapshot(&store, &mut bytes, 4 * 1024 * 1024).unwrap();
    assert!(
        snapshot
            .header
            .catalog
            .iter()
            .any(|t| t.name == "fluent_pending_configs")
    );
    assert!(
        !snapshot
            .header
            .catalog
            .iter()
            .any(|t| t.name == "fluent_config_receipts" || t.name == "transaction_auth")
    );
}

#[test]
fn pending_config_populated_respace_keeps_receipt_bytes_and_reinstalls_guards() {
    let mut store = loaded();
    let request = pending("urn:derived", &["urn:base"]);
    request.retain(&store).unwrap();
    let approval = approve(&store, &request);
    govern(&mut store, &approval);
    accept(&mut store, &request, &approval).unwrap();
    let receipt = request.receipt(&store, 0).unwrap().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let from = dir.path().join("from.db");
    let to = dir.path().join("to.db");
    std::fs::write(&from, store.serialize_db().unwrap()).unwrap();
    drop(store);
    crate::store::respace::respace_file(&from, &to, 7).unwrap();
    let independent = rusqlite::Connection::open(&to).unwrap();
    assert!(
        independent
            .execute("DELETE FROM fluent_config_receipts", [])
            .is_err()
    );
    drop(independent);
    let store = Store::open(to.to_str().unwrap()).unwrap();
    assert_eq!(request.receipt(&store, 0).unwrap().unwrap(), receipt);
    assert_eq!(
        Pending::load(&store, &request.id())
            .unwrap()
            .unwrap()
            .bytes(),
        request.bytes()
    );
}

#[test]
fn pending_config_scrub_decodes_escaped_request_fields_and_bounds_the_complete_manifest() {
    let original = pending("urn:derived", &["urn:base"]);
    let raw = String::from_utf8(original.bytes().to_vec())
        .unwrap()
        .replace("urn:derived", r#"urn:\u0046IXTURE_PRIVATE_TOKEN"#);
    let request = Pending::decode(raw.as_bytes()).unwrap();
    let mut relay = Store::open_in_memory().unwrap();
    crate::share_scrub::seed_test_catalogue(&mut relay);
    let opts = ShareOptions {
        no_shapes: true,
        queries: Some(Vec::new()),
        observable_configs: vec![raw.clone()],
        destination: ShareDestination::Outward,
        ..Default::default()
    };
    assert!(matches!(
        share_payload(&relay, &opts, crate::share::SHARE_PAYLOAD_MAX_BYTES),
        Err(crate::Error::PolicyDenied(_))
    ));
    let carrier = payload(&request).manifest;
    assert!(
        crate::pack::pack_to_bytes_with_carriers(
            &relay,
            crate::schema::ROOT_GRAPH_IRI,
            &crate::pack::PackOptions::default(),
            AT,
            std::slice::from_ref(&carrier)
        )
        .is_err()
    );
    let receiver = loaded();
    assert!(crate::fluent::pending_config::storage::validate_requests(&vec![raw; 65]).is_err());
    request.retain(&receiver).unwrap();
    assert_eq!(
        Pending::load(&receiver, &request.id())
            .unwrap()
            .unwrap()
            .bytes(),
        request.bytes()
    );
    assert!(
        receiver
            .lookup("urn:FIXTURE_PRIVATE_TOKEN")
            .unwrap()
            .is_none()
    );
    // A syntactically valid, hash-valid carrier still refuses a malformed config.
    let mut bad = carrier;
    bad.observable_configs = vec!["{}".into()];
    bad.share_id = crate::share::sha256(&crate::share::manifest_bytes(&bad, false).unwrap());
    assert!(
        crate::share_merge::share_from_parts(
            std::path::Path::new("pending-test"),
            bad,
            "",
            "".into()
        )
        .is_err()
    );
    assert!(request.receipt(&receiver, 0).unwrap().is_none());
}

#[test]
fn pending_config_disk_merge_retains_requests_and_cannot_install_configuration() {
    let request = pending("urn:derived", &["urn:base"]);
    let dir = tempfile::tempdir().unwrap();
    let source = Store::open_in_memory().unwrap();
    let base = crate::share::share(
        &source,
        dir.path().join("base").to_str().unwrap(),
        &ShareOptions {
            no_shapes: true,
            queries: Some(Vec::new()),
            destination: ShareDestination::Internal,
            ..Default::default()
        },
    )
    .unwrap();
    let shared = crate::share::share(
        &source,
        dir.path().join("incoming").to_str().unwrap(),
        &ShareOptions {
            no_shapes: true,
            queries: Some(Vec::new()),
            observable_configs: vec![String::from_utf8(request.bytes().to_vec()).unwrap()],
            destination: ShareDestination::Internal,
            parent_share: Some(base.share_id),
            ..Default::default()
        },
    )
    .unwrap();
    let mut receiver = loaded();
    let result = crate::share_merge::merge(
        &mut receiver,
        &dir.path().join("incoming"),
        AT,
        Some("root"),
    )
    .unwrap();
    assert_eq!(result.outcome, "merged");
    assert!(shared.fluent_proofs.is_empty());
    assert_eq!(
        Pending::load(&receiver, &request.id())
            .unwrap()
            .unwrap()
            .bytes(),
        request.bytes()
    );
    assert!(receiver.lookup("urn:derived").unwrap().is_none());
    assert!(request.receipt(&receiver, 0).unwrap().is_none());
}
