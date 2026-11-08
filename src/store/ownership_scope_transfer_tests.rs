use super::*;

fn seed(store: &mut Store, name: &str) {
    let nt = format!("<urn:transfer:{name}> <urn:transfer:p> \"value\" .");
    crate::rdf::ingest_rdf(
        store,
        nt.as_bytes(),
        oxrdfio::RdfFormat::NTriples,
        None,
        TS,
        None,
        None,
    )
    .unwrap();
}

fn share_request(tag: &str) -> (tempfile::TempDir, crate::share_import::ShareImportRequest) {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("share");
    let mut sender = Store::open_in_memory().unwrap();
    crate::share_scrub::seed_test_catalogue(&mut sender);
    seed(&mut sender, tag);
    crate::share::share(
        &sender,
        output.to_str().unwrap(),
        &crate::share::ShareOptions {
            no_shapes: true,
            ..Default::default()
        },
    )
    .unwrap();
    let read = |name: &str| std::fs::read_to_string(output.join(name)).unwrap();
    let request = crate::share_import::ShareImportRequest {
        manifest: serde_json::from_str(&read("manifest.json")).unwrap(),
        export_ntriples: read("export.nt"),
        shapes_turtle: read("shapes.ttl"),
        queries_turtle: None,
        alignments_turtle: None,
        query_namespace: None,
        replace_queries: false,
        source: "https://example.org/fixture/share".into(),
        actor: Some("fixture".into()),
        accept_exact: false,
        attestation: None,
        destination: Default::default(),
    };
    (dir, request)
}

#[test]
fn actual_share_import_writes_in_a_permit_and_refuses_a_fresh_share_after_retirement() {
    let mut f = Fixture::configured(crate::share_scrub::seed_test_catalogue);
    let (_source, request) = share_request("positive");
    let empty = snapshot(&f.store);
    let receipt = f
        .store
        .ownership_operation(|s| crate::share_import::import_share(s, &request, TS, None))
        .unwrap();
    assert!(receipt.triples.accepted + receipt.triples.quarantined > 0);
    assert_ne!(snapshot(&f.store), empty);
    let (_other, fresh) = share_request("fresh-negative");
    super::super::super::ownership_journal::advance(&f.journal, &f.active(), &f.retiring())
        .unwrap();
    let before = snapshot(&f.store);
    assert!(
        f.store
            .ownership_operation(|s| crate::share_import::import_share(s, &fresh, TS, None))
            .is_err()
    );
    assert!(crate::share_import::import_share(&mut f.store, &fresh, TS, None).is_err());
    assert_eq!(snapshot(&f.store), before);
}

fn fork_fixture() -> Fixture {
    Fixture::configured(|s| {
        s.fork_create("branch", s.latest_tx_id().unwrap(), TS, None)
            .unwrap();
        let graph = s.fork_graph_for_read("branch").unwrap();
        let datum = Datum {
            entity: s.intern("urn:fork:positive").unwrap(),
            attribute: s.intern("urn:fork:p").unwrap(),
            value: Value::Int(1),
            valid_from: TS.into(),
            valid_to: None,
            op: Op::Assert,
        };
        s.transact_to_graph(&[datum], TS, None, None, graph)
            .unwrap();
    })
}

#[test]
fn actual_fork_promotion_pairs_a_positive_with_an_unmerged_retired_fixture() {
    let mut positive = fork_fixture();
    let before = snapshot(&positive.store);
    let result = positive
        .store
        .ownership_operation(|s| s.fork_promote("branch", TS, None))
        .unwrap();
    assert!(
        matches!(
            result,
            super::super::super::forks::ForkPromotion::Promoted { asserted: 1, .. }
        ),
        "{result:?}"
    );
    assert_ne!(snapshot(&positive.store), before);
    let mut stale = fork_fixture();
    super::super::super::ownership_journal::advance(
        &stale.journal,
        &stale.active(),
        &stale.retiring(),
    )
    .unwrap();
    let before = snapshot(&stale.store);
    assert!(
        stale
            .store
            .ownership_operation(|s| s.fork_promote("branch", TS, None))
            .is_err()
    );
    assert!(stale.store.fork_promote("branch", TS, None).is_err());
    assert_eq!(snapshot(&stale.store), before);
}

#[test]
fn offline_raw_connection_import_preserves_its_real_cold_enrollment_gate() {
    // This writer opens its OWN connection. A permit on another connection
    // cannot authorize it: its existing enrolled-startup refusal must hold.
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.db");
    let second = dir.path().join("second.db");
    let destination = dir.path().join("destination.db");
    for (path, tag) in [(&source, "first"), (&second, "second")] {
        let mut store = Store::open(path.to_str().unwrap()).unwrap();
        seed(&mut store, tag);
    }
    let positive = super::super::super::import::import_graph(
        &destination,
        &source,
        crate::schema::ROOT_GRAPH_IRI,
    )
    .unwrap();
    assert!(positive.facts > 0);
    let held = Store::open(destination.to_str().unwrap()).unwrap();
    let raw = rusqlite::Connection::open(&destination).unwrap();
    raw.execute_batch("CREATE TABLE ownership_handoff_enrollment(payload TEXT)")
        .unwrap();
    let before = snapshot(&held);
    let err = super::super::super::import::import_graph(
        &destination,
        &second,
        crate::schema::ROOT_GRAPH_IRI,
    )
    .unwrap_err();
    assert!(err.to_string().contains("enrolled startup"), "{err}");
    assert_eq!(
        snapshot(&held),
        before,
        "no raw import, epoch or schema mutation"
    );
}

#[cfg(unix)]
#[test]
fn symlink_hardlink_and_unprotected_journal_or_latch_refuse_before_sql() {
    for change in [
        "journal-symlink",
        "journal-hardlink",
        "journal-permissions",
        "lock-symlink",
    ] {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let mut f = Fixture::new();
        let before = snapshot(&f.store);
        match change {
            "journal-symlink" => {
                let target = f.journal.with_extension("saved");
                std::fs::rename(&f.journal, &target).unwrap();
                symlink(&target, &f.journal).unwrap();
            }
            "journal-hardlink" => {
                std::fs::hard_link(&f.journal, f.journal.with_extension("copy")).unwrap();
            }
            "journal-permissions" => {
                std::fs::set_permissions(&f.journal, std::fs::Permissions::from_mode(0o644))
                    .unwrap();
            }
            _ => {
                let lock = ownership_latch::sibling(&f.journal, ".lock").unwrap();
                let target = lock.with_extension("saved");
                std::fs::rename(&lock, &target).unwrap();
                symlink(&target, &lock).unwrap();
            }
        }
        assert!(
            f.store
                .ownership_transaction(|s| write(s, "bad-file"))
                .is_err(),
            "{change}"
        );
        assert_eq!(snapshot(&f.store), before);
    }
}
