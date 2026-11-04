use super::*;
use crate::graph_catalog::Budget;
use serde_json::json;
use std::{sync::mpsc, time::Duration};

fn fixture(root: &Path, name: &str) -> (Store, Route) {
    let path = root.join(format!("{name}.db"));
    let path = path.to_str().unwrap();
    let graph = format!("https://example.test/graphs/{name}");
    {
        let store = Store::open(path).unwrap();
        store.graph_create(&graph).unwrap();
    }
    Store::adopt(path).unwrap();
    let store = Store::open(path).unwrap();
    let route = Route {
        graph,
        file_id: name.into(),
        store_id: store.store_id().unwrap(),
        log_epoch: store.log_epoch().unwrap(),
        generation: 4,
        policy: "policy-v1".into(),
    };
    (store, route)
}

fn catalog(routes: Vec<Route>) -> Arc<Catalog> {
    Arc::new(
        Catalog::new(
            7,
            "legacy-store",
            routes,
            Budget {
                max_owners: 2,
                queued_per_owner: 1,
                readers_per_owner: 1,
                max_queued: 2,
                max_connections: 6,
            },
        )
        .unwrap(),
    )
}

fn request(owner: &Owner) -> WriteRequest<'_> {
    WriteRequest {
        graph: Some(&owner.route.graph),
        catalog_revision: 7,
        owner_generation: Some(4),
        policy: "policy-v1",
        signed: false,
        cross_graph_context: false,
    }
}

fn input(owner: &Owner, value: &str) -> Value {
    json!({
        "graph": owner.route.graph,
        "turtle": format!("<urn:test:subject> <urn:test:value> {value:?} ."),
        "replace_snapshot": true,
        "snapshot": "test-producer",
        "source": "test-producer",
    })
}

fn values(owner: &Owner) -> Value {
    owner
        .read(|store| {
            crate::tool_query(
                store,
                &json!({
                    "graph": owner.route.graph,
                    "query": "SELECT ?value WHERE { <urn:test:subject> <urn:test:value> ?value }"
                }),
            )
        })
        .unwrap()
}

#[test]
fn held_owner_does_not_block_another_owners_real_knot_and_point_read() {
    let temp = tempfile::tempdir().unwrap();
    let (a_store, a_route) = fixture(temp.path(), "a");
    let (b_store, b_route) = fixture(temp.path(), "b");
    let catalog = catalog(vec![a_route.clone(), b_route.clone()]);
    let a = Owner::prepare(
        catalog.clone(),
        temp.path(),
        &a_route.graph,
        a_store,
        "policy-v1",
    )
    .unwrap();
    let b = Arc::new(
        Owner::prepare(catalog, temp.path(), &b_route.graph, b_store, "policy-v1").unwrap(),
    );
    let held_a = a.writer.lock();
    let (tx, rx) = mpsc::channel();
    let worker = {
        let b = b.clone();
        std::thread::spawn(move || {
            let result = b.knot(&request(&b), &input(&b, "independent"));
            let read = values(&b);
            tx.send((result, read)).unwrap();
        })
    };
    // This assertion occurs while A is still held. Joining/releasing A first
    // would let a shared-writer implementation pass vacuously.
    let completed = rx.recv_timeout(Duration::from_secs(3));
    drop(held_a);
    worker.join().unwrap();
    let (written, read) = completed.expect("B queued behind unrelated owner A");
    assert!(written.is_ok(), "{written:?}");
    assert_eq!(read["count"], 1);
    assert_eq!(values(&a)["count"], 0, "write leaked to A");
    assert_eq!(a.admitted.load(Ordering::Acquire), 0);
}

