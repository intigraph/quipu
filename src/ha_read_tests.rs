use super::*;
use crate::asof_share::{self, Cut, Request};
use crate::read_authorization::ReadGrant;
use crate::replicate_source::{Frame, Session, StreamRequest};
use crate::signing::SigningIdentity;
use crate::store::Datum;
use crate::types::{Op, Value};
use ring::signature::Ed25519KeyPair;
use serde_json::json;
use sha2::{Digest as _, Sha256};
use std::cell::Cell;
use std::sync::Arc;

const G: &str = "urn:ha:graph";
const OWNER: &str = "urn:ha:owner";
const WHO: &str = "urn:ha:reader";
const T: &str = "2026-10-26T00:00:00Z";
thread_local! { static CLOCK: Cell<u64> = const { Cell::new(1000) }; }
fn clock() -> Result<u64> {
    Ok(CLOCK.with(Cell::get))
}
fn tick(ms: u64) {
    CLOCK.with(|c| c.set(ms));
}

fn signing() -> Arc<SigningIdentity> {
    let bytes = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
    Arc::new(SigningIdentity::new(
        Ed25519KeyPair::from_pkcs8(bytes.as_ref()).unwrap(),
        "quipu",
    ))
}
fn write(s: &mut Store, value: Value) {
    let g = s.graph_create(G).unwrap();
    let d = Datum {
        entity: s.intern("urn:ha:item").unwrap(),
        attribute: s.intern("urn:ha:p").unwrap(),
        value,
        valid_from: T.into(),
        valid_to: None,
        op: Op::Assert,
    };
    s.transact_to_graph(&[d], T, None, None, g).unwrap();
}
fn source() -> Store {
    let mut s = Store::open_in_memory().unwrap();
    write(&mut s, Value::Str("one".into()));
    write(&mut s, Value::Str("two".into()));
    write(&mut s, Value::Bytes(vec![0, 1, 255]));
    let g = s.committed_graph_id(G).unwrap().unwrap();
    let d = Datum {
        entity: s.intern("urn:ha:item").unwrap(),
        attribute: s.intern("urn:ha:window").unwrap(),
        value: Value::Str("finite".into()),
        valid_from: T.into(),
        valid_to: Some("2027-01-01T00:00:00Z".into()),
        op: Op::Assert,
    };
    s.transact_to_graph(&[d], T, None, None, g).unwrap();
    s
}
fn cut(src: &Store, dir: &std::path::Path) -> Cut {
    let mut cut = asof_share::cut(
        src,
        &Request {
            graphs: vec![G.into()],
            context_graphs: vec![],
            tx: None,
            expected_store_id: src.store_id().unwrap(),
        },
        dir,
        &asof_share::Limits {
            disk_floor_bytes: 0,
            part_max_len: 800,
            ..Default::default()
        },
    )
    .unwrap();
    // Synthetic trusted-source declaration for unit controls, independent of
    // whether a local sabotage build is dirty. Final validation builds clean.
    cut.manifest.producer.git_dirty = false;
    cut.manifest_sha256 = crate::share::sha256(&asof_share::manifest_bytes(&cut.manifest).unwrap());
    cut
}
fn frames(cut: &Cut) -> Vec<Frame> {
    let mut out = vec![Frame {
        event: "quipu.asof.begin",
        data: json!({
        "manifest":cut.manifest,"parts":cut.parts.parts,"records":cut.parts.records,
        "digest":cut.parts.digest})
        .to_string(),
    }];
    out.extend(
        std::fs::read_to_string(&cut.spool)
            .unwrap()
            .lines()
            .map(|data| Frame {
                event: "quipu.asof.part",
                data: data.to_owned(),
            }),
    );
    out.push(Frame {
        event: "quipu.asof.commit",
        data: json!({
        "source_tx":cut.manifest.watermark_tx,"digest":cut.parts.digest})
        .to_string(),
    });
    out
}
fn config(cut: &Cut, key: &SigningIdentity) -> Config {
    Config {
        enabled: true,
        manifest: cut.manifest.clone(),
        stream_public_key: key.public_key_hex(),
        max_age_ms: 100,
        max_lag_txs: 0,
    }
}
fn profile(store: Store, c: Config, cut: &Cut) -> Profile {
    tick(1000);
    let mut p = Profile::open(store, c).unwrap();
    p.clock = clock;
    p.last_clock = 1000;
    for frame in frames(cut) {
        p.apply_frame(frame.event, &frame.data).unwrap();
    }
    assert!(p.complete);
    p
}
fn session(src: &Store, key: Arc<SigningIdentity>, p: &mut Profile) -> Session {
    let since = p
        .applier
        .coverage()
        .unwrap_or(p.applier.progress().unwrap().source_tx);
    let (s, frame) = Session::open(
        src,
        &StreamRequest {
            graphs: vec![G.into()],
            context_graphs: vec![],
            since,
            expected_store_id: src.store_id().unwrap(),
        },
        "private-ha-session".into(),
        key,
    )
    .unwrap();
    assert!(matches!(
        p.apply_frame(frame.event, &frame.data).unwrap(),
        Outcome::SessionOpened { .. }
    ));
    s
}
fn fresh(src: &Store, s: &mut Session, p: &mut Profile) {
    let nonce = p.challenge().unwrap();
    let (frames, answered) = s.poll(src, 100, Some(nonce)).unwrap();
    assert!(answered);
    for f in frames {
        p.apply_frame(f.event, &f.data).unwrap();
    }
}
fn identity() -> ReadIdentity {
    let token = "synthetic-private-ha-credential";
    let doc = json!({"version":1,"credentials":[{"credential_id":"fixture","principal":WHO,
        "audience":"quipu","token_sha256":format!("{:x}",Sha256::digest(token.as_bytes()))}]});
    let registry = crate::crew_credentials::CredentialRegistry::parse(
        &serde_json::to_vec(&doc).unwrap(),
        "quipu",
    )
    .unwrap();
    ReadIdentity::authenticate_named(&registry, Some(&format!("Bearer {token}"))).unwrap()
}
fn policy() -> OwnerReadPolicy {
    let mut p = OwnerReadPolicy::open(":memory:", OWNER).unwrap();
    p.apply(
        &ReadGrant {
            version: 1,
            id: "urn:ha:grant".into(),
            principal: WHO.into(),
            graphs: [G.into()].into(),
            operations: [ReadOperation::Query, ReadOperation::Replicate].into(),
            valid_from: 1,
            valid_until: 100,
            max_offline_seconds: 30,
            issuer: OWNER.into(),
            revision: 1,
            revoked: false,
        },
        "private HA fixture",
    )
    .unwrap();
    p
}
const QUERY: &str = "SELECT ?s WHERE { ?s <urn:ha:p> ?v } ORDER BY ?s";
fn query(p: &mut Profile, policy: &OwnerReadPolicy, min_tx: i64) -> Result<Response> {
    p.query(policy, &identity(), QUERY, min_tx, 2)
}

