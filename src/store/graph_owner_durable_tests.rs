use super::*;

const ID: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
fn identity(name: &str, class: &str) -> crate::transaction_auth::Identity {
    crate::transaction_auth::Identity {
        principal: format!("urn:owner:{name}"),
        credential_id: Some(name.into()),
        auth_class: class.into(),
        attestation: Default::default(),
    }
}
fn pin(owner: &Owner) -> WriteRequest<'_> {
    WriteRequest {
        graph: Some(&owner.route.graph),
        catalog_revision: owner.catalog.revision(),
        owner_generation: Some(owner.route.generation),
        policy: &owner.route.policy,
        signed: false,
        cross_graph_context: false,
    }
}
fn prepared(root: &Path, name: &str) -> (Owner, Route) {
    let (store, route) = fixture(root, name);
    let owner = Owner::prepare(
        catalog(vec![route.clone()]),
        root,
        &route.graph,
        store,
        "policy-v1",
    )
    .unwrap();
    (owner, route)
}
fn has_ledger(store: &Store) -> bool {
    store
        .conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='knot_attempts')",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn durable_receipt_lives_in_owner_while_legacy_writer_is_held_and_survives_reopen() {
    crate::transaction_auth::with_identity(Some(identity("alice", "named_bearer")), || {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let legacy = Store::open(root.join("legacy.db").to_str().unwrap()).unwrap();
        legacy.conn.execute_batch("BEGIN IMMEDIATE").unwrap();
        let before = legacy.conn.total_changes();
        let (owner, route) = prepared(&root, "a");
        let body = input(&owner, "durable");
        let result = owner.knot_durable(&pin(&owner), &body, ID).unwrap();
        let receipt = owner.knot_attempt_status(&pin(&owner), &body, ID).unwrap();
        assert_eq!(receipt["state"], "finished");
        assert_eq!(receipt["receipt"]["tx_id"], result["tx_id"]);
        assert!(!has_ledger(&legacy));
        assert_eq!(legacy.conn.total_changes(), before);
        legacy.conn.execute_batch("ROLLBACK").unwrap();
        let configured = owner.catalog.clone();
        drop(owner);
        let store = Store::open(root.join("a.db").to_str().unwrap()).unwrap();
        let reopened = Owner::prepare(configured, &root, &route.graph, store, "policy-v1").unwrap();
        assert_eq!(
            reopened
                .knot_attempt_status(&pin(&reopened), &body, ID)
                .unwrap(),
            receipt
        );
        assert!(reopened.knot_durable(&pin(&reopened), &body, ID).is_err());
    });
}

#[test]
fn original_id_stays_consumed_when_generation_or_body_or_identity_changes() {
    crate::transaction_auth::with_identity(Some(identity("alice", "named_bearer")), || {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (owner, mut route) = prepared(&root, "a");
        let body = input(&owner, "once");
        owner.knot_durable(&pin(&owner), &body, ID).unwrap();
        let changed = input(&owner, "changed");
        assert_eq!(
            owner
                .knot_attempt_status(&pin(&owner), &changed, ID)
                .unwrap()["state"],
            "unknown"
        );
        assert!(owner.knot_durable(&pin(&owner), &changed, ID).is_err());
        crate::transaction_auth::with_identity(Some(identity("bob", "named_bearer")), || {
            assert_eq!(
                owner.knot_attempt_status(&pin(&owner), &body, ID).unwrap()["state"],
                "unknown"
            );
            assert!(owner.knot_durable(&pin(&owner), &body, ID).is_err());
        });
        drop(owner);
        route.generation += 1;
        let configured = Arc::new(
            Catalog::new(
                8,
                "legacy-store",
                vec![route.clone()],
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
        let store = Store::open(root.join("a.db").to_str().unwrap()).unwrap();
        let owner = Owner::prepare(configured, &root, &route.graph, store, "policy-v1").unwrap();
        assert_eq!(
            owner.knot_attempt_status(&pin(&owner), &body, ID).unwrap()["state"],
            "unknown"
        );
        assert!(owner.knot_durable(&pin(&owner), &body, ID).is_err());
        assert_eq!(
            values(&owner)["count"],
            1,
            "no context change replays the effect"
        );
    });
}

#[test]
fn failed_effect_retains_stopped_custody_without_a_success_receipt() {
    crate::transaction_auth::with_identity(Some(identity("alice", "named_bearer")), || {
        let dir = tempfile::tempdir().unwrap();
        let (owner, _) = prepared(dir.path(), "a");
        let body = json!({"graph": owner.route.graph, "turtle": "not valid turtle"});
        assert!(owner.knot_durable(&pin(&owner), &body, ID).is_err());
        let status = owner.knot_attempt_status(&pin(&owner), &body, ID).unwrap();
        assert_eq!(status["state"], "stopped");
        assert!(status.get("receipt").is_none());
        assert!(status.get("commit").is_none());
        assert!(owner.knot_durable(&pin(&owner), &body, ID).is_err());
        assert_eq!(values(&owner)["count"], 0);
    });
}

#[test]
fn unsupported_scoped_v2_and_bad_pins_refuse_before_creating_a_ledger() {
    let dir = tempfile::tempdir().unwrap();
    let (owner, _) = prepared(dir.path(), "a");
    let body = input(&owner, "denied");
    crate::transaction_auth::with_identity(
        Some(identity("machine", "scoped_named_bearer")),
        || {
            assert!(owner.knot_durable(&pin(&owner), &body, ID).is_err());
            assert!(
                owner.knot(&pin(&owner), &body).is_err(),
                "v1 must not bypass its missing graph grant"
            );
        },
    );
    crate::transaction_auth::with_identity(Some(identity("alice", "named_bearer")), || {
        let mut wrong = pin(&owner);
        wrong.owner_generation = Some(999);
        assert!(owner.knot_durable(&wrong, &body, ID).is_err());
        wrong = pin(&owner);
        wrong.signed = true;
        assert!(owner.knot_durable(&wrong, &body, ID).is_err());
        wrong = pin(&owner);
        wrong.cross_graph_context = true;
        assert!(owner.knot_durable(&wrong, &body, ID).is_err());
    });
    assert!(!owner.read(|store| Ok(has_ledger(store))).unwrap());
    assert_eq!(values(&owner)["count"], 0);
}
