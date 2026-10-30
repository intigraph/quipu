//! Signed cut admission through the existing transactional shared applier.
use super::*;
use crate::applier::{BootstrapTrust, Source};
use crate::asof_share::{self, Manifest};
use crate::signing::SigningIdentity;

fn signed_cut() -> (
    Store,
    Json,
    Vec<Json>,
    BootstrapTrust,
    String,
    SigningIdentity,
) {
    let src = source();
    let temp = tempfile::tempdir().unwrap();
    let request = asof_share::Request {
        graphs: vec!["ROOT".into(), BOARD.into()],
        context_graphs: vec![],
        tx: None,
        expected_store_id: src.store_id().unwrap(),
    };
    let cut = asof_share::cut(
        &src,
        &request,
        temp.path(),
        &asof_share::Limits {
            disk_floor_bytes: 0,
            ..Default::default()
        },
    )
    .unwrap();
    let records = std::fs::read_to_string(&cut.spool)
        .unwrap()
        .lines()
        .flat_map(|line| {
            serde_json::from_str::<Json>(line).unwrap()["records"]
                .as_array()
                .unwrap()
                .clone()
        })
        .collect();
    let key = SigningIdentity::load(&temp.path().join("synthetic-source-key"), "fixture").unwrap();
    let trust = BootstrapTrust {
        public_key_hex: key.public_key_hex(),
        source: Source {
            store_id: cut.manifest.source.store_id.clone(),
            log_epoch: cut.manifest.source.log_epoch.clone(),
        },
        scope_digest: cut.manifest.scope_digest.clone(),
        policy_digest: cut.manifest.policy_digest.clone(),
        manifest_sha256: cut.manifest_sha256.clone(),
    };
    let signature = asof_share::sign_manifest(&cut.manifest, &key).unwrap();
    (
        src,
        serde_json::to_value(cut.manifest).unwrap(),
        records,
        trust,
        signature,
        key,
    )
}

fn signed_frame(manifest: &Json, records: &[Json], signature: &str) -> Json {
    json!({"manifest":manifest,"records":records,"manifest_signature":signature})
}

fn untouched(store: &Store, applier: &Applier) {
    assert!(store.conn.is_autocommit());
    assert!(live_digest(store).is_empty());
    assert!(applier.progress().is_none());
    assert!(super::super::load_admission(store).unwrap().is_none());
}

#[test]
fn signed_cut_commits_its_descriptor_with_facts_and_progress_and_reopens_fail_closed() {
    let (src, manifest, records, trust, signature, _) = signed_cut();
    let (mut replica, mut applier) = empty();
    applier.require_signed_bootstrap(trust.clone()).unwrap();
    assert!(matches!(
        applier
            .apply_frame(
                &mut replica,
                "quipu.asof",
                &signed_frame(&manifest, &records, &signature).to_string(),
                1
            )
            .unwrap(),
        Outcome::Bootstrapped { .. }
    ));
    assert_eq!(live_digest(&src), live_digest(&replica));
    assert!(super::super::load_admission(&replica).unwrap().is_some());
    assert!(applier.require_signed_bootstrap(trust.clone()).is_err());
    drop(applier);
    let mut reopened = Applier::open(&replica).unwrap();
    assert!(matches!(
        reopened
            .apply_frame(&mut replica, "quipu.changes", "{}", 2)
            .unwrap(),
        Outcome::Refused { .. }
    ));
    let mut wrong = trust.clone();
    wrong.public_key_hex = "00".repeat(32);
    assert!(reopened.require_signed_bootstrap(wrong).is_err());
    reopened.require_signed_bootstrap(trust).unwrap();
    assert!(matches!(
        reopened
            .apply_frame(&mut replica, "quipu.changes", "{}", 3)
            .unwrap(),
        Outcome::Refused { .. }
    ));
}

#[test]
fn missing_wrong_or_self_declared_signature_and_changed_descriptor_never_publish() {
    let (_, manifest, records, trust, signature, _) = signed_cut();
    let good = signed_frame(&manifest, &records, &signature);
    let mut variants = Vec::new();
    let mut unsigned = good.clone();
    unsigned
        .as_object_mut()
        .unwrap()
        .remove("manifest_signature");
    variants.push(unsigned);
    let mut wrong = good.clone();
    wrong["manifest_signature"] = json!("00".repeat(64));
    wrong["public_key"] = json!("00".repeat(32));
    variants.push(wrong);
    for field in ["scope_digest", "policy_digest", "shapes_hash"] {
        let mut changed = good.clone();
        changed["manifest"][field] = json!("sha256:changed");
        variants.push(changed);
    }
    for field in ["store_id", "log_epoch"] {
        let mut changed = good.clone();
        changed["manifest"]["source"][field] = json!("changed-source");
        variants.push(changed);
    }
    let mut changed = good.clone();
    changed["manifest"]["watermark_tx"] = json!(manifest["watermark_tx"].as_i64().unwrap() + 1);
    variants.push(changed);
    for frame in variants {
        let (mut replica, mut applier) = empty();
        applier.require_signed_bootstrap(trust.clone()).unwrap();
        assert!(matches!(
            applier
                .apply_frame(&mut replica, "quipu.asof", &frame.to_string(), 1)
                .unwrap(),
            Outcome::Refused { .. }
        ));
        untouched(&replica, &applier);
    }
    let typed: Manifest = serde_json::from_value(manifest).unwrap();
    assert!(!asof_share::verify_manifest(
        &typed,
        &"00".repeat(32),
        &signature
    ));
}

