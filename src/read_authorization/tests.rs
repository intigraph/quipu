use super::*;

const P: &str = "urn:principal:p";
const Q: &str = "urn:principal:q";
const OWNER: &str = "urn:principal:owner";
const A: &str = "urn:graph:a";
const B: &str = "urn:graph:b";
const C: &str = "urn:graph:c";

fn identity(principal: &str) -> ReadIdentity {
    let token = format!("fixture-{principal}");
    let registry = serde_json::json!({"version":1,"credentials":[{
        "credential_id":"fixture", "principal":principal, "audience":"quipu",
        "token_sha256": format!("{:x}", Sha256::digest(token.as_bytes()))
    }]});
    let registry = crate::crew_credentials::CredentialRegistry::parse(
        &serde_json::to_vec(&registry).unwrap(),
        "quipu",
    )
    .unwrap();
    ReadIdentity::authenticate_named(&registry, Some(&format!("Bearer {token}"))).unwrap()
}

fn grant(principal: &str, graphs: &[&str], operation: ReadOperation) -> ReadGrant {
    ReadGrant {
        version: 1,
        id: format!("urn:grant:{principal}:{operation:?}"),
        principal: principal.into(),
        graphs: graphs.iter().map(|s| (*s).into()).collect(),
        operations: [operation].into(),
        valid_from: 1,
        valid_until: 100,
        max_offline_seconds: 300,
        issuer: OWNER.into(),
        revision: 1,
        revoked: false,
    }
}

fn policy() -> OwnerReadPolicy {
    let mut policy = OwnerReadPolicy::open(":memory:", OWNER).unwrap();
    for operation in [
        ReadOperation::Query,
        ReadOperation::Metadata,
        ReadOperation::Export,
    ] {
        policy
            .apply(&grant(P, &[A, C], operation), "private fixture")
            .unwrap();
        policy
            .apply(&grant(Q, &[B], operation), "private fixture")
            .unwrap();
    }
    policy
}

fn source(hidden: &str) -> Store {
    let mut store = Store::open_in_memory().unwrap();
    // Different hidden terms and registry entries precede the same allowed data.
    let b = store.graph_create(B).unwrap();
    let secret = format!("<urn:hidden:{hidden}> <urn:predicate:hidden> \"{hidden}\" .\n");
    crate::rdf::ingest_assigned_rdf(
        &mut store,
        secret.as_bytes(),
        oxrdfio::RdfFormat::NTriples,
        None,
        "2026-01-01T00:00:00Z",
        None,
        Some(hidden),
        b,
    )
    .unwrap();
    store
        .graph_create(&format!("urn:hidden:graph:{hidden}"))
        .unwrap();
    for (graph, text) in [
        (
            A,
            "<urn:e> <urn:p> <urn:value> .\n<urn:value> <urn:p> <urn:end> .\n",
        ),
        (C, "<urn:context> <urn:p> \"context\" .\n"),
    ] {
        let g = store.graph_create(graph).unwrap();
        crate::rdf::ingest_assigned_rdf(
            &mut store,
            text.as_bytes(),
            oxrdfio::RdfFormat::NTriples,
            None,
            "2026-01-01T00:00:00Z",
            None,
            None,
            g,
        )
        .unwrap();
    }
    store
}

fn view(policy: &OwnerReadPolicy, source: &Store, who: &str, operation: ReadOperation) -> ReadView {
    policy
        .snapshot()
        .unwrap()
        .authorize(&identity(who), operation, None, 2)
        .unwrap()
        .open(source, 2)
        .unwrap()
}

