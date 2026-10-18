use super::*;

const TS: &str = "2026-01-01T00:00:00Z";

fn input() -> Commit {
    Commit {
        owner: Owner {
            deployment: "urn:example:deployment".into(),
            project: "urn:example:project".into(),
            crew: "urn:example:alpha".into(),
        },
        path: "topics/one.md".into(),
        text: Some("\u{feff}# Unicode λ\r\n[topic](two.md)\r\n".into()),
        expected_snapshot: None,
        expected_file: None,
        idempotency_key: "one".into(),
        harness: "fixture".into(),
        account: "selected".into(),
    }
}

fn named() -> crate::crew_credentials::CrewPrincipal {
    crate::crew_credentials::CrewPrincipal {
        iri: "urn:example:alpha".into(),
        credential_id: "fixture-alpha".into(),
    }
}

#[test]
fn exact_text_and_owner_readback_are_not_a_summary() {
    let mut store = Store::open_in_memory().unwrap();
    let request = input();
    assert!(read(&store, 0, &request.owner).unwrap().is_none());
    let receipt = commit(&mut store, 0, &request, Authority::Named(&named()), TS).unwrap();
    let head = read(&store, 0, &request.owner).unwrap().unwrap();
    assert_eq!(head.revision, receipt.snapshot);
    assert_eq!(head.files[&request.path], receipt.file);
    assert_eq!(revision(&store, 0, &receipt).unwrap().text, request.text);
    let mut other = request.owner.clone();
    other.crew = "urn:example:beta".into();
    assert!(read(&store, 0, &other).unwrap().is_none());
}

#[test]
fn lost_response_replay_precedes_now_stale_snapshot_and_file_heads() {
    let mut store = Store::open_in_memory().unwrap();
    let request = input();
    let first = commit(&mut store, 0, &request, Authority::Local, TS).unwrap();
    let mut second = request.clone();
    second.idempotency_key = "two".into();
    second.path = "two.md".into();
    second.expected_snapshot = Some(first.snapshot.clone());
    commit(&mut store, 0, &second, Authority::Local, TS).unwrap();
    let txs = store.list_transactions().unwrap().len();
    assert_eq!(
        commit(&mut store, 0, &request, Authority::Local, TS).unwrap(),
        first
    );
    assert_eq!(store.list_transactions().unwrap().len(), txs);
    let mut changed = request;
    changed.text = Some("different intent".into());
    assert!(commit(&mut store, 0, &changed, Authority::Local, TS).is_err());
    assert_eq!(store.list_transactions().unwrap().len(), txs);
}

#[test]
fn unchanged_bytes_with_new_stale_intent_still_conflict() {
    let mut store = Store::open_in_memory().unwrap();
    let mut request = input();
    commit(&mut store, 0, &request, Authority::Local, TS).unwrap();
    request.idempotency_key = "not-a-replay".into();
    assert!(commit(&mut store, 0, &request, Authority::Local, TS).is_err());
}

#[test]
fn unproven_or_wrong_named_writer_cannot_commit_any_facts() {
    let mut store = Store::open_in_memory().unwrap();
    let request = input();
    assert!(commit(&mut store, 0, &request, Authority::Unproven, TS).is_err());
    let mut wrong = named();
    wrong.iri = "urn:example:beta".into();
    assert!(commit(&mut store, 0, &request, Authority::Named(&wrong), TS).is_err());
    assert!(store.current_facts().unwrap().is_empty());
    assert!(store.list_transactions().unwrap().is_empty());
}

#[test]
fn explicit_tombstone_restore_keeps_old_version_recoverable() {
    let mut store = Store::open_in_memory().unwrap();
    let mut request = input();
    let first = commit(&mut store, 0, &request, Authority::Local, TS).unwrap();
    request.expected_snapshot = Some(first.snapshot.clone());
    request.expected_file = Some(first.file.revision.clone());
    request.text = None;
    request.idempotency_key = "delete".into();
    let deleted = commit(&mut store, 0, &request, Authority::Local, TS).unwrap();
    assert!(deleted.file.tombstone);
    assert!(revision(&store, 0, &deleted).unwrap().text.is_none());
    request.expected_snapshot = Some(deleted.snapshot);
    request.expected_file = Some(deleted.file.revision);
    request.text = Some("restored".into());
    request.idempotency_key = "restore".into();
    let restored = commit(&mut store, 0, &request, Authority::Local, TS).unwrap();
    assert!(!restored.file.tombstone);
    assert_eq!(revision(&store, 0, &first).unwrap().text, input().text);
}

#[test]
fn unsafe_path_oversize_and_missing_parent_refuse_without_head_change() {
    let mut store = Store::open_in_memory().unwrap();
    for path in ["../x", "/x", "a//x", "a\\x", "./x"] {
        let mut request = input();
        request.path = path.into();
        assert!(commit(&mut store, 0, &request, Authority::Local, TS).is_err());
    }
    let mut request = input();
    request.text = Some("x".repeat(MAX_FILE_BYTES + 1));
    assert!(commit(&mut store, 0, &request, Authority::Local, TS).is_err());
    request.text = None;
    assert!(commit(&mut store, 0, &request, Authority::Local, TS).is_err());
    assert!(read(&store, 0, &request.owner).unwrap().is_none());
    assert!(store.list_transactions().unwrap().is_empty());
}

