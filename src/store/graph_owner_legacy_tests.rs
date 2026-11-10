use super::*;
use crate::{
    read_authorization::{ComposedReadAdmission, ReadOperation},
    schema::ROOT_GRAPH_IRI,
};

fn legacy_fixture(root: &Path) -> (Owner, Owner, ComposedReadAdmission) {
    let (mut legacy, mut pin) = fixture(root, "legacy");
    let (store, route) = fixture(root, "routed");
    pin.graph = ROOT_GRAPH_IRI.into();
    crate::rdf::ingest_assigned_rdf(
        &mut legacy,
        b"<urn:legacy> <urn:link> <urn:shared> .".as_slice(),
        oxrdfio::RdfFormat::Turtle,
        None,
        "1970-01-01T00:00:00Z",
        None,
        None,
        0,
    )
    .unwrap();
    let hidden = legacy
        .lookup("https://example.test/graphs/legacy")
        .unwrap()
        .unwrap();
    crate::rdf::ingest_assigned_rdf(
        &mut legacy,
        b"<urn:hidden> <urn:link> <urn:shared> .".as_slice(),
        oxrdfio::RdfFormat::Turtle,
        None,
        "1970-01-01T00:00:00Z",
        None,
        None,
        hidden,
    )
    .unwrap();
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
        .with_legacy_participant(pin.clone())
        .unwrap(),
    );
    assert!(ComposedReadAdmission::new(catalog.clone(), 512, 2, 1).is_err());
    let admission = ComposedReadAdmission::new(catalog.clone(), 512, 3, 1).unwrap();
    assert_eq!(admission.allocation().connections, 7);
    let legacy = Owner::prepare_with_memory(
        catalog.clone(),
        root,
        ROOT_GRAPH_IRI,
        legacy,
        "policy-v1",
        admission.allocation(),
    )
    .unwrap();
    let routed = Owner::prepare_with_memory(
        catalog,
        root,
        &route.graph,
        store,
        "policy-v1",
        admission.allocation(),
    )
    .unwrap();
    routed
        .knot(
            &request(&routed),
            &json!({"graph":routed.route.graph,
        "turtle":"<urn:shared> <urn:value> \"answer\" ."}),
        )
        .unwrap();
    (legacy, routed, admission)
}

#[test]
fn legacy_root_composes_only_explicit_scope_at_common_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let (legacy, routed, admission) = legacy_fixture(temp.path());
    let (policy, identity) = composition_authority(&[ROOT_GRAPH_IRI, &routed.route.graph]);
    let policy = policy.snapshot().unwrap();
    let without_legacy = ComposedReadAdmission::new(
        Arc::new(
            Catalog::new(
                7,
                &legacy.route.store_id,
                vec![routed.route.clone()],
                legacy.catalog.budget(),
            )
            .unwrap(),
        ),
        512,
        3,
        1,
    )
    .unwrap();
    let deadline = strict_deadline();
    // Legacy is additional to max_owners=1, never an unbudgeted routed owner.
    Owner::read_strict(&[&routed, &legacy], deadline, |reads| {
        // Commit after capture. The retained ROOT snapshot must still be old.
        crate::rdf::ingest_assigned_rdf(&mut legacy.writer.lock(),
            b"<urn:late> <urn:link> <urn:shared> .".as_slice(), oxrdfio::RdfFormat::Turtle,
            None, "1970-01-01T00:00:00Z", None, None, 0)?;
        assert!(policy.authorize(&identity, ReadOperation::Query, None, 2)?
            .open_composed_cut(reads, without_legacy.try_reserve()?, 2, deadline).is_err());
        let view = policy.authorize(&identity, ReadOperation::Query, None, 2)?
            .open_composed_cut(reads, admission.try_reserve()?, 2, deadline)?;
        let query = format!("SELECT ?s ?v FROM <{}> FROM NAMED <{}> WHERE {{ ?s <urn:link> ?x . GRAPH <{}> {{ ?x <urn:value> ?v }} }}", ROOT_GRAPH_IRI, routed.route.graph, routed.route.graph);
        for resident in [false, true] {
            let answer: Value = serde_json::from_slice(&view.query(&query, resident, 2)?).unwrap();
            assert_eq!(answer["results"]["bindings"].as_array().unwrap().len(), 1);
            assert_eq!(answer["results"]["bindings"][0]["s"]["value"], "urn:legacy");
            assert!(view.query("ASK { GRAPH <urn:quipu:graph:root> { ?s ?p ?o } }", resident, 2).is_err());
            assert!(view.query("ASK { GRAPH <https://example.test/graphs/legacy> { ?s ?p ?o } }", resident, 2).is_err());
        }
        view.recheck(&policy, &identity, reads, 2)?;
        Ok(())
    }).unwrap();
    assert!(admission.try_reserve().is_ok());
    for owner in [&legacy, &routed] {
        assert!(owner.readers[0].lock().conn.is_autocommit());
    }
    // An ordinary routed-only grant never acquires implicit ROOT visibility.
    let (policy, identity) = composition_authority(&[&routed.route.graph]);
    let policy = policy.snapshot().unwrap();
    Owner::read_strict(&[&legacy, &routed], strict_deadline(), |reads| {
        assert!(
            policy
                .authorize(&identity, ReadOperation::Query, None, 2)?
                .open_composed_cut(reads, admission.try_reserve()?, 2, strict_deadline())
                .is_err()
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn legacy_root_busy_fence_refuses_entire_cut_and_remains_non_routed() {
    let temp = tempfile::tempdir().unwrap();
    let (legacy, routed, _) = legacy_fixture(temp.path());
    let held = legacy.writer.lock();
    let mut called = false;
    assert!(
        Owner::read_strict(&[&routed, &legacy], strict_deadline(), |_| {
            called = true;
            Ok(())
        })
        .is_err()
    );
    assert!(!called);
    drop(held);
    Owner::read_strict(&[&routed, &legacy], strict_deadline(), |_| Ok(())).unwrap();
    assert_eq!(
        legacy
            .catalog
            .write_target(&WriteRequest {
                graph: None,
                catalog_revision: 7,
                owner_generation: None,
                policy: "policy-v1",
                signed: false,
                cross_graph_context: false,
            })
            .unwrap(),
        WriteTarget::Legacy
    );
    assert!(
        legacy
            .knot(&request(&legacy), &input(&legacy, "forbidden"))
            .is_err()
    );
}

#[test]
fn legacy_pin_refuses_foreign_identity_duplicate_file_and_route_promotion() {
    let temp = tempfile::tempdir().unwrap();
    let (store, mut pin) = fixture(temp.path(), "legacy");
    pin.graph = ROOT_GRAPH_IRI.into();
    assert!(
        Catalog::new(
            7,
            &pin.store_id,
            vec![pin.clone()],
            Budget {
                max_owners: 1,
                queued_per_owner: 1,
                readers_per_owner: 1,
                max_queued: 1,
                max_connections: 3,
            }
        )
        .is_err()
    );
    assert!(
        Arc::try_unwrap(catalog(vec![]))
            .unwrap()
            .with_legacy_participant(pin.clone())
            .is_err()
    );
    let mut route = pin.clone();
    route.graph = "urn:other".into();
    route.store_id = "other".into();
    let c = Catalog::new(
        7,
        &pin.store_id,
        vec![route],
        Budget {
            max_owners: 1,
            queued_per_owner: 1,
            readers_per_owner: 1,
            max_queued: 1,
            max_connections: 3,
        },
    )
    .unwrap();
    assert!(c.with_legacy_participant(pin).is_err());
    drop(store);
}