#[test]
fn two_real_stores_have_identical_allowed_query_export_and_metadata() {
    let policy = policy();
    let left = source("one");
    let right = source("different_denied_data");
    for operation in [
        ReadOperation::Query,
        ReadOperation::Export,
        ReadOperation::Metadata,
    ] {
        let a = view(&policy, &left, P, operation);
        let b = view(&policy, &right, P, operation);
        match operation {
            ReadOperation::Query => {
                for model in [false, true] {
                    for query in [
                        "SELECT ?s ?p ?o WHERE { ?s ?p ?o } ORDER BY ?s ?p ?o",
                        "SELECT ?g ?s WHERE { GRAPH ?g { ?s ?p ?o } } ORDER BY ?g ?s",
                        "ASK { <urn:e> <urn:p>+ <urn:end> }",
                        "ASK { <urn:e> <urn:p> ?v FILTER NOT EXISTS { ?v <urn:absent> ?hidden } }",
                        "SELECT ?s WHERE { { SELECT ?s WHERE { ?s <urn:p> ?o } } } ORDER BY ?s",
                        "ASK FROM <urn:graph:a> { <urn:e> <urn:p> <urn:value> }",
                    ] {
                        let first = a.query(query, model, 2).unwrap();
                        let second = b.query(query, model, 2).unwrap();
                        assert_eq!(first, second, "{query}");
                    }
                    let yes = a
                        .query("ASK { <urn:e> <urn:p> <urn:value> }", model, 2)
                        .unwrap();
                    assert!(String::from_utf8(yes).unwrap().contains("true"));
                }
            }
            ReadOperation::Export => {
                assert_eq!(a.export(2).unwrap(), b.export(2).unwrap());
                assert_eq!(a.export(2).unwrap().len(), 2);
            }
            ReadOperation::Metadata => {
                assert_eq!(a.metadata(2).unwrap(), b.metadata(2).unwrap());
                assert_eq!(a.metadata(2).unwrap()[A], 2);
            }
            ReadOperation::Replicate => unreachable!(),
        }
    }
    let q = view(&policy, &left, Q, ReadOperation::Query);
    assert!(
        String::from_utf8(q.query("ASK { ?s ?p ?o }", false, 2).unwrap())
            .unwrap()
            .contains("true")
    );
}

#[test]
fn exact_datasets_and_nested_graph_scopes_refuse_uniformly() {
    let policy = policy();
    let source = source("secret");
    let snapshot = policy.snapshot().unwrap();
    for graphs in [
        vec![B],
        vec![crate::schema::ROOT_GRAPH_IRI],
        vec![A, B],
        vec!["urn:missing"],
        vec![],
    ] {
        let selected = graphs.iter().map(|s| (*s).into()).collect();
        assert_eq!(
            snapshot
                .authorize(&identity(P), ReadOperation::Query, Some(&selected), 2)
                .err()
                .unwrap()
                .to_string(),
            denied().to_string()
        );
    }
    let view = view(&policy, &source, P, ReadOperation::Query);
    for query in [
        "ASK { GRAPH <urn:graph:b> { ?s ?p ?o } }",
        "ASK { GRAPH <urn:missing> { ?s ?p ?o } }",
        "ASK FROM <urn:graph:a> FROM <urn:graph:b> { ?s ?p ?o }",
        "ASK FROM NAMED <urn:graph:b> { GRAPH ?g { ?s ?p ?o } }",
        "ASK { FILTER NOT EXISTS { GRAPH <urn:graph:b> { ?s ?p ?o } } }",
        "SELECT * WHERE { { SELECT * WHERE { GRAPH <urn:graph:b> { ?s ?p ?o } } } }",
        "ASK { SERVICE <https://example.invalid/> { ?s ?p ?o } }",
        "ASK { VALUES ?s {} GRAPH <urn:graph:b> { ?s ?p ?o } }",
    ] {
        assert_eq!(
            view.query(query, false, 2).unwrap_err().to_string(),
            denied().to_string(),
            "{query}"
        );
    }
    assert!(view.export(2).is_err());
    assert!(view.metadata(2).is_err());
}

#[test]
fn revocation_is_atomic_persistent_and_old_views_do_not_revalidate() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let path = file.path().to_str().unwrap();
    let mut policy = OwnerReadPolicy::open(path, OWNER).unwrap();
    let original = grant(P, &[A], ReadOperation::Query);
    let first = policy.apply(&original, "fixture:grant").unwrap();
    let authorized = policy
        .snapshot()
        .unwrap()
        .authorize(&identity(P), ReadOperation::Query, None, 2)
        .unwrap();
    let mut revoked = original.clone();
    revoked.revoked = true;
    revoked.revision = 2;
    let second = policy.apply(&revoked, "fixture:revoke").unwrap();
    assert!(second > first);
    let current = policy.snapshot().unwrap();
    assert!(authorized.recheck(&current, &identity(P), 2).is_err());
    assert!(
        current
            .authorize(&identity(P), ReadOperation::Query, None, 2)
            .is_err()
    );
    assert!(policy.apply(&original, "fixture:stale").is_err());
    drop(policy);
    let reopened = OwnerReadPolicy::open(path, OWNER).unwrap();
    assert!(
        reopened
            .snapshot()
            .unwrap()
            .authorize(&identity(P), ReadOperation::Query, None, 2)
            .is_err()
    );
    assert!(OwnerReadPolicy::open(path, "urn:principal:impostor").is_err());
}

