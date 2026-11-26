use super::*;
use crate::{
    graph_catalog::{Budget, GraphSet},
    read_authorization::*,
    schema::ROOT_GRAPH_IRI,
};
use serde_json::json;
use std::time::Duration;

const APP: &str = "urn:application";
const PROV: &str = "urn:provenance";
const REFS: &str = "urn:references";

fn file(root: &Path, name: &str, graphs: &[&str]) -> Store {
    let path = root.join(format!("{name}.db"));
    {
        let store = Store::open(path.to_str().unwrap()).unwrap();
        for graph in graphs {
            store.graph_create(graph).unwrap();
        }
    }
    Store::adopt(path.to_str().unwrap()).unwrap();
    Store::open(path.to_str().unwrap()).unwrap()
}

fn write(store: &mut Store, graph: &str, text: &str) {
    let id = if graph == ROOT_GRAPH_IRI {
        0
    } else {
        store.lookup(graph).unwrap().unwrap()
    };
    crate::rdf::ingest_assigned_rdf(
        store,
        text.as_bytes(),
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-01-01T00:00:00Z",
        None,
        None,
        id,
    )
    .unwrap();
}

fn pin(store: &Store, name: &str, graph: &str) -> Route {
    Route {
        graph: graph.into(),
        file_id: name.into(),
        store_id: store.store_id().unwrap(),
        log_epoch: store.log_epoch().unwrap(),
        generation: 3,
        policy: "policy-v1".into(),
    }
}

fn reservation() -> ComposedReadReservation {
    ComposedReadReservation {
        runtime_bytes: 128 * 1_048_576,
        view_bytes: 64 * 1_048_576,
        query_bytes: 32 * 1_048_576,
        max_response_bytes: 1_048_576,
    }
}

fn fixture(root: &Path) -> (GraphSetDataset, ComposedReadAdmission) {
    let mut legacy = file(root, "legacy", &[REFS, "urn:hidden"]);
    let mut routed = file(root, "application", &[APP, PROV, "urn:hidden"]);
    for i in 0..32 {
        routed.intern(&format!("urn:padding:{i}")).unwrap();
    }
    write(
        &mut legacy,
        ROOT_GRAPH_IRI,
        "<urn:service> <urn:tracks> <urn:reference> .",
    );
    write(
        &mut legacy,
        REFS,
        "<urn:reference> <urn:item> <urn:shared> .",
    );
    write(&mut routed, APP, "<urn:shared> <urn:value> \"answer\" .");
    write(
        &mut routed,
        PROV,
        "<urn:write> <urn:describes> <urn:shared> .",
    );
    write(
        &mut routed,
        "urn:hidden",
        "<urn:shared> <urn:value> \"secret\" .",
    );
    assert_ne!(
        legacy.lookup("urn:shared").unwrap(),
        routed.lookup("urn:shared").unwrap()
    );
    let catalog = Arc::new(
        GraphSetCatalog::new(
            7,
            &legacy.store_id().unwrap(),
            vec![GraphSet {
                owner: pin(&routed, "application", APP),
                graphs: vec![APP.into(), PROV.into()],
            }],
            Budget {
                max_owners: 1,
                queued_per_owner: 1,
                readers_per_owner: 1,
                max_queued: 1,
                max_connections: 3,
            },
            4,
        )
        .unwrap()
        .with_legacy_graphs(
            pin(&legacy, "legacy", ROOT_GRAPH_IRI),
            vec![ROOT_GRAPH_IRI.into(), REFS.into()],
        )
        .unwrap(),
    );
    let admission =
        ComposedReadAdmission::for_graph_sets(catalog.clone(), 512, 3, 1, reservation()).unwrap();
    let owner = GraphSetOwner::prepare(
        catalog.clone(),
        root,
        APP,
        routed,
        "policy-v1",
        admission.allocation(),
    )
    .unwrap();
    let dataset = GraphSetDataset::prepare(
        catalog,
        root,
        legacy,
        vec![owner],
        "policy-v1",
        admission.allocation(),
    )
    .unwrap();
    (dataset, admission)
}