#[test]
fn complete_cut_tail_and_signed_freshness_are_required_for_data_and_count() {
    let dir = tempfile::tempdir().unwrap();
    let mut src = source();
    let key = signing();
    let c = cut(&src, dir.path());
    let mut p = profile(Store::open_in_memory().unwrap(), config(&c, &key), &c);
    let auth = policy();
    assert!(
        query(&mut p, &auth, 0).is_err(),
        "cut/ready alone is not freshness"
    );
    let mut s = session(&src, key, &mut p);
    fresh(&src, &mut s, &mut p);
    let first = query(&mut p, &auth, src.latest_tx_id().unwrap()).unwrap();
    println!(
        "qualified cut: applied_tx={} lag={} age_bound_ms={} body={}",
        first.applied_tx,
        first.verified_lag_txs,
        first.staleness_bound_ms,
        String::from_utf8_lossy(&first.body)
    );
    assert_eq!(first.verified_lag_txs, 0);
    assert_eq!(first.applied_tx, src.latest_tx_id().unwrap());
    assert_eq!(
        crate::applier::live_digest(&p.store),
        crate::applier::live_digest(&src)
    );
    write(&mut src, Value::Str("three".into()));
    fresh(&src, &mut s, &mut p);
    assert_eq!(
        crate::applier::live_digest(&p.store),
        crate::applier::live_digest(&src)
    );
    let response = p
        .query(
            &auth,
            &identity(),
            "SELECT (COUNT(?v) AS ?n) WHERE { ?s <urn:ha:p> ?v }",
            src.latest_tx_id().unwrap(),
            2,
        )
        .unwrap();
    assert!(String::from_utf8(response.body).unwrap().contains("4"));
    let future = query(&mut p, &auth, src.latest_tx_id().unwrap() + 1);
    println!("future minTx result: {future:?}");
    assert!(future.is_err());
}