#[test]
fn signed_manifest_does_not_waive_record_integrity_or_allow_feed_before_bootstrap() {
    let (_, manifest, records, trust, signature, _) = signed_cut();
    let (mut replica, mut applier) = empty();
    applier.require_signed_bootstrap(trust.clone()).unwrap();
    for event in [
        "quipu.changes",
        "quipu.changes.begin",
        "quipu.replicate.session",
    ] {
        assert!(matches!(
            applier.apply_frame(&mut replica, event, "{}", 0).unwrap(),
            Outcome::Refused { .. }
        ));
        untouched(&replica, &applier);
    }
    let mut frame = signed_frame(&manifest, &records, &signature);
    frame["records"][0]["entity"] = json!("urn:tampered:fixture");
    assert!(matches!(
        applier
            .apply_frame(&mut replica, "quipu.asof", &frame.to_string(), 1)
            .unwrap(),
        Outcome::Refused { .. }
    ));
    untouched(&replica, &applier);
}

#[test]
fn chunked_signed_cut_checks_before_staging_and_commits_only_after_barrier() {
    let (src, manifest, records, trust, signature, _) = signed_cut();
    let mut frames = chunked(&manifest, &records, 1);
    let mut begin: Json = serde_json::from_str(&frames[0].1).unwrap();
    begin["manifest_signature"] = json!(signature);
    frames[0].1 = begin.to_string();
    let (mut replica, mut applier) = empty();
    applier.require_signed_bootstrap(trust).unwrap();
    for (event, data) in &frames[..frames.len() - 1] {
        applier.apply_frame(&mut replica, event, data, 1).unwrap();
        assert!(applier.progress().is_none());
        assert!(super::super::load_admission(&replica).unwrap().is_none());
    }
    let (event, data) = frames.last().unwrap();
    assert!(matches!(
        applier.apply_frame(&mut replica, event, data, 2).unwrap(),
        Outcome::Bootstrapped { .. }
    ));
    assert_eq!(live_digest(&src), live_digest(&replica));
}

#[test]
fn admission_write_failure_rolls_back_facts_and_cursor() {
    let (_, manifest, records, trust, signature, _) = signed_cut();
    let (mut replica, mut applier) = empty();
    applier.require_signed_bootstrap(trust).unwrap();
    replica.conn.execute_batch("CREATE TRIGGER refuse_bootstrap_admission BEFORE INSERT ON replica_bootstrap_admission BEGIN SELECT RAISE(ABORT,'synthetic refusal'); END;").unwrap();
    assert!(
        applier
            .apply_frame(
                &mut replica,
                "quipu.asof",
                &signed_frame(&manifest, &records, &signature).to_string(),
                1
            )
            .is_err()
    );
    untouched(&replica, &applier);
}

#[test]
fn unsigned_or_unknown_frame_interrupts_the_entire_staged_cut() {
    let (_, manifest, records, trust, signature, _) = signed_cut();
    for event in [
        "quipu.changes",
        "quipu.asof.future",
        "quipu.replicate.future",
    ] {
        let (mut replica, mut applier) = empty();
        applier.require_signed_bootstrap(trust.clone()).unwrap();
        let mut frames = chunked(&manifest, &records, 1);
        let mut begin: Json = serde_json::from_str(&frames[0].1).unwrap();
        begin["manifest_signature"] = json!(signature);
        frames[0].1 = begin.to_string();
        for (name, data) in &frames[..2] {
            applier.apply_frame(&mut replica, name, data, 0).unwrap();
        }
        assert!(!replica.conn.is_autocommit());
        assert!(matches!(
            applier.apply_frame(&mut replica, event, "{}", 1).unwrap(),
            Outcome::Refused { .. }
        ));
        untouched(&replica, &applier);
    }
}

#[test]
fn unknown_frame_invalidates_a_previously_valid_freshness_proof() {
    let (src, manifest, records, trust, signature, key) = signed_cut();
    let (mut replica, mut applier) = empty();
    applier.require_signed_bootstrap(trust).unwrap();
    applier
        .apply_frame(
            &mut replica,
            "quipu.asof",
            &signed_frame(&manifest, &records, &signature).to_string(),
            0,
        )
        .unwrap();
    let request = crate::replicate_source::StreamRequest {
        graphs: vec!["ROOT".into(), BOARD.into()],
        context_graphs: vec![],
        since: manifest["watermark_tx"].as_i64().unwrap(),
        expected_store_id: src.store_id().unwrap(),
    };
    let (mut session, opening) = crate::replicate_source::Session::open(
        &src,
        &request,
        "ab".repeat(16),
        std::sync::Arc::new(key),
    )
    .unwrap();
    applier
        .apply_frame(&mut replica, opening.event, &opening.data, 1)
        .unwrap();
    let nonce = [7_u8; 16];
    applier.challenge(session.id(), nonce, 10).unwrap();
    let (frames, answered) = session.poll(&src, 100, Some(nonce)).unwrap();
    assert!(answered);
    for frame in frames {
        applier
            .apply_frame(&mut replica, frame.event, &frame.data, 12)
            .unwrap();
    }
    assert!(applier.freshness(15).bounded);
    assert!(matches!(
        applier
            .apply_frame(&mut replica, "quipu.replicate.future", "{}", 16)
            .unwrap(),
        Outcome::Refused { .. }
    ));
    assert!(!applier.freshness(17).bounded);
}

#[path = "applier_bootstrap_tail_signature_tests.rs"]
mod tail;