#[test]
fn guarded_snapshot_replacement_and_refusal_keep_owner_scope() {
    let temp = tempfile::tempdir().unwrap();
    let (store, route) = fixture(temp.path(), "a");
    let owner = Owner::prepare(
        catalog(vec![route.clone()]),
        temp.path(),
        &route.graph,
        store,
        "policy-v1",
    )
    .unwrap();
    for value in ["old", "new"] {
        owner.knot(&request(&owner), &input(&owner, value)).unwrap();
        let result = values(&owner);
        assert_eq!(
            result["count"], 1,
            "snapshot was appended instead of replaced"
        );
        assert_eq!(result["rows"][0]["value"], value);
    }
    let before = values(&owner);
    let mut invalid = input(&owner, "should-not-land");
    invalid["snapshot"] = json!("");
    assert!(owner.knot(&request(&owner), &invalid).is_err());
    assert_eq!(values(&owner), before);
    invalid = input(&owner, "wrong-file");
    invalid["graph"] = json!("https://example.test/graphs/other");
    assert!(owner.knot(&request(&owner), &invalid).is_err());
    assert_eq!(values(&owner), before);
    assert_eq!(owner.admitted.load(Ordering::Acquire), 0);
}

#[test]
fn full_queue_and_reader_pool_refuse_without_writer_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let (store, route) = fixture(temp.path(), "a");
    let owner = Owner::prepare(
        catalog(vec![route.clone()]),
        temp.path(),
        &route.graph,
        store,
        "policy-v1",
    )
    .unwrap();
    let active = owner.admit().unwrap();
    let queued = owner.admit().unwrap();
    assert!(
        owner
            .knot(&request(&owner), &input(&owner, "full"))
            .is_err()
    );
    drop(queued);
    drop(active);
    owner
        .knot(&request(&owner), &input(&owner, "available"))
        .unwrap();
    let writer = owner.writer.lock();
    let reader = owner.readers[0].lock();
    // Keep the negative control bounded even if a future implementation adds
    // a blocking reader queue or writer fallback. Release only after observing
    // whether it returned while both connections were occupied.
    std::thread::scope(|scope| {
        let (tx, rx) = mpsc::channel();
        let owner = &owner;
        let worker = scope.spawn(move || tx.send(owner.read(|_| Ok(()))).unwrap());
        let result = rx.recv_timeout(Duration::from_secs(3));
        drop(reader);
        drop(writer);
        worker.join().unwrap();
        assert!(
            result
                .expect("read waited on an occupied connection")
                .is_err()
        );
    });
    assert_eq!(values(&owner)["count"], 1);
}