#[test]
fn exact_age_boundary_drain_transport_and_clock_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    let src = source();
    let key = signing();
    let c = cut(&src, dir.path());
    let mut p = profile(Store::open_in_memory().unwrap(), config(&c, &key), &c);
    let auth = policy();
    let mut s = session(&src, key, &mut p);
    fresh(&src, &mut s, &mut p);
    tick(1099);
    assert!(query(&mut p, &auth, 0).is_ok());
    tick(1100);
    assert!(query(&mut p, &auth, 0).is_err());
    fresh(&src, &mut s, &mut p);
    assert!(query(&mut p, &auth, 0).is_ok());
    p.set_draining(true);
    assert!(query(&mut p, &auth, 0).is_err());
    p.set_draining(false);
    assert!(query(&mut p, &auth, 0).is_err());
    fresh(&src, &mut s, &mut p);
    assert!(query(&mut p, &auth, 0).is_ok());
    tick(1000);
    assert!(
        query(&mut p, &auth, 0).is_err(),
        "regression cannot become age0"
    );
    tick(1200);
    assert!(
        query(&mut p, &auth, 0).is_err(),
        "old proof was invalidated"
    );
    fresh(&src, &mut s, &mut p);
    p.end_session();
    assert!(query(&mut p, &auth, 0).is_err());
}

#[test]
fn disabled_missing_pins_provider_and_migration_refusals_create_no_profile_state() {
    let dir = tempfile::tempdir().unwrap();
    let src = source();
    let key = signing();
    let c = cut(&src, dir.path());
    for fault in 0..9 {
        let path = dir.path().join(format!("disabled-{fault}.db"));
        let store = Store::open(path.to_str().unwrap()).unwrap();
        let mut cfg = config(&c, &key);
        match fault {
            0 => cfg.enabled = false,
            1 => cfg.max_age_ms = 0,
            2 => cfg.max_age_ms = u64::MAX,
            3 => cfg.max_lag_txs = 1,
            4 => cfg.manifest.producer.version = "0.17.0-unqualified".into(),
            5 => cfg.manifest.scope_digest = "unknown".into(),
            6 => cfg.manifest.completeness.graphs.get_mut(G).unwrap().frozen = Some(json!({})),
            7 => cfg.manifest.producer.git_dirty = true,
            _ => cfg.manifest.producer.git_sha = "unqualified-revision".into(),
        }
        assert!(Profile::open(store, cfg).is_err());
        let store = Store::open_read_only(path.to_str().unwrap()).unwrap();
        let n:i64=store.conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name IN ('ha_read_cut','replica_progress')",[],|r|r.get(0)).unwrap();
        assert_eq!(n, 0, "refusal must precede profile/applier DDL");
    }
}

#[test]
fn restart_requires_exact_marker_new_session_and_new_freshness_proof() {
    let dir = tempfile::tempdir().unwrap();
    let src = source();
    let key = signing();
    let c = cut(&src, dir.path());
    let cfg = config(&c, &key);
    let path = dir.path().join("replica.db");
    let mut p = profile(
        Store::open(path.to_str().unwrap()).unwrap(),
        cfg.clone(),
        &c,
    );
    let auth = policy();
    let mut s = session(&src, key.clone(), &mut p);
    fresh(&src, &mut s, &mut p);
    assert!(query(&mut p, &auth, 0).is_ok());
    drop(p);
    let mut p = Profile::open(Store::open(path.to_str().unwrap()).unwrap(), cfg.clone()).unwrap();
    p.clock = clock;
    p.last_clock = 1000;
    assert!(query(&mut p, &auth, 0).is_err());
    let mut s = session(&src, key, &mut p);
    fresh(&src, &mut s, &mut p);
    assert!(query(&mut p, &auth, 0).is_ok());
    p.store.conn.execute("DELETE FROM ha_read_cut", []).unwrap();
    assert!(query(&mut p, &auth, 0).is_err());
}