#[test]
fn two_independent_connections_same_base_have_one_winner() {
    use std::sync::{Arc, Barrier};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fixture.db");
    Store::open(path.to_str().unwrap()).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let threads = (0..2)
        .map(|n| {
            let barrier = barrier.clone();
            let path = path.clone();
            std::thread::spawn(move || {
                let mut store = Store::open(path.to_str().unwrap()).unwrap();
                let mut request = input();
                request.idempotency_key = format!("writer-{n}");
                // Different files still contend on the complete owner snapshot.
                request.path = format!("{n}.md");
                barrier.wait();
                commit(&mut store, 0, &request, Authority::Local, TS).is_ok()
            })
        })
        .collect::<Vec<_>>();
    let winners = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .filter(|ok| *ok)
        .count();
    assert_eq!(winners, 1);
    let store = Store::open(path.to_str().unwrap()).unwrap();
    assert_eq!(
        read(&store, 0, &input().owner)
            .unwrap()
            .unwrap()
            .files
            .len(),
        1
    );
}

#[test]
fn graph_scope_and_mutated_readback_are_checked() {
    let mut store = Store::open_in_memory().unwrap();
    let graph = store.graph_create("urn:example:memory").unwrap();
    let request = input();
    let receipt = commit(&mut store, graph, &request, Authority::Local, TS).unwrap();
    assert!(read(&store, 0, &request.owner).unwrap().is_none());
    assert!(read(&store, graph, &request.owner).unwrap().is_some());
    let mut corrupted = receipt;
    corrupted.file.sha256 = "0".repeat(64);
    assert!(revision(&store, graph, &corrupted).is_err());
}

#[cfg(feature = "shacl")]
#[test]
fn governed_refusal_retains_event_and_leaves_no_memory_head() {
    let mut store = Store::open_in_memory().unwrap();
    let shape = format!(
        r#"
        @prefix sh: <http://www.w3.org/ns/shacl#> .
        <urn:example:shape> a sh:NodeShape ;
          sh:targetClass <{}Artifact> ;
          sh:property [ sh:path <urn:example:required> ; sh:minCount 1 ] .
    "#,
        store.base_ns
    );
    store.load_shapes("fixture", &shape, TS).unwrap();
    store.shacl_config_mut().validate_on_write = true;
    assert!(commit(&mut store, 0, &input(), Authority::Local, TS).is_err());
    assert!(read(&store, 0, &input().owner).unwrap().is_none());
    assert!(
        store
            .refusals_by_gate()
            .unwrap()
            .iter()
            .any(|(gate, n)| gate == "shacl" && *n > 0)
    );
    assert!(store.current_facts().unwrap().is_empty());
}

#[test]
fn credential_evidence_is_distinct_from_the_declared_actor() {
    let mut store = Store::open_in_memory().unwrap();
    let principal = named();
    let identity = crate::transaction_auth::Identity {
        principal: principal.iri.clone(),
        credential_id: Some(principal.credential_id.clone()),
        auth_class: "named_bearer".into(),
        attestation: Default::default(),
    };
    crate::transaction_auth::with_identity(Some(identity.clone()), || {
        commit(&mut store, 0, &input(), Authority::Named(&principal), TS).unwrap();
    });
    let tx = store.list_transactions().unwrap().last().unwrap().id;
    assert_eq!(store.transaction_auth(tx).unwrap(), Some(identity));
}

#[test]
fn public_repository_selector_excludes_memory_even_on_public_subjects() {
    let mut store = Store::open_in_memory().unwrap();
    commit(&mut store, 0, &input(), Authority::Local, TS).unwrap();
    let public = "https://quipu.dev/knowledge/PublicFixture";
    let facts = [
        datum(
            &store,
            public,
            "http://www.w3.org/2000/01/rdf-schema#label",
            Value::Str("PUBLIC_CONTROL".into()),
            TS,
            Op::Assert,
        )
        .unwrap(),
        datum(
            &store,
            public,
            "https://quipu.dev/memory#sourceText",
            Value::Str("PRIVATE_MEMORY_SENTINEL".into()),
            TS,
            Op::Assert,
        )
        .unwrap(),
        datum(
            &store,
            public,
            "http://www.w3.org/2000/01/rdf-schema#seeAlso",
            Value::Ref(store.intern("urn:quipu:memory:revision:sentinel").unwrap()),
            TS,
            Op::Assert,
        )
        .unwrap(),
    ];
    store.transact(&facts, TS, None, None).unwrap();
    let query = include_str!("../../queries/repository-share-quipu.rq");
    let (bytes, count) =
        crate::export_rdf_construct(&store, oxrdfio::RdfFormat::NTriples, query).unwrap();
    let output = String::from_utf8(bytes).unwrap();
    assert!(count > 0 && output.contains("PUBLIC_CONTROL"));
    assert!(!output.contains("PRIVATE_MEMORY_SENTINEL"));
    assert!(!output.contains("urn:quipu:memory:"));
}