fn authority(graphs: &[&str]) -> (OwnerReadPolicy, ReadIdentity) {
    use sha2::{Digest, Sha256};
    let registry = crate::crew_credentials::CredentialRegistry::parse(
        &serde_json::to_vec(&json!({"version":1,"credentials":[{
            "credential_id":"graph-set-fixture", "principal":"urn:reader", "audience":"quipu",
            "token_sha256":format!("{:x}", Sha256::digest(b"graph-set-fixture-token"))
        }]}))
        .unwrap(),
        "quipu",
    )
    .unwrap();
    let identity =
        ReadIdentity::authenticate_named(&registry, Some("Bearer graph-set-fixture-token"))
            .unwrap();
    let mut policy = OwnerReadPolicy::open(":memory:", "urn:owner").unwrap();
    policy
        .apply(
            &ReadGrant {
                version: 1,
                id: "urn:grant".into(),
                principal: "urn:reader".into(),
                graphs: graphs.iter().map(|g| (*g).into()).collect(),
                operations: [ReadOperation::Query].into(),
                valid_from: 1,
                valid_until: 100,
                max_offline_seconds: 300,
                issuer: "urn:owner".into(),
                revision: 1,
                revoked: false,
            },
            "private fixture",
        )
        .unwrap();
    (policy, identity)
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

#[test]
fn root_reference_and_data_provenance_join_by_rdf_identity_across_two_files() {
    let temp = tempfile::tempdir().unwrap();
    let (dataset, admission) = fixture(temp.path());
    let graphs = [ROOT_GRAPH_IRI, REFS, APP, PROV];
    let (policy, identity) = authority(&graphs);
    let policy = policy.snapshot().unwrap();
    let permit = admission.try_reserve().unwrap();
    let output = dataset.read(&graphs, deadline(), |reads| {
        let view = policy.authorize(&identity, ReadOperation::Query, None, 2)?
            .open_composed_cut(reads, permit, 2, deadline())?;
        view.query(&format!("SELECT ?value FROM <{ROOT_GRAPH_IRI}> FROM NAMED <{REFS}> FROM NAMED <{APP}> FROM NAMED <{PROV}> WHERE {{ <urn:service> <urn:tracks> ?ref . GRAPH <{REFS}> {{ ?ref <urn:item> ?item }} GRAPH <{APP}> {{ ?item <urn:value> ?value }} GRAPH <{PROV}> {{ ?write <urn:describes> ?item }} }}"), false, 2)
    }).unwrap();
    let result: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(result["results"]["bindings"].as_array().unwrap().len(), 1);
    assert_eq!(result["results"]["bindings"][0]["value"]["value"], "answer");
}

#[test]
fn logical_members_share_one_snapshot_even_when_the_owner_commits_during_evaluation() {
    let temp = tempfile::tempdir().unwrap();
    let (dataset, _) = fixture(temp.path());
    let owner = &dataset.routed[APP];
    let (entity, attribute) = owner
        .owner
        .read(|store| {
            Ok((
                store.lookup("urn:shared")?.unwrap(),
                store.lookup("urn:value")?.unwrap(),
            ))
        })
        .unwrap();
    let datum = crate::store::Datum {
        entity,
        attribute,
        value: crate::types::Value::Str("later".into()),
        valid_from: "2026-01-01T00:00:00Z".into(),
        valid_to: None,
        op: crate::types::Op::Assert,
    };
    let route = owner.owner.catalog_route();
    let batch = GraphSetBatch {
        store_id: route.store_id.clone(),
        log_epoch: route.log_epoch.clone(),
        graphs: vec![
            (APP.into(), vec![datum.clone()]),
            (PROV.into(), vec![datum]),
        ],
        timestamp: "2026-01-01T00:00:00Z".into(),
        actor: None,
        source: None,
    };
    let requests: Vec<_> = [APP, PROV]
        .into_iter()
        .map(|g| WriteRequest {
            graph: Some(g),
            catalog_revision: 7,
            owner_generation: Some(3),
            policy: "policy-v1",
            signed: false,
            cross_graph_context: false,
        })
        .collect();
    dataset
        .read(&[APP, PROV], deadline(), |reads| {
            assert!(std::ptr::eq(reads[0].store, reads[1].store));
            assert_eq!(reads[0].local_tx, reads[1].local_tx);
            owner.transact(&requests, &batch)?;
            for read in reads {
                let id = read.store.lookup(&read.route.graph)?.unwrap();
                let count: i64 = read.store.conn.query_row(
                    "SELECT count(*) FROM facts WHERE g=?1",
                    [id],
                    |r| r.get(0),
                )?;
                assert_eq!(
                    count, 1,
                    "logical member escaped its captured file snapshot"
                );
            }
            Ok(())
        })
        .unwrap();
    dataset
        .read(&[APP, PROV], deadline(), |reads| {
            for read in reads {
                let id = read.store.lookup(&read.route.graph)?.unwrap();
                let count: i64 = read.store.conn.query_row(
                    "SELECT count(*) FROM facts WHERE g=?1",
                    [id],
                    |r| r.get(0),
                )?;
                assert_eq!(count, 2);
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn unselected_root_fence_is_not_borrowed_and_unknown_or_duplicate_scope_refuses() {
    let temp = tempfile::tempdir().unwrap();
    let (dataset, _) = fixture(temp.path());
    let held = dataset.legacy.writer.lock();
    dataset
        .read(&[APP, PROV], deadline(), |reads| {
            assert_eq!(reads.len(), 2);
            Ok(())
        })
        .unwrap();
    for graphs in [
        vec![ROOT_GRAPH_IRI, APP],
        vec![APP, "urn:hidden"],
        vec![APP, APP],
    ] {
        assert!(
            dataset
                .read(&graphs, deadline(), |_| -> Result<()> {
                    panic!("refused dataset reached evaluator")
                })
                .is_err()
        );
    }
    drop(held);
    dataset
        .read(&[ROOT_GRAPH_IRI, APP], deadline(), |_| Ok(()))
        .unwrap();
}

#[test]
fn physical_manifest_does_not_grant_logical_graphs_or_weaken_ordinary_admission() {
    let temp = tempfile::tempdir().unwrap();
    let (dataset, admission) = fixture(temp.path());
    for (graphs, special) in [(vec![APP], true), (vec![APP, PROV], false)] {
        let (policy, identity) = authority(&graphs);
        let policy = policy.snapshot().unwrap();
        let ordinary =
            ComposedReadAdmission::new(dataset.catalog.physical_pin(), 512, 3, 1, reservation())
                .unwrap();
        let permit = if special {
            admission.try_reserve().unwrap()
        } else {
            ordinary.try_reserve().unwrap()
        };
        assert!(
            dataset
                .read(&[APP, PROV], deadline(), |reads| {
                    policy
                        .authorize(&identity, ReadOperation::Query, None, 2)?
                        .open_composed_cut(reads, permit, 2, deadline())
                })
                .is_err()
        );
    }
    let request = WriteRequest {
        graph: Some(REFS),
        catalog_revision: 7,
        owner_generation: Some(3),
        policy: "policy-v1",
        signed: false,
        cross_graph_context: false,
    };
    assert!(dataset.catalog.plan_batch(&[request]).is_err());
}

#[path = "graph_owner_set_read_matrix_tests.rs"]
mod matrix;