#[test]
fn wrong_cut_epoch_or_source_and_missing_names_never_splice_or_authorize() {
    let dir = tempfile::tempdir().unwrap();
    let src = source();
    let key = signing();
    let c = cut(&src, dir.path());
    let mut p = Profile::open(Store::open_in_memory().unwrap(), config(&c, &key)).unwrap();
    p.clock = clock;
    p.last_clock = 1000;
    let mut f = frames(&c);
    let mut v: serde_json::Value = serde_json::from_str(&f[0].data).unwrap();
    v["manifest"]["source"]["log_epoch"] = json!("different");
    f[0].data = v.to_string();
    assert!(p.apply_frame(f[0].event, &f[0].data).is_err());
    assert!(p.applier.progress().is_none());
    for f in frames(&c) {
        p.apply_frame(f.event, &f.data).unwrap();
    }
    let mut s = session(&src, key, &mut p);
    fresh(&src, &mut s, &mut p);
    let empty = OwnerReadPolicy::open(":memory:", OWNER).unwrap();
    assert!(query(&mut p, &empty, 0).is_err());
    assert!(
        p.query(
            &policy(),
            &identity(),
            "ASK { GRAPH <urn:ha:denied> { ?s ?p ?o } }",
            0,
            2
        )
        .is_err()
    );
    p.store
        .conn
        .execute("UPDATE replica_coverage SET log_epoch='other'", [])
        .unwrap();
    assert!(query(&mut p, &policy(), 0).is_err());
}

#[test]
fn eligibility_and_authorized_copy_are_the_same_sqlite_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let src = source();
    let key = signing();
    let c = cut(&src, dir.path());
    let path = dir.path().join("snapshot.db");
    let mut p = profile(
        Store::open(path.to_str().unwrap()).unwrap(),
        config(&c, &key),
        &c,
    );
    let mut s = session(&src, key, &mut p);
    fresh(&src, &mut s, &mut p);
    let mut writer = Store::open(path.to_str().unwrap()).unwrap();
    p.after_pin = Some(Box::new(move || {
        write(&mut writer, Value::Str("concurrent".into()));
    }));
    let r = p
        .query(
            &policy(),
            &identity(),
            "SELECT (COUNT(?v) AS ?n) WHERE { ?s <urn:ha:p> ?v }",
            0,
            2,
        )
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
    assert_eq!(body["results"]["bindings"][0]["n"]["value"], "3");
    let actual = crate::sparql::query(
        &p.store,
        "SELECT (COUNT(?v) AS ?n) WHERE { GRAPH <urn:ha:graph> { ?s <urn:ha:p> ?v } }",
    )
    .unwrap();
    assert_eq!(
        actual.rows()[0].get("n"),
        Some(&Value::Int(4)),
        "the external writer really committed"
    );
}

#[test]
fn epoch_end_is_persisted_and_cannot_become_ready_on_restart() {
    let dir = tempfile::tempdir().unwrap();
    let src = source();
    let key = signing();
    let c = cut(&src, dir.path());
    let cfg = config(&c, &key);
    let path = dir.path().join("ended.db");
    let mut p = profile(
        Store::open(path.to_str().unwrap()).unwrap(),
        cfg.clone(),
        &c,
    );
    let mut s = session(&src, key, &mut p);
    fresh(&src, &mut s, &mut p);
    src.mint_log_epoch("private epoch end").unwrap();
    let (frames, _) = s.poll(&src, 100, None).unwrap();
    assert!(!frames.is_empty());
    for f in frames {
        p.apply_frame(f.event, &f.data).unwrap();
    }
    assert!(query(&mut p, &policy(), 0).is_err());
    drop(p);
    let mut p = Profile::open(Store::open(path.to_str().unwrap()).unwrap(), cfg).unwrap();
    p.clock = clock;
    p.last_clock = 1000;
    assert!(query(&mut p, &policy(), 0).is_err());
}

#[test]
fn policy_revocation_and_expiry_are_not_overridden_by_replica_health() {
    let dir = tempfile::tempdir().unwrap();
    let src = source();
    let key = signing();
    let c = cut(&src, dir.path());
    let mut p = profile(Store::open_in_memory().unwrap(), config(&c, &key), &c);
    let mut s = session(&src, key, &mut p);
    fresh(&src, &mut s, &mut p);
    let mut auth = policy();
    assert!(query(&mut p, &auth, 0).is_ok());
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
            revoked: true,
        },
        "private revocation",
    )
    .unwrap();
    assert!(query(&mut p, &auth, 0).is_err());
    assert!(p.query(&policy(), &identity(), QUERY, 0, 100).is_err());
    assert!(
        p.query(&policy(), &identity(), QUERY, 0, 99).is_err(),
        "ceil publication time cannot extend an expiring grant"
    );
}

#[path = "ha_read_http_tests.rs"]
mod http;

#[path = "ha_read_provider_tests.rs"]
mod providers;
