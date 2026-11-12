//! Explicit pending materialization is neither ordinary load nor admission.
use super::*;
use crate::pack::materialize_pending_carriers;

fn table_count(store: &Store, table: &str) -> i64 {
    store
        .conn
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

fn state(store: &Store) -> Vec<i64> {
    [
        "terms",
        "transactions",
        "facts",
        "shapes",
        "queries",
        "transaction_auth",
        "fluent_origin_receipts",
        "fluent_write_permits",
    ]
    .map(|table| table_count(store, table))
    .to_vec()
}

fn retained(store: &Store) -> Vec<(String, String)> {
    store
        .conn
        .prepare("SELECT share_id,manifest FROM share_fluent_carriers ORDER BY share_id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<std::result::Result<_, _>>()
        .unwrap()
}

#[test]
fn pending_materialization_exact_bytes_disk_replay_and_fresh_native_authority() {
    for graph in [crate::schema::ROOT_GRAPH_IRI, "urn:graph-a", "urn:graph-b"] {
        let proof = super::super::carrier_tests::proof(graph);
        let request = super::super::carrier_tests::request(std::slice::from_ref(&proof));
        let (_, payload) = bytes(std::slice::from_ref(&request.manifest));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.db");
        std::fs::write(&path, &payload).unwrap();
        let dest = dir.path().join("receiver.db");
        let mut receiver = Store::open(dest.to_str().unwrap()).unwrap();
        receiver.set_base_ns(crate::namespace::DEFAULT_BASE_NS);
        let before = state(&receiver);
        let report = materialize_pending_carriers(
            path.to_str().unwrap(),
            &mut receiver,
            ShareDestination::Internal,
        )
        .unwrap();
        assert_eq!((report.retained, report.unchanged), (1, 0));
        assert_eq!(state(&receiver), before);
        assert!(receiver.lookup("urn:carried-event").unwrap().is_none());
        let expected = vec![(
            request.manifest.share_id.clone(),
            String::from_utf8(crate::share::manifest_bytes(&request.manifest, true).unwrap())
                .unwrap(),
        )];
        assert_eq!(retained(&receiver), expected);
        drop(receiver);
        let mut receiver = Store::open(dest.to_str().unwrap()).unwrap();
        receiver.set_base_ns(crate::namespace::DEFAULT_BASE_NS);
        let changes = receiver.conn.total_changes();
        let replay = materialize_pending_carriers(
            path.to_str().unwrap(),
            &mut receiver,
            ShareDestination::Internal,
        )
        .unwrap();
        assert_eq!((replay.retained, replay.unchanged), (0, 1));
        assert_eq!(receiver.conn.total_changes(), changes);
        assert_eq!(retained(&receiver), expected);
        assert_eq!(state(&receiver), before);
        assert_eq!(std::fs::read(&path).unwrap(), payload);

        // Transport did not load even the prerequisite local shape/profile.
        let (mut configured, g) = super::super::tests::loaded_shapes(graph);
        configured.set_base_ns(crate::namespace::DEFAULT_BASE_NS);
        materialize_pending_carriers(
            path.to_str().unwrap(),
            &mut configured,
            ShareDestination::Internal,
        )
        .unwrap();
        assert!(
            with_identity(Some(super::super::carrier_tests::importer()), || {
                crate::share_fluent::admit(&mut configured, &request.manifest.share_id, g)
            })
            .is_err()
        );
        super::super::governed_tests::authorize(&mut configured, &proof);
        let tx = with_identity(Some(super::super::carrier_tests::importer()), || {
            crate::share_fluent::admit(&mut configured, &request.manifest.share_id, g)
        })
        .unwrap();
        assert_eq!(
            Proof::retained(&configured, g, "urn:carried-event")
                .unwrap()
                .body,
            proof.body
        );
        assert_eq!(
            with_identity(Some(super::super::carrier_tests::importer()), || {
                crate::share_fluent::admit(&mut configured, &request.manifest.share_id, g)
            })
            .unwrap(),
            tx
        );
    }
}

#[test]
fn pending_materialization_conflict_and_sql_failure_roll_back_entire_batch() {
    let a = manifest(vec![" {\"a\":1} ".into()]);
    let b = manifest(vec!["{\"b\":2}".into()]);
    let mut selected = vec![a, b];
    selected.sort_by(|a, b| a.share_id.cmp(&b.share_id));
    let (_, payload) = bytes(&selected);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.db");
    std::fs::write(&path, payload).unwrap();
    for conflict in [true, false] {
        let mut receiver = Store::open_in_memory().unwrap();
        if conflict {
            // Last row conflicts, so the first insertion must roll back too.
            receiver
                .conn
                .execute(
                    "INSERT INTO share_fluent_carriers VALUES(?1,'malformed')",
                    [&selected[1].share_id],
                )
                .unwrap();
        } else {
            receiver.conn.execute_batch(&format!(
                "CREATE TRIGGER refuse_second_pending BEFORE INSERT ON share_fluent_carriers WHEN NEW.share_id='{}' BEGIN SELECT RAISE(ABORT,'test constraint'); END;",
                selected[1].share_id)).unwrap();
        }
        let before = state(&receiver);
        let pending = retained(&receiver);
        assert!(
            materialize_pending_carriers(
                path.to_str().unwrap(),
                &mut receiver,
                ShareDestination::Internal
            )
            .is_err()
        );
        assert_eq!(retained(&receiver), pending);
        assert_eq!(state(&receiver), before);
    }
}

#[test]
fn pending_materialization_receiver_policy_inspects_escaped_and_shadowed_bytes() {
    for proof in [
        r#"{"value":"FIXTURE_PRIVATE_TOKEN"}"#,
        r#"{"value":"\u0046IXTURE_PRIVATE_TOKEN"}"#,
        r#"{"value":"\u0046IXTURE_PRIVATE_TOKEN","value":"public"}"#,
    ] {
        let pending = manifest(vec![proof.into()]);
        // Producer has no catalogue. Receiver policy must still be enforced.
        let (_, payload) = bytes(std::slice::from_ref(&pending));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.db");
        std::fs::write(&path, payload).unwrap();
        let mut receiver = Store::open_in_memory().unwrap();
        crate::share_scrub::seed_test_catalogue(&mut receiver);
        let before = state(&receiver);
        assert!(matches!(
            materialize_pending_carriers(
                path.to_str().unwrap(),
                &mut receiver,
                ShareDestination::Outward
            ),
            Err(Error::PolicyDenied(_))
        ));
        assert!(retained(&receiver).is_empty());
        assert_eq!(state(&receiver), before);
        materialize_pending_carriers(
            path.to_str().unwrap(),
            &mut receiver,
            ShareDestination::Internal,
        )
        .unwrap();
        assert_eq!(
            crate::share_fluent::imported_proofs(&receiver, &pending.share_id).unwrap(),
            Some(vec![proof.into()])
        );
    }
}

#[test]
fn pending_materialization_refuses_tamper_and_empty_without_receiver_changes() {
    let pending = manifest(vec!["{}".into()]);
    for tamper in [true, false] {
        let (_, payload) = if tamper {
            bytes(std::slice::from_ref(&pending))
        } else {
            bytes(&[])
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.db");
        std::fs::write(&path, payload).unwrap();
        if tamper {
            rusqlite::Connection::open(&path)
                .unwrap()
                .execute(
                    "UPDATE share_fluent_carriers SET manifest=manifest||' '",
                    [],
                )
                .unwrap();
        }
        let mut receiver = Store::open_in_memory().unwrap();
        let before = state(&receiver);
        assert!(
            materialize_pending_carriers(
                path.to_str().unwrap(),
                &mut receiver,
                ShareDestination::Internal
            )
            .is_err()
        );
        assert!(retained(&receiver).is_empty());
        assert_eq!(state(&receiver), before);
    }
}

#[test]
fn pending_materialization_never_installs_pack_facts_or_configuration() {
    let mut producer = Store::open_in_memory().unwrap();
    crate::rdf::ingest_rdf(
        &mut producer,
        b"<urn:pack-config> <urn:value> \"must remain pending\" .".as_slice(),
        oxrdfio::RdfFormat::NTriples,
        None,
        AT,
        None,
        None,
    )
    .unwrap();
    producer
        .load_shapes(
            "source-only",
            "@prefix sh: <http://www.w3.org/ns/shacl#> . <urn:shape> sh:path <urn:source-only> .",
            AT,
        )
        .unwrap();
    let pending = manifest(vec![
        r#"{"observable":"urn:untrusted-definition","actor":"root"}"#.into(),
    ]);
    let (_, payload) = pack_to_bytes_with_carriers(
        &producer,
        crate::schema::ROOT_GRAPH_IRI,
        &PackOptions {
            shapes: vec!["source-only".into()],
            ..options()
        },
        AT,
        std::slice::from_ref(&pending),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("configured-source.db");
    std::fs::write(&path, payload).unwrap();
    let source = Store::open_immutable(path.to_str().unwrap()).unwrap();
    assert!(!source.current_facts_in_graph(0).unwrap().is_empty());
    assert_eq!(source.list_shapes().unwrap().len(), 1);
    let mut receiver = Store::open_in_memory().unwrap();
    let before = state(&receiver);
    materialize_pending_carriers(
        path.to_str().unwrap(),
        &mut receiver,
        ShareDestination::Internal,
    )
    .unwrap();
    assert_eq!(state(&receiver), before);
    assert!(receiver.lookup("urn:pack-config").unwrap().is_none());
    assert!(
        receiver
            .lookup("urn:untrusted-definition")
            .unwrap()
            .is_none()
    );
    assert!(receiver.list_shapes().unwrap().is_empty());
    assert_eq!(
        crate::share_fluent::imported_proofs(&receiver, &pending.share_id).unwrap(),
        Some(pending.fluent_proofs)
    );
}