#[test]
fn invalid_identity_no_grant_expiry_and_operation_intersection_deny() {
    let policy = policy();
    let snapshot = policy.snapshot().unwrap();
    assert!(
        snapshot
            .authorize(
                &identity("urn:principal:unbound"),
                ReadOperation::Query,
                None,
                2
            )
            .is_err()
    );
    assert!(
        snapshot
            .authorize(&identity(P), ReadOperation::Replicate, None, 2)
            .is_err()
    );
    assert!(
        snapshot
            .authorize(&identity(P), ReadOperation::Query, None, 100)
            .is_err()
    );
    let registry = crate::crew_credentials::CredentialRegistry::default();
    assert!(ReadIdentity::authenticate_named(&registry, Some("Bearer invalid")).is_err());
    for class in ["legacy_shared_bearer", "caller_declared", "write_only"] {
        let declared = crate::transaction_auth::Identity {
            principal: P.into(),
            credential_id: Some("copied".into()),
            auth_class: class.into(),
            attestation: Default::default(),
        };
        assert!(ReadIdentity::from_verified_request(&declared, 100).is_err());
    }
}

#[test]
fn failed_revision_rolls_back_generation_and_provisional_dictionary_entries() {
    let mut policy = policy();
    let before = policy.snapshot().unwrap();
    policy.store.conn.execute_batch("CREATE TEMP TRIGGER refuse_read_policy BEFORE INSERT ON facts BEGIN SELECT RAISE(ABORT, 'private fixture failure'); END;").unwrap();
    let mut changed = grant(P, &[A], ReadOperation::Query);
    changed.revision = 2;
    changed.revoked = true;
    assert!(policy.apply(&changed, "fixture:failure").is_err());
    let after = policy.snapshot().unwrap();
    assert_eq!(before.generation, after.generation);
    assert_eq!(before.digest, after.digest);
    assert!(
        after
            .authorize(&identity(P), ReadOperation::Query, None, 2)
            .is_ok()
    );
    policy
        .store
        .conn
        .execute_batch("DROP TRIGGER refuse_read_policy")
        .unwrap();
    policy.apply(&changed, "fixture:retry").unwrap();
    assert!(
        policy
            .snapshot()
            .unwrap()
            .authorize(&identity(P), ReadOperation::Query, None, 2)
            .is_err()
    );
}

#[test]
fn malformed_authority_and_policy_missing_are_not_fallbacks() {
    let mut policy = policy();
    let graph = policy.store.committed_graph_id(CONTROL).unwrap().unwrap();
    let invalid = crate::store::Datum {
        entity: policy.store.intern("urn:malformed").unwrap(),
        attribute: policy.store.intern(RECORD).unwrap(),
        value: crate::Value::Str("not JSON".into()),
        valid_from: "2026-01-01T00:00:00Z".into(),
        valid_to: None,
        op: crate::types::Op::Assert,
    };
    policy
        .store
        .transact_to_graph(&[invalid], "2026-01-01T00:00:00Z", None, None, graph)
        .unwrap();
    assert!(policy.snapshot().is_err());
    let dir = tempfile::tempdir().unwrap();
    assert!(
        OwnerReadPolicy::open_existing(dir.path().join("absent").to_str().unwrap(), OWNER).is_err()
    );
    assert!(!dir.path().join("absent").exists());
}

#[test]
fn clock_rollback_and_source_epoch_change_invalidate_owned_views() {
    let policy = policy();
    let source = source("hidden");
    let snapshot = policy.snapshot().unwrap();
    let identity = identity(P);
    let view = snapshot
        .authorize(&identity, ReadOperation::Query, None, 5)
        .unwrap()
        .open(&source, 5)
        .unwrap();
    assert!(view.query("ASK {}", false, 4).is_err());
    assert!(view.recheck(&snapshot, &identity, 4).is_err());
    assert!(view.recheck_source(&source).is_ok());
    source
        .mint_log_epoch("private fixture epoch change")
        .unwrap();
    assert!(view.recheck_source(&source).is_err());
}

