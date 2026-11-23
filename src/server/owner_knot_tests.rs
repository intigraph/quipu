use super::*;
use quipu::{
    Store,
    graph_catalog::{Budget, Catalog, Route},
    store::graph_owner::Owner,
};
use serde_json::json;
use std::time::Duration;

const GRAPH: &str = "https://example.test/graphs/transport";
const ID: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn identity(class: &str) -> quipu::transaction_auth::Identity {
    quipu::transaction_auth::Identity {
        principal: "urn:test:transport".into(),
        credential_id: Some("transport-key".into()),
        auth_class: class.into(),
        attestation: Default::default(),
    }
}

fn fixture() -> (tempfile::TempDir, Store, Arc<PreparedOwner>, Pin, Value) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("owner.db");
    Store::open(path.to_str().unwrap())
        .unwrap()
        .graph_create(GRAPH)
        .unwrap();
    Store::adopt(path.to_str().unwrap()).unwrap();
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let route = Route {
        graph: GRAPH.into(),
        file_id: "owner".into(),
        store_id: store.store_id().unwrap(),
        log_epoch: store.log_epoch().unwrap(),
        generation: 4,
        policy: "policy-v1".into(),
    };
    let legacy = Store::open(root.join("legacy.db").to_str().unwrap()).unwrap();
    let catalog = Arc::new(
        Catalog::new(
            7,
            &legacy.store_id().unwrap(),
            vec![route],
            Budget {
                max_owners: 1,
                queued_per_owner: 1,
                readers_per_owner: 1,
                max_queued: 1,
                max_connections: 3,
            },
        )
        .unwrap(),
    );
    legacy.fence_routed_graph(GRAPH, 7, 4).unwrap();
    let owner = Owner::prepare(catalog.clone(), &root, GRAPH, store, "policy-v1").unwrap();
    let router = Router::admit(catalog, &legacy, vec![owner]).unwrap();
    let endpoint = PreparedOwner::new(router, GRAPH.into()).unwrap();
    let pin = Pin {
        catalog_revision: 7,
        generation: 4,
        policy: "policy-v1".into(),
        signed: false,
        cross_graph_context: false,
    };
    let input = json!({"graph":GRAPH,"turtle":"<urn:test:s> <urn:test:p> \"transport\" .","source":"transport-test"});
    (dir, legacy, endpoint, pin, input)
}

#[tokio::test]
async fn real_owner_receipt_progresses_with_legacy_writer_and_global_permit_held() {
    let (dir, legacy, owner, pin, input) = fixture();
    let sqlite = rusqlite::Connection::open(dir.path().join("legacy.db")).unwrap();
    sqlite.execute_batch("BEGIN IMMEDIATE").unwrap();
    let (started, running) = tokio::sync::oneshot::channel();
    let (release, held) = std::sync::mpsc::channel();
    let legacy_work = tokio::spawn(crate::admission::write_blocking(move || {
        started.send(()).unwrap();
        held.recv().unwrap();
        Ok(())
    }));
    running.await.unwrap();
    let outcome = crate::auth::run_identified(
        Some(identity("named_bearer")),
        tokio::time::timeout(
            Duration::from_secs(5),
            owner.knot(pin.clone(), input.clone(), ID.into()),
        ),
    )
    .await;
    sqlite.execute_batch("ROLLBACK").unwrap();
    release.send(()).unwrap();
    legacy_work.await.unwrap().unwrap();
    let result = outcome
        .expect("owner entered legacy admission or SQLite writer")
        .unwrap();
    let proof = crate::auth::run_identified(
        Some(identity("named_bearer")),
        owner.status(pin.clone(), input.clone(), ID.into()),
    )
    .await
    .unwrap();
    assert_eq!(proof["state"], "finished");
    assert_eq!(proof["receipt"]["tx_id"], result["tx_id"]);
    assert!(
        crate::auth::run_identified(
            Some(identity("named_bearer")),
            owner.seal(pin.clone(), input.clone(), ID.into())
        )
        .await
        .is_err(),
        "a committed effect cannot be sealed"
    );
    assert!(
        crate::auth::run_identified(
            Some(identity("named_bearer")),
            owner.knot(pin, input, ID.into())
        )
        .await
        .is_err()
    );
    let legacy_ledger: i64 = sqlite
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='knot_attempts'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(legacy_ledger, 0);
    legacy.verify_routed_fence().unwrap();
}

#[tokio::test]
async fn status_uses_its_reader_while_owner_write_queue_is_held() {
    let (_dir, _legacy, owner, pin, input) = fixture();
    let (started, running) = tokio::sync::oneshot::channel();
    let (release, held) = std::sync::mpsc::channel();
    let work = {
        let owner = owner.clone();
        tokio::spawn(async move {
            owner
                .writes
                .write(move || {
                    started.send(()).unwrap();
                    held.recv().unwrap();
                    Ok(())
                })
                .await
        })
    };
    running.await.unwrap();
    let proof = crate::auth::run_identified(
        Some(identity("named_bearer")),
        tokio::time::timeout(Duration::from_secs(5), owner.status(pin, input, ID.into())),
    )
    .await;
    release.send(()).unwrap();
    work.await.unwrap().unwrap();
    assert_eq!(
        proof.expect("status entered owner writer queue").unwrap()["state"],
        "unknown"
    );
}

#[tokio::test]
async fn invalid_contracts_refuse_before_consuming_an_id() {
    let (_dir, _legacy, owner, pin, input) = fixture();
    assert!(
        owner
            .knot(pin.clone(), input.clone(), ID.into())
            .await
            .is_err()
    );
    assert!(
        crate::auth::run_identified(
            Some(identity("scoped_named_bearer")),
            owner.knot(pin.clone(), input.clone(), ID.into())
        )
        .await
        .is_err()
    );
    for variant in 0..4 {
        let mut bad = pin.clone();
        match variant {
            0 => bad.generation += 1,
            1 => bad.catalog_revision += 1,
            2 => bad.signed = true,
            _ => bad.cross_graph_context = true,
        }
        assert!(
            crate::auth::run_identified(
                Some(identity("named_bearer")),
                owner.knot(bad, input.clone(), ID.into())
            )
            .await
            .is_err()
        );
    }
    let mut wrong_graph = input.clone();
    wrong_graph["graph"] = json!("urn:test:other");
    assert!(
        crate::auth::run_identified(
            Some(identity("named_bearer")),
            owner.knot(pin.clone(), wrong_graph, ID.into())
        )
        .await
        .is_err()
    );
    crate::auth::run_identified(
        Some(identity("named_bearer")),
        owner.knot(pin, input, ID.into()),
    )
    .await
    .unwrap();
}
