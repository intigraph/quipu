use super::*;

#[test]
fn unqualified_custom_providers_are_refused_even_in_unevaluated_branches() {
    let dir = tempfile::tempdir().unwrap();
    let src = source();
    let key = signing();
    let c = cut(&src, dir.path());
    let mut p = profile(Store::open_in_memory().unwrap(), config(&c, &key), &c);
    let mut s = session(&src, key, &mut p);
    fresh(&src, &mut s, &mut p);
    for text in [
        "SELECT ?x WHERE { VALUES ?x { 1 } FILTER(false && <urn:custom:model>(?x)) }",
        "ASK { FILTER NOT EXISTS { FILTER(<urn:custom:index>(1)) } }",
        "SELECT (<urn:custom:aggregate>(?x) AS ?n) WHERE { VALUES ?x { 1 } }",
        "ASK { SERVICE <urn:other:service> { ?s ?p ?o } }",
    ] {
        assert!(
            p.query(&policy(), &identity(), text, 0, 2).is_err(),
            "{text}"
        );
    }
    assert!(
        p.query(
            &policy(),
            &identity(),
            "SELECT (COUNT(*) AS ?n) WHERE { ?s ?p ?o }",
            0,
            2
        )
        .is_ok()
    );
}

#[test]
fn query_only_principal_cannot_probe_replication_metadata_with_min_tx() {
    let dir = tempfile::tempdir().unwrap();
    let src = source();
    let key = signing();
    let c = cut(&src, dir.path());
    let mut p = profile(Store::open_in_memory().unwrap(), config(&c, &key), &c);
    let mut s = session(&src, key, &mut p);
    fresh(&src, &mut s, &mut p);
    let mut auth = policy();
    auth.apply(
        &ReadGrant {
            version: 1,
            id: "urn:ha:grant".into(),
            principal: WHO.into(),
            graphs: [G.into()].into(),
            operations: [ReadOperation::Query].into(),
            valid_from: 1,
            valid_until: 100,
            max_offline_seconds: 30,
            issuer: OWNER.into(),
            revision: 2,
            revoked: false,
        },
        "query-only private principal",
    )
    .unwrap();
    for tx in [0, i64::MAX] {
        assert!(matches!(
            query(&mut p, &auth, tx),
            Err(Error::PolicyDenied(_))
        ));
    }
}

#[test]
fn refused_stream_remains_halted_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let src = source();
    let key = signing();
    let c = cut(&src, dir.path());
    let cfg = config(&c, &key);
    let path = dir.path().join("refused.db");
    let mut p = profile(
        Store::open(path.to_str().unwrap()).unwrap(),
        cfg.clone(),
        &c,
    );
    let mut s = session(&src, key.clone(), &mut p);
    fresh(&src, &mut s, &mut p);
    assert!(query(&mut p, &policy(), 0).is_ok());
    assert!(matches!(
        p.apply_frame("quipu.replicate.session", "{}").unwrap(),
        Outcome::Refused { .. }
    ));
    assert!(query(&mut p, &policy(), 0).is_err());
    drop(p);
    let mut p = Profile::open(Store::open(path.to_str().unwrap()).unwrap(), cfg).unwrap();
    p.clock = clock;
    p.last_clock = 1000;
    assert!(p.halted);
    // Even attempted session re-entry cannot erase the persisted local stop.
    assert!(p.apply_frame("quipu.replicate.session", "{}").is_err());
    assert!(query(&mut p, &policy(), 0).is_err());
}

#[test]
fn query_copy_time_cannot_publish_an_expired_freshness_proof() {
    let dir = tempfile::tempdir().unwrap();
    let src = source();
    let key = signing();
    let c = cut(&src, dir.path());
    let mut p = profile(Store::open_in_memory().unwrap(), config(&c, &key), &c);
    let mut s = session(&src, key, &mut p);
    fresh(&src, &mut s, &mut p);
    assert!(query(&mut p, &policy(), 0).is_ok());
    p.after_pin = Some(Box::new(|| tick(1100)));
    let expired = query(&mut p, &policy(), 0);
    assert!(
        matches!(expired, Err(Error::CannotVerify(ref message)) if message.contains("query outlived freshness"))
    );
    fresh(&src, &mut s, &mut p);
    assert!(query(&mut p, &policy(), 0).is_ok());
}