#[test]
fn private_scoped_view_cost() {
    let policy = policy();
    let source = source("hidden");
    let rss = || {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|text| {
                text.lines()
                    .find(|line| line.starts_with("VmRSS:"))
                    .map(str::to_owned)
            })
    };
    let before = rss();
    let mut timings = Vec::new();
    let mut payload_bytes = 0;
    for _ in 0..30 {
        let started = std::time::Instant::now();
        let view = view(&policy, &source, P, ReadOperation::Query);
        let result = view
            .query("SELECT ?s WHERE { ?s <urn:p> ?v } ORDER BY ?s", true, 2)
            .unwrap();
        payload_bytes = result.len();
        timings.push(started.elapsed().as_micros());
    }
    timings.sort_unstable();
    eprintln!(
        "PRIVATE_COST calls=30 network_requests=0 p95_us={} response_bytes={} rss_before={before:?} rss_after={:?}; includes policy snapshot+authorization+bounded materialization+query; RSS is this test process, not per-view allocation or production evidence",
        timings[28],
        payload_bytes,
        rss()
    );
}

#[test]
fn concurrent_owner_handles_cannot_commit_the_same_revision_twice() {
    use std::sync::{Arc, Barrier};
    let file = tempfile::NamedTempFile::new().unwrap();
    let path = file.path().to_str().unwrap().to_owned();
    let mut original = OwnerReadPolicy::open(&path, OWNER).unwrap();
    original
        .apply(&grant(P, &[A], ReadOperation::Query), "fixture:first")
        .unwrap();
    let before = original.snapshot().unwrap().generation;
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut policy = OwnerReadPolicy::open(&path, OWNER).unwrap();
                let mut revision = grant(P, &[A], ReadOperation::Query);
                revision.revision = 2;
                revision.revoked = true;
                barrier.wait();
                policy.apply(&revision, "fixture:concurrent").is_ok()
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.into_iter().filter(|success| *success).count(), 1);
    let after = original.snapshot().unwrap();
    assert_eq!(after.generation, before + 1);
    assert!(
        after
            .authorize(&identity(P), ReadOperation::Query, None, 2)
            .is_err()
    );
}

#[test]
fn grants_are_operation_specific_and_invalid_issuer_or_version_has_no_effect() {
    let mut policy = policy();
    let before = policy.snapshot().unwrap().generation;
    let mut invalid = grant(P, &[A], ReadOperation::Query);
    invalid.id = "urn:grant:invalid".into();
    invalid.issuer = "urn:principal:tenant".into();
    assert!(policy.apply(&invalid, "fixture:unqualified").is_err());
    invalid.issuer = OWNER.into();
    invalid.version = 2;
    assert!(policy.apply(&invalid, "fixture:future-version").is_err());
    assert_eq!(policy.snapshot().unwrap().generation, before);
    let mut export = grant(P, &[A], ReadOperation::Export);
    export.revision = 2;
    policy.apply(&export, "fixture:narrow-export").unwrap();
    let c = [C.to_owned()].into();
    assert!(
        policy
            .snapshot()
            .unwrap()
            .authorize(&identity(P), ReadOperation::Query, Some(&c), 2)
            .is_ok()
    );
    assert!(
        policy
            .snapshot()
            .unwrap()
            .authorize(&identity(P), ReadOperation::Export, Some(&c), 2)
            .is_err()
    );
}

#[test]
fn an_oversized_value_refuses_before_materializing_a_partial_view() {
    let policy = policy();
    let mut source = source("hidden");
    let g = source.committed_graph_id(A).unwrap().unwrap();
    let datum = crate::store::Datum {
        entity: source.intern("urn:large").unwrap(),
        attribute: source.intern("urn:p").unwrap(),
        value: crate::Value::Str("x".repeat(16 * 1024 * 1024 + 1)),
        valid_from: "2026-01-01T00:00:00Z".into(),
        valid_to: None,
        op: crate::types::Op::Assert,
    };
    source
        .transact_to_graph(&[datum], "2026-01-01T00:00:00Z", None, None, g)
        .unwrap();
    let authorization = policy
        .snapshot()
        .unwrap()
        .authorize(&identity(P), ReadOperation::Query, None, 2)
        .unwrap();
    assert!(authorization.open(&source, 2).is_err());
}
