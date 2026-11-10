use super::*;
use crate::graph_catalog::Budget;
use serde_json::json;
use std::{fs::File, sync::mpsc, time::Duration};

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
    assert!(matches!(
        Store::open(path.to_str().unwrap()),
        Err(crate::Error::PolicyDenied(_))
    ));
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

#[test]
fn blocked_embedding_releases_writer_and_preserves_independent_owner_progress() {
    use crate::vector::KnowledgeVectorStore;
    struct LatchedProvider {
        entered: mpsc::Sender<()>,
        release: std::sync::Mutex<mpsc::Receiver<()>>,
    }
    impl crate::EmbeddingProvider for LatchedProvider {
        fn embed_text(&self, _text: &str) -> crate::Result<Vec<f32>> {
            self.entered.send(()).map_err(refused)?;
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .map_err(refused)?;
            Ok(vec![0.5; 8])
        }
        fn dimension(&self) -> usize {
            8
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let (mut a_store, a_route) = fixture(temp.path(), "a");
    let (b_store, b_route) = fixture(temp.path(), "b");
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    a_store.set_embedding_provider(Arc::new(LatchedProvider {
        entered: entered_tx,
        release: std::sync::Mutex::new(release_rx),
    }));
    a_store.embedding_config_mut().auto_embed = true;
    // Named-graph embedding text is an explicit configured consumer choice.
    // ROOT-only defaults intentionally produce no work for this graph.
    a_store.search_config_mut().named_graphs = true;
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
    let writer = {
        let a = a.clone();
        std::thread::spawn(move || a.knot(&request(&a), &input(&a, "embedding")))
    };
    let entered = entered_rx.recv_timeout(Duration::from_secs(3));
    let writer_free = a.writer.try_lock().is_some();
    let reserved = a.admitted.load(Ordering::Acquire);
    let (done_tx, done_rx) = mpsc::channel();
    let independent = {
        let b = b.clone();
        std::thread::spawn(move || {
            let written = b.knot(&request(&b), &input(&b, "independent"));
            done_tx.send((written, values(&b))).unwrap();
        })
    };
    let completed_before_release = done_rx.recv_timeout(Duration::from_secs(3));
    // Release even on failed observations; no assertion may strand inference.
    let _ = release_tx.send(());
    let written = writer.join().unwrap();
    independent.join().unwrap();
    entered.expect("real knot never reached the configured provider");
    assert!(writer_free, "inference held A's writer");
    assert_eq!(reserved, 1, "inference released its admission reservation");
    let (b_write, b_read) = completed_before_release.expect("B blocked behind A inference");
    assert!(b_write.is_ok(), "{b_write:?}");
    assert_eq!(b_read["count"], 1);
    assert!(written.is_ok(), "{written:?}");
    assert_eq!(a.read(KnowledgeVectorStore::vector_count).unwrap(), 1);
    assert_eq!(b.read(KnowledgeVectorStore::vector_count).unwrap(), 0);
    assert_eq!(a.admitted.load(Ordering::Acquire), 0);
}

// arming: library -- native owner admission regression controls.
fn qualification_probe_ordinary_writer(held_before_owner: bool) {
    let temp = tempfile::tempdir().unwrap();
    let (store, route) = fixture(temp.path(), "probe");
    let path = temp.path().join("probe.db");
    let path = path.to_str().unwrap();
    let mut ordinary = held_before_owner.then(|| Store::open(path).unwrap());
    let prepared = Owner::prepare(
        catalog(vec![route.clone()]),
        temp.path(),
        &route.graph,
        store,
        "policy-v1",
    );
    let owner = match prepared {
        Err(crate::Error::PolicyDenied(_)) if held_before_owner => return,
        Err(error) => panic!("owner positive admission unproved: {error:?}"),
        Ok(owner) => owner,
    };
    owner
        .knot(&request(&owner), &input(&owner, "owner-control"))
        .unwrap();
    let before = values(&owner);
    assert!(
        before.to_string().contains("owner-control"),
        "owner point-read control missing"
    );
    if ordinary.is_none() {
        ordinary = match Store::open(path) {
            Err(crate::Error::PolicyDenied(_)) => return,
            Err(error) => panic!("ordinary open has an unrelated failure: {error:?}"),
            Ok(store) => Some(store),
        };
    }
    let result = crate::tool_knot(
        ordinary.as_mut().unwrap(),
        &input(&owner, "ordinary-bypass"),
    );
    let after = values(&owner);
    assert!(
        matches!(&result, Err(crate::Error::PolicyDenied(_))) && after == before,
        "ordinary native write escaped prepared owner; held_before_owner={held_before_owner}; actual_result={result:?}; before={before}; after={after}"
    );
}

#[test]
fn qualification_ordinary_new_handle_cannot_bypass_owner() {
    qualification_probe_ordinary_writer(false);
}

#[test]
fn qualification_ordinary_already_open_handle_cannot_bypass_owner() {
    qualification_probe_ordinary_writer(true);
}

#[test]
fn ordinary_multi_handle_writes_and_readers_remain_available_without_owner() {
    let temp = tempfile::tempdir().unwrap();
    let (mut first, route) = fixture(temp.path(), "ordinary");
    let path = temp.path().join("ordinary.db");
    let mut second = Store::open(path.to_str().unwrap()).unwrap();
    for (store, value) in [(&mut first, "first"), (&mut second, "second")] {
        crate::tool_knot(
            store,
            &json!({"graph": route.graph,
            "turtle": format!("<urn:test:subject> <urn:test:value> {value:?} ."),
            "snapshot": "test-producer", "replace_snapshot": true,
            "source": "test-producer"}),
        )
        .unwrap();
    }
    let reader = Store::open_read_only(path.to_str().unwrap()).unwrap();
    let result = crate::tool_query(
        &reader,
        &json!({"graph": route.graph,
        "query": "SELECT ?value WHERE { <urn:test:subject> <urn:test:value> ?value }"}),
    )
    .unwrap();
    assert_eq!(result["count"], 1);
    assert_eq!(result["rows"][0]["value"], "second");
}

#[test]
fn live_owner_refuses_adoption_without_epoch_change_and_releases_lease_on_drop() {
    let temp = tempfile::tempdir().unwrap();
    let (store, route) = fixture(temp.path(), "lifetime");
    let path = temp.path().join("lifetime.db");
    let path = path.to_str().unwrap();
    let owner = Owner::prepare(
        catalog(vec![route.clone()]),
        temp.path(),
        &route.graph,
        store,
        "policy-v1",
    )
    .unwrap();
    owner
        .knot(&request(&owner), &input(&owner, "owner-control"))
        .unwrap();
    let before = values(&owner);
    assert!(matches!(
        Store::adopt(path),
        Err(crate::Error::PolicyDenied(_))
    ));
    let reader = Store::open_read_only(path).unwrap();
    assert_eq!(reader.log_epoch().unwrap(), route.log_epoch);
    assert_eq!(values(&owner), before);
    drop(owner);
    // A lifetime lease is intentionally not durable owner enrollment. This
    // control prevents claiming that it survives process exit or retirement.
    let mut ordinary = Store::open(path).unwrap();
    crate::tool_knot(
        &mut ordinary,
        &json!({"graph": route.graph,
        "turtle": "<urn:test:subject> <urn:test:value> \"after-drop\" .",
        "snapshot": "test-producer", "replace_snapshot": true,
        "source": "test-producer"}),
    )
    .unwrap();
    let after = crate::tool_query(
        &reader,
        &json!({"graph": route.graph,
        "query": "SELECT ?value WHERE { <urn:test:subject> <urn:test:value> ?value }"}),
    )
    .unwrap();
    assert_eq!(after["count"], 1);
    assert_eq!(after["rows"][0]["value"], "after-drop");
}

fn strict_fixture(root: &Path) -> (Owner, Owner) {
    let (a, ar) = fixture(root, "a");
    let (b, br) = fixture(root, "b");
    let catalog = catalog(vec![ar.clone(), br.clone()]);
    let a = Owner::prepare(catalog.clone(), root, &ar.graph, a, "policy-v1").unwrap();
    let b = Owner::prepare(catalog, root, &br.graph, b, "policy-v1").unwrap();
    (a, b)
}
fn strict_deadline() -> std::time::Instant {
    std::time::Instant::now() + Duration::from_secs(3)
}
fn captured_value(read: &CapturedRead<'_>) -> Value {
    crate::tool_query(
        read.store,
        &json!({"graph": read.route.graph,
        "query": "SELECT ?value WHERE { <urn:test:subject> <urn:test:value> ?value }"}),
    )
    .unwrap()
}

#[test]
fn strict_cut_retains_all_snapshots_after_releasing_commit_fences() {
    let temp = tempfile::tempdir().unwrap();
    let (a, b) = strict_fixture(temp.path());
    for (owner, value) in [(&a, "old-a"), (&b, "old-b")] {
        owner.knot(&request(owner), &input(owner, value)).unwrap();
    }
    let heads = [
        a.read(Store::latest_tx_id).unwrap(),
        b.read(Store::latest_tx_id).unwrap(),
    ];
    Owner::read_strict(&[&b, &a], strict_deadline(), |reads| {
        assert_eq!(reads.len(), 2);
        for (i, owner) in [&a, &b].into_iter().enumerate() {
            assert_eq!(reads[i].route, &owner.route);
            assert_eq!(reads[i].catalog_revision, 7);
            assert_eq!(reads[i].local_tx, heads[i]);
            assert!(
                owner.writer.try_lock().is_some(),
                "evaluation retained a commit fence"
            );
            assert!(
                !reads[i].store.conn.is_autocommit(),
                "capture lost its SQLite snapshot"
            );
        }
        a.knot(&request(&a), &input(&a, "new-a"))?;
        b.knot(&request(&b), &input(&b, "new-b"))?;
        for (i, old) in ["old-a", "old-b"].into_iter().enumerate() {
            let value = captured_value(&reads[i]);
            assert_eq!(value["count"], 1);
            assert_eq!(value["rows"][0]["value"], old);
            assert_eq!(reads[i].store.latest_tx_id()?, heads[i]);
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(values(&a)["rows"][0]["value"], "new-a");
    assert_eq!(values(&b)["rows"][0]["value"], "new-b");
}

#[test]
fn strict_cut_contention_releases_partial_admission_without_waiting_or_partial_callback() {
    let temp = tempfile::tempdir().unwrap();
    let (a, b) = strict_fixture(temp.path());
    let occupied = b.writer.lock();
    let invoked = std::cell::Cell::new(false);
    assert!(
        Owner::read_strict(&[&a, &b], strict_deadline(), |_| {
            invoked.set(true);
            Ok(())
        })
        .is_err()
    );
    assert!(!invoked.get());
    assert!(a.writer.try_lock().is_some());
    // An unselected hot owner must not participate in this barrier.
    Owner::read_strict(&[&a], strict_deadline(), |_| Ok(())).unwrap();
    drop(occupied);
    let occupied = b.readers[0].lock();
    assert!(
        Owner::read_strict(&[&a, &b], strict_deadline(), |_| {
            invoked.set(true);
            Ok(())
        })
        .is_err()
    );
    assert!(!invoked.get());
    assert!(a.writer.try_lock().is_some());
    assert!(b.writer.try_lock().is_some());
    assert!(a.readers[0].lock().conn.is_autocommit());
    drop(occupied);
    Owner::read_strict(&[&a, &b], strict_deadline(), |_| Ok(())).unwrap();
}

#[test]
fn strict_cut_refuses_invalid_selection_and_expired_deadline_before_callback() {
    let temp = tempfile::tempdir().unwrap();
    let (a, b) = strict_fixture(temp.path());
    let mut invoked = false;
    for selected in [vec![], vec![&a, &a]] {
        assert!(
            Owner::read_strict(&selected, strict_deadline(), |_| {
                invoked = true;
                Ok(())
            })
            .is_err()
        );
    }
    assert!(
        Owner::read_strict(
            &[&a, &b],
            std::time::Instant::now() - Duration::from_secs(1),
            |_| {
                invoked = true;
                Ok(())
            }
        )
        .is_err()
    );
    assert!(!invoked);
    Owner::read_strict(&[&a, &b], strict_deadline(), |_| Ok(())).unwrap();
}

#[test]
fn strict_cut_callback_and_late_identity_failures_release_every_transaction() {
    let temp = tempfile::tempdir().unwrap();
    let (a, b) = strict_fixture(temp.path());
    let mut invoked = false;
    assert!(
        Owner::read_strict::<()>(&[&a, &b], strict_deadline(), |_| {
            invoked = true;
            Err(refused("test callback failure"))
        })
        .is_err()
    );
    assert!(invoked, "negative callback never reached");
    for owner in [&a, &b] {
        assert!(owner.readers[0].lock().conn.is_autocommit());
        assert!(owner.writer.try_lock().is_some());
    }
    Owner::read_strict(&[&a, &b], strict_deadline(), |_| Ok(())).unwrap();
    b.writer
        .lock()
        .mint_log_epoch("private strict-cut identity failure")
        .unwrap();
    invoked = false;
    assert!(
        Owner::read_strict(&[&a, &b], strict_deadline(), |_| {
            invoked = true;
            Ok(())
        })
        .is_err()
    );
    assert!(!invoked);
    for owner in [&a, &b] {
        assert!(owner.readers[0].lock().conn.is_autocommit());
        assert!(owner.writer.try_lock().is_some());
    }
}

#[test]
fn strict_cut_refuses_late_success_and_releases_all_snapshots() {
    let temp = tempfile::tempdir().unwrap();
    let (a, b) = strict_fixture(temp.path());
    let deadline = strict_deadline();
    let mut invoked = false;
    let result = Owner::read_strict(&[&a, &b], deadline, |_| {
        invoked = true;
        while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
            std::thread::sleep(remaining);
        }
        Ok("late answer")
    });
    assert!(
        invoked,
        "callback must be admitted before testing late success"
    );
    assert!(
        result.is_err(),
        "a result past its deadline must not escape"
    );
    for owner in [&a, &b] {
        assert!(owner.readers[0].lock().conn.is_autocommit());
        assert!(owner.writer.try_lock().is_some());
    }
    Owner::read_strict(&[&a, &b], strict_deadline(), |_| Ok(())).unwrap();
}

#[test]
fn strict_cut_pins_post_open_dictionary_growth_and_canonical_reference_values() {
    let temp = tempfile::tempdir().unwrap();
    let (a, b) = strict_fixture(temp.path());
    // Readers already exist. Give the same new IRI different file-local IDs;
    // neither a startup dictionary nor raw numeric identity is a valid oracle.
    a.writer
        .lock()
        .intern("urn:test:dictionary-padding")
        .unwrap();
    let turtle = "<urn:test:late-subject> <urn:test:late-predicate> <urn:test:late-object> .";
    for owner in [&a, &b] {
        owner
            .knot(
                &request(owner),
                &json!({
                    "graph": owner.route.graph, "turtle": turtle,
                    "source": "test-producer", "snapshot": "late-dictionary",
                    "replace_snapshot": true,
                }),
            )
            .unwrap();
    }
    let iri = "urn:test:late-object";
    let a_id = a.writer.lock().lookup(iri).unwrap().unwrap();
    let b_id = b.writer.lock().lookup(iri).unwrap().unwrap();
    assert_ne!(a_id, b_id, "fixture must distinguish file-local IDs");
    let query = |read: &CapturedRead<'_>| {
        crate::tool_query(
            read.store,
            &json!({
                "graph": read.route.graph,
                "query": "SELECT ?object WHERE { <urn:test:late-subject> <urn:test:late-predicate> ?object }",
            }),
        )
    };
    Owner::read_strict(&[&a, &b], strict_deadline(), |reads| {
        for read in reads {
            let result = query(read)?;
            assert_eq!(result["count"], 1);
            assert_eq!(result["rows"][0]["object"], iri);
        }
        // Add entirely new dictionary terms after the common cut. The old
        // snapshots must resolve the old Ref while a later cut sees the new one.
        for owner in [&a, &b] {
            owner.knot(&request(owner), &json!({
                "graph": owner.route.graph,
                "turtle": "<urn:test:late-subject> <urn:test:late-predicate> <urn:test:new-object> .",
                "source": "test-producer", "snapshot": "late-dictionary",
                "replace_snapshot": true,
            }))?;
        }
        for read in reads {
            assert_eq!(query(read)?["rows"][0]["object"], iri);
            assert!(read.store.lookup("urn:test:new-object")?.is_none());
        }
        Ok(())
    }).unwrap();
    Owner::read_strict(&[&a, &b], strict_deadline(), |reads| {
        for read in reads {
            let result = query(read)?;
            assert_eq!(result["count"], 1);
            assert_eq!(result["rows"][0]["object"], "urn:test:new-object");
        }
        Ok(())
    })
    .unwrap();
}

#[test]
fn owner_memory_plan_reaches_writer_fence_and_every_reader() {
    let temp = tempfile::tempdir().unwrap();
    let (a, ar) = fixture(temp.path(), "a");
    let (b, br) = fixture(temp.path(), "b");
    let catalog = catalog(vec![ar.clone(), br.clone()]);
    let allocation = catalog.memory_allocation(512, 1, 1).unwrap();
    assert_eq!(allocation.connections, 8);
    let a = Owner::prepare_with_memory(
        catalog.clone(),
        temp.path(),
        &ar.graph,
        a,
        "policy-v1",
        allocation,
    )
    .unwrap();
    let b = Owner::prepare_with_memory(catalog, temp.path(), &br.graph, b, "policy-v1", allocation)
        .unwrap();
    let expected = (-(allocation.cache_kib as i64), allocation.mmap_bytes as i64);
    for owner in [&a, &b] {
        let writer = owner.writer.lock();
        let settings = |store: &Store| {
            (
                store
                    .conn
                    .query_row("PRAGMA cache_size", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                store
                    .conn
                    .query_row("PRAGMA mmap_size", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
            )
        };
        assert_eq!(settings(&writer), expected);
        assert_eq!(
            writer.ownership_fence.as_ref().unwrap().memory_settings(),
            expected
        );
        for reader in &owner.readers {
            let reader = reader.lock();
            assert_eq!(settings(&reader), expected);
            assert_eq!(
                reader
                    .conn
                    .query_row("PRAGMA query_only", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
    }
    // Allocation must not weaken ownership callbacks or change graph visibility.
    for owner in [&a, &b] {
        owner
            .knot(&request(owner), &input(owner, "budgeted"))
            .unwrap();
        assert_eq!(values(owner)["rows"][0]["value"], "budgeted");
    }
}

#[test]
fn budgeted_owner_refuses_missing_fence_reader() {
    let temp = tempfile::tempdir().unwrap();
    let (mut store, route) = fixture(temp.path(), "a");
    let catalog = catalog(vec![route.clone()]);
    let allocation = catalog.memory_allocation(320, 1, 1).unwrap();
    store.ownership_fence = None;
    assert!(
        Owner::prepare_with_memory(
            catalog,
            temp.path(),
            &route.graph,
            store,
            "policy-v1",
            allocation,
        )
        .is_err()
    );
}

fn composition_authority(
    graphs: &[&str],
) -> (
    crate::read_authorization::OwnerReadPolicy,
    crate::read_authorization::ReadIdentity,
) {
    use crate::read_authorization::*;
    use sha2::{Digest, Sha256};
    let registry = crate::crew_credentials::CredentialRegistry::parse(
        &serde_json::to_vec(&json!({"version":1,"credentials":[{
            "credential_id":"composition-fixture", "principal":"urn:principal:reader",
            "audience":"quipu", "token_sha256":format!("{:x}", Sha256::digest(b"composition-fixture-token"))
        }]})).unwrap(), "quipu",
    ).unwrap();
    let identity =
        ReadIdentity::authenticate_named(&registry, Some("Bearer composition-fixture-token"))
            .unwrap();
    let mut policy = OwnerReadPolicy::open(":memory:", "urn:principal:owner").unwrap();
    policy
        .apply(
            &ReadGrant {
                version: 1,
                id: "urn:grant:composition".into(),
                principal: "urn:principal:reader".into(),
                graphs: graphs.iter().map(|s| (*s).to_owned()).collect(),
                operations: [ReadOperation::Query, ReadOperation::Export].into(),
                valid_from: 1,
                valid_until: 100,
                max_offline_seconds: 300,
                issuer: "urn:principal:owner".into(),
                revision: 1,
                revoked: false,
            },
            "private composed fixture",
        )
        .unwrap();
    (policy, identity)
}

#[test]
fn composed_cut_joins_canonical_terms_with_bags_and_exact_graph_grants() {
    use crate::read_authorization::ReadOperation;
    let temp = tempfile::tempdir().unwrap();
    let (a, b) = strict_fixture(temp.path());
    for i in 0..32 {
        b.writer.lock().intern(&format!("urn:padding:{i}")).unwrap();
    }
    let turtle_a = "<urn:left:1> <urn:link> <urn:shared> . <urn:left:2> <urn:link> <urn:shared> .";
    let turtle_b = "<urn:shared> <urn:value> \"answer\"@en . <urn:shared> <urn:typed> \"007\"^^<http://www.w3.org/2001/XMLSchema#integer> .";
    let mut baseline = Store::open_in_memory().unwrap();
    for (owner, text) in [(&a, turtle_a), (&b, turtle_b)] {
        owner
            .knot(
                &request(owner),
                &json!({"graph":owner.route.graph,"turtle":text}),
            )
            .unwrap();
        let graph = baseline.graph_create(&owner.route.graph).unwrap();
        crate::rdf::ingest_assigned_rdf(
            &mut baseline,
            text.as_bytes(),
            oxrdfio::RdfFormat::Turtle,
            None,
            "1970-01-01T00:00:00Z",
            None,
            None,
            graph,
        )
        .unwrap();
    }
    assert_ne!(
        a.writer.lock().lookup("urn:shared").unwrap(),
        b.writer.lock().lookup("urn:shared").unwrap()
    );
    let (policy, identity) = composition_authority(&[&a.route.graph, &b.route.graph]);
    let policy = policy.snapshot().unwrap();
    let allocation = a.catalog.memory_allocation(512, 1, 1).unwrap();
    let deadline = strict_deadline();
    Owner::read_strict(&[&b, &a], deadline, |reads| {
        let view = policy.authorize(&identity, ReadOperation::Query, None, 2)?
            .open_composed_cut(reads, allocation, 2, deadline)?;
        let reference = policy.authorize(&identity, ReadOperation::Query, None, 2)?.open(&baseline, 2)?;
        for query in [
            format!("SELECT ?s ?value WHERE {{ GRAPH <{}> {{ ?s <urn:link> ?x }} GRAPH <{}> {{ ?x <urn:value> ?value }} }} ORDER BY ?s", a.route.graph, b.route.graph),
            format!("SELECT (COUNT(*) AS ?n) WHERE {{ GRAPH <{}> {{ ?s <urn:link> ?x }} GRAPH <{}> {{ ?x <urn:value> ?value }} }}", a.route.graph, b.route.graph),
            format!("SELECT ?v WHERE {{ GRAPH <{}> {{ <urn:shared> <urn:typed> ?v }} }}", b.route.graph),
            "SELECT (COUNT(*) AS ?n) WHERE { ?s ?p ?o }".into(),
        ] {
            assert_eq!(view.query(&query, false, 2)?, reference.query(&query, false, 2)?, "{query}");
            assert_eq!(view.query(&query, true, 2)?, reference.query(&query, true, 2)?, "resident: {query}");
        }
        for query in [
            "ASK { GRAPH <urn:hidden> { ?s ?p ?o } }",
            "ASK FROM <urn:hidden> { ?s ?p ?o }",
            "ASK { FILTER NOT EXISTS { GRAPH <urn:hidden> { ?s ?p ?o } } }",
            "ASK { SERVICE <https://example.invalid/> { ?s ?p ?o } }",
        ] { assert!(view.query(query, false, 2).is_err()); }
        view.recheck(&policy, &identity, reads, 2)?;
        assert!(view.recheck(&policy, &identity, &reads[..1], 2).is_err());
        Ok(())
    }).unwrap();
}

#[test]
fn composed_cut_refuses_scope_operation_expiry_deadline_and_oversized_values() {
    use crate::read_authorization::ReadOperation;
    let temp = tempfile::tempdir().unwrap();
    let (a, b) = strict_fixture(temp.path());
    let (policy, identity) = composition_authority(&[&a.route.graph, &b.route.graph]);
    let policy = policy.snapshot().unwrap();
    let (subset, _) = composition_authority(&[&a.route.graph]);
    let subset = subset.snapshot().unwrap();
    let allocation = a.catalog.memory_allocation(512, 1, 1).unwrap();
    Owner::read_strict(&[&a, &b], strict_deadline(), |reads| {
        assert!(
            subset
                .authorize(&identity, ReadOperation::Query, None, 2)?
                .open_composed_cut(reads, allocation, 2, strict_deadline())
                .is_err()
        );
        assert!(
            policy
                .authorize(&identity, ReadOperation::Query, None, 2)?
                .open_composed_cut(&reads[..1], allocation, 2, strict_deadline())
                .is_err()
        );
        assert!(
            policy
                .authorize(&identity, ReadOperation::Export, None, 2)?
                .open_composed_cut(reads, allocation, 2, strict_deadline())
                .is_err()
        );
        assert!(
            policy
                .authorize(&identity, ReadOperation::Query, None, 2)?
                .open_composed_cut(reads, allocation, 100, strict_deadline())
                .is_err()
        );
        assert!(
            policy
                .authorize(&identity, ReadOperation::Query, None, 2)?
                .open_composed_cut(reads, allocation, 2, std::time::Instant::now())
                .is_err()
        );
        Ok(())
    })
    .unwrap();
    a.knot(&request(&a), &input(&a, &"x".repeat(65_536)))
        .unwrap();
    Owner::read_strict(&[&a, &b], strict_deadline(), |reads| {
        assert!(
            policy
                .authorize(&identity, ReadOperation::Query, None, 2)?
                .open_composed_cut(reads, allocation, 2, strict_deadline())
                .is_err()
        );
        Ok(())
    })
    .unwrap();
    Owner::read_strict(&[&a, &b], strict_deadline(), |_| Ok(())).unwrap();
}

#[test]
fn composed_cut_recheck_binds_generation_policy_and_expired_buffer() {
    use crate::read_authorization::{ReadGrant, ReadOperation};
    let temp = tempfile::tempdir().unwrap();
    let (a, b) = strict_fixture(temp.path());
    let graphs = [&a.route.graph, &b.route.graph];
    let (mut owner_policy, identity) = composition_authority(&[graphs[0], graphs[1]]);
    let policy = owner_policy.snapshot().unwrap();
    let allocation = a.catalog.memory_allocation(512, 1, 1).unwrap();
    Owner::read_strict(&[&a, &b], strict_deadline(), |reads| {
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        let view = policy
            .authorize(&identity, ReadOperation::Query, None, 2)?
            .open_composed_cut(reads, allocation, 2, deadline)?;
        for field in ["generation", "policy", "file"] {
            let mut route = reads[0].route.clone();
            match field {
                "generation" => route.generation += 1,
                "policy" => route.policy.push_str("-changed"),
                _ => route.file_id.push_str("-changed"),
            }
            let changed = [
                CapturedRead {
                    route: &route,
                    catalog_revision: reads[0].catalog_revision,
                    local_tx: reads[0].local_tx,
                    store: reads[0].store,
                },
                CapturedRead {
                    route: reads[1].route,
                    catalog_revision: reads[1].catalog_revision,
                    local_tx: reads[1].local_tx,
                    store: reads[1].store,
                },
            ];
            assert!(
                view.recheck(&policy, &identity, &changed, 2).is_err(),
                "{field}"
            );
        }
        view.recheck(&policy, &identity, reads, 2)?;
        owner_policy.apply(
            &ReadGrant {
                version: 1,
                id: "urn:grant:composition".into(),
                principal: "urn:principal:reader".into(),
                graphs: graphs.iter().map(|s| (*s).clone()).collect(),
                operations: [ReadOperation::Query, ReadOperation::Export].into(),
                valid_from: 1,
                valid_until: 100,
                max_offline_seconds: 300,
                issuer: "urn:principal:owner".into(),
                revision: 2,
                revoked: true,
            },
            "private revocation control",
        )?;
        assert!(
            view.recheck(&owner_policy.snapshot()?, &identity, reads, 2)
                .is_err()
        );
        while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
            std::thread::sleep(remaining);
        }
        assert!(view.query("ASK { ?s ?p ?o }", false, 2).is_err());
        assert!(view.recheck(&policy, &identity, reads, 2).is_err());
        Ok(())
    })
    .unwrap();
}