#[test]
fn duplicate_native_owner_and_replaced_placement_refuse() {
    let temp = tempfile::tempdir().unwrap();
    let (store, route) = fixture(temp.path(), "a");
    let catalog = catalog(vec![route.clone()]);
    let owner = Owner::prepare(
        catalog.clone(),
        temp.path(),
        &route.graph,
        store,
        "policy-v1",
    )
    .unwrap();
    let path = temp.path().join("a.db");
    let duplicate = Store::open(path.to_str().unwrap()).unwrap();
    assert!(Owner::prepare(catalog, temp.path(), &route.graph, duplicate, "policy-v1").is_err());
    owner
        .knot(&request(&owner), &input(&owner, "before"))
        .unwrap();
    let before: i64 = owner
        .writer
        .lock()
        .conn
        .query_row("SELECT count(*) FROM facts", [], |r| r.get(0))
        .unwrap();
    std::fs::rename(&path, temp.path().join("retained.db")).unwrap();
    File::create(&path).unwrap();
    assert!(
        owner
            .knot(&request(&owner), &input(&owner, "after"))
            .is_err()
    );
    assert!(owner.read(|_| Ok(())).is_err());
    let after: i64 = owner
        .writer
        .lock()
        .conn
        .query_row("SELECT count(*) FROM facts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(after, before, "placement refusal changed held database");
}

#[test]
fn inactive_native_ownership_wrong_pins_and_wrong_policy_refuse() {
    let temp = tempfile::tempdir().unwrap();
    for pin in ["store", "epoch", "policy", "file", "inactive"] {
        let name = format!("a-{pin}");
        let (store, mut route) = fixture(temp.path(), &name);
        match pin {
            "store" => route.store_id = "wrong-store".into(),
            "epoch" => route.log_epoch = "wrong-epoch".into(),
            "policy" => route.policy = "wrong-policy".into(),
            "file" => route.file_id = "wrong-file".into(),
            "inactive" => (),
            _ => unreachable!(),
        }
        let store = if pin == "inactive" {
            Store::open_in_memory().unwrap()
        } else {
            store
        };
        assert!(
            Owner::prepare(
                catalog(vec![route.clone()]),
                temp.path(),
                &route.graph,
                store,
                "policy-v1"
            )
            .is_err(),
            "{pin}"
        );
    }
}

#[test]
fn authenticated_identity_stays_on_the_committing_owner_and_signed_scope_refuses() {
    use crate::transaction_auth::{AttestationSlot, Identity, PendingAttestation, with_identity};
    let temp = tempfile::tempdir().unwrap();
    let (store, route) = fixture(temp.path(), "a");
    let owner = Owner::prepare(
        catalog(vec![route.clone()]),
        temp.path(),
        &route.graph,
        store,
        "policy-v1",
    )
    .unwrap();
    let identity = Identity {
        principal: "urn:test:authenticated".into(),
        credential_id: Some("test-credential".into()),
        auth_class: "named_bearer".into(),
        attestation: Default::default(),
    };
    let result = with_identity(Some(identity.clone()), || {
        owner.knot(&request(&owner), &input(&owner, "authenticated"))
    })
    .unwrap();
    let tx = result["tx_id"].as_i64().unwrap();
    assert_eq!(
        owner.read(|store| store.transaction_auth(tx)).unwrap(),
        Some(identity.clone())
    );
    let pending = PendingAttestation::new(
        "test-session".into(),
        "test-nonce".into(),
        "test-key".into(),
        "test-introducer".into(),
    );
    let signed = Identity {
        attestation: AttestationSlot(Some(pending.clone())),
        ..identity
    };
    let before = values(&owner);
    // The request flag is deliberately FALSE. The actual scoped identity must
    // refuse even when a caller incorrectly classified it as unsigned.
    assert!(
        with_identity(Some(signed), || {
            owner.knot(&request(&owner), &input(&owner, "signed"))
        })
        .is_err()
    );
    assert_eq!(
        pending.state(),
        crate::transaction_auth::AttestState::Unspent
    );
    assert_eq!(values(&owner), before);
}

#[test]
#[cfg(feature = "shacl")]
fn configured_shapes_are_enforced_through_the_real_knot_pipeline() {
    let temp = tempfile::tempdir().unwrap();
    let (mut store, route) = fixture(temp.path(), "a");
    store.set_base_ns("https://example.test/ontology/");
    store.shacl_config_mut().validate_on_write = true;
    store
        .load_shapes(
            "owner-control",
            r#"
        @prefix sh: <http://www.w3.org/ns/shacl#> .
        <urn:test:shape> a sh:NodeShape ; sh:targetClass <urn:test:Thing> ;
          sh:property [ sh:path <urn:test:name> ; sh:minCount 1 ; sh:maxCount 1 ] .
    "#,
            "2026-11-04T00:00:00Z",
        )
        .unwrap();
    let owner = Owner::prepare(
        catalog(vec![route.clone()]),
        temp.path(),
        &route.graph,
        store,
        "policy-v1",
    )
    .unwrap();
    let mut valid = input(&owner, "unused");
    valid["turtle"] = json!(r#"<urn:test:item> a <urn:test:Thing> ; <urn:test:name> "valid" ."#);
    owner.knot(&request(&owner), &valid).unwrap();
    let mut invalid = valid.clone();
    invalid["turtle"] = json!(r#"<urn:test:item> a <urn:test:Thing> ; <urn:test:name> "a", "b" ."#);
    // Payload SHACL refusal is a successful response with conforms:false.
    let refused = owner.knot(&request(&owner), &invalid).unwrap();
    assert_eq!(refused["conforms"], false);
    assert_eq!(refused["violations"], 1);
    let observed = owner
        .read(|store| {
            assert_eq!(store.base_ns(), "https://example.test/ontology/");
            crate::tool_query(
                store,
                &json!({"graph": owner.route.graph,
            "query": "SELECT ?name WHERE { <urn:test:item> <urn:test:name> ?name }"}),
            )
        })
        .unwrap();
    assert_eq!(observed["count"], 1);
    assert_eq!(observed["rows"][0]["name"], "valid");
}

#[test]
fn read_identity_and_callback_share_one_snapshot_and_release_after_errors() {
    let temp = tempfile::tempdir().unwrap();
    let (store, route) = fixture(temp.path(), "a");
    let owner = Owner::prepare(
        catalog(vec![route.clone()]),
        temp.path(),
        &route.graph,
        store,
        "policy-v1",
    )
    .unwrap();
    // A failed callback must not leak a transaction into the next request.
    assert!(
        owner
            .read::<()>(|_| Err(refused("fixture callback failure")))
            .is_err()
    );
    assert_eq!(owner.read(Store::log_epoch).unwrap(), route.log_epoch);
    let admitted_epoch = owner
        .read(|reader| {
            let changed = owner
                .writer
                .lock()
                .mint_log_epoch("private snapshot fixture")?;
            assert_ne!(changed, route.log_epoch);
            reader.log_epoch()
        })
        .unwrap();
    assert_eq!(
        admitted_epoch, route.log_epoch,
        "callback crossed the admitted epoch cut"
    );
    assert!(
        owner.read(Store::log_epoch).is_err(),
        "next admission accepted changed epoch"
    );
    assert!(
        owner.readers[0].lock().conn.is_autocommit(),
        "read transaction leaked"
    );
}

#[test]
fn real_queued_writes_are_bounded_without_stalling_another_owner() {
    let temp = tempfile::tempdir().unwrap();
    let (a_store, a_route) = fixture(temp.path(), "a");
    let (b_store, b_route) = fixture(temp.path(), "b");
    let catalog = catalog(vec![a_route.clone(), b_route.clone()]);
    let a = Arc::new(
        Owner::prepare(
            catalog.clone(),
            temp.path(),
            &a_route.graph,
            a_store,
            "policy-v1",
        )
        .unwrap(),
    );
    let b = Arc::new(
        Owner::prepare(catalog, temp.path(), &b_route.graph, b_store, "policy-v1").unwrap(),
    );
    let held = a.writer.lock();
    let mut queued = Vec::new();
    for value in ["first", "second"] {
        let a = a.clone();
        queued.push(std::thread::spawn(move || {
            a.knot(&request(&a), &input(&a, value))
        }));
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while a.admitted.load(Ordering::Acquire) != 2 && std::time::Instant::now() < deadline {
        std::thread::yield_now();
    }
    let admitted_while_held = a.admitted.load(Ordering::Acquire);
    let (overflow_tx, overflow_rx) = mpsc::channel();
    let overflow = {
        let a = a.clone();
        std::thread::spawn(move || {
            overflow_tx
                .send(a.knot(&request(&a), &input(&a, "overflow")))
                .unwrap();
        })
    };
    let (progress_tx, progress_rx) = mpsc::channel();
    let progress = {
        let b = b.clone();
        std::thread::spawn(move || {
            let written = b.knot(&request(&b), &input(&b, "independent"));
            progress_tx.send((written, values(&b))).unwrap();
        })
    };
    // Observe both results before releasing A. Always release and join before
    // asserting, so a broken bound/fallback cannot strand a fixture thread.
    let overflow_while_held = overflow_rx.recv_timeout(Duration::from_secs(3));
    let progress_while_held = progress_rx.recv_timeout(Duration::from_secs(3));
    drop(held);
    let completed: Vec<_> = queued
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    overflow.join().unwrap();
    progress.join().unwrap();
    assert_eq!(
        admitted_while_held, 2,
        "real calls did not fill A's admission bound"
    );
    let refused = overflow_while_held.expect("overflow waited on A's writer");
    assert!(refused.unwrap_err().to_string().contains("mailbox full"));
    let (written, read) = progress_while_held.expect("B waited behind A's queued writes");
    assert!(written.is_ok(), "{written:?}");
    assert_eq!(read["count"], 1);
    assert!(completed.iter().all(Result::is_ok), "{completed:?}");
    assert_eq!(a.admitted.load(Ordering::Acquire), 0);
    let a_values = values(&a);
    assert_eq!(a_values["count"], 1, "queued snapshots were appended");
    assert_ne!(a_values["rows"][0]["value"], "overflow");
}
