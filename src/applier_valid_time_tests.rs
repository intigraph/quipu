//! A replica must answer historical queries like its primary after promotion.
use super::*;
use crate::sparql::{QueryResult, TemporalContext, query_temporal};

const SEED: &str = "2026-01-01T00:00:00Z";
const CORRECTION: &str = "2026-03-01T00:00:00Z";
const PROMOTION: &str = "2026-06-01T00:00:00Z";

fn answers(store: &Store, at: &str, tx: Option<i64>) -> Vec<String> {
    let QueryResult::Select { rows, .. } = query_temporal(
        store,
        "SELECT ?o WHERE { <urn:example:item> <urn:example:status> ?o }",
        &TemporalContext {
            valid_at: Some(at.into()),
            as_of_tx: tx,
            ..Default::default()
        },
    )
    .unwrap() else {
        panic!("expected SELECT");
    };
    let mut values: Vec<String> = rows.into_iter().map(|r| format!("{:?}", r["o"])).collect();
    values.sort();
    values
}

#[test]
fn backdated_promote_answers_match_primary_before_at_and_after_correction() {
    let mut primary = Store::open_in_memory().unwrap();
    let mut old = d(
        &primary,
        "urn:example:item",
        "urn:example:status",
        Value::Str("old".into()),
        Op::Assert,
    );
    old.valid_from = SEED.into();
    primary.transact(&[old.clone()], SEED, None, None).unwrap();
    let fork = primary
        .fork_create("correction", primary.latest_tx_id().unwrap(), SEED, None)
        .unwrap();
    old.op = Op::Retract;
    let mut new = old.clone();
    new.op = Op::Assert;
    new.value = Value::Str("new".into());
    new.valid_from = CORRECTION.into();
    primary
        .transact_to_graph(&[old, new], CORRECTION, None, None, fork.g)
        .unwrap();
    let before = primary.latest_tx_id().unwrap();
    primary.fork_promote("correction", PROMOTION, None).unwrap();
    assert_eq!(
        answers(&primary, "2026-02-01T00:00:00Z", None),
        ["Str(\"old\")"]
    );
    assert_eq!(answers(&primary, CORRECTION, None), ["Str(\"new\")"]);
    assert_eq!(
        answers(&primary, CORRECTION, Some(before)),
        ["Str(\"old\")"]
    );
    let mut replica = Store::open_in_memory().unwrap();
    let mut applier = Applier::open(&replica).unwrap();
    for page in pages(&primary) {
        let result = applier
            .apply_frame(&mut replica, "quipu.changes", &page, 1)
            .unwrap();
        assert!(matches!(result, Outcome::Applied { .. }), "{result:?}");
    }
    for at in [
        "2025-12-01T00:00:00Z",
        "2026-02-01T00:00:00Z",
        CORRECTION,
        "2026-04-01T00:00:00Z",
        PROMOTION,
        "2026-07-01T00:00:00Z",
    ] {
        assert_eq!(
            answers(&replica, at, None),
            answers(&primary, at, None),
            "valid time {at}"
        );
    }
    let source = applier.progress().unwrap().source.clone();
    let local_before = super::super::map::local_cursor(&replica, &source, before)
        .unwrap()
        .unwrap();
    assert_eq!(
        answers(&replica, CORRECTION, Some(local_before)),
        answers(&primary, CORRECTION, Some(before))
    );
}

#[test]
fn legacy_retracts_and_explicit_transaction_closes_have_the_same_digest() {
    let src = source();
    let current = pages(&src);
    let mut replica = Store::open_in_memory().unwrap();
    let mut applier = Applier::open(&replica).unwrap();
    for raw in &current {
        assert!(matches!(
            applier
                .apply_frame(&mut replica, "quipu.changes", raw, 1)
                .unwrap(),
            Outcome::Applied { .. }
        ));
    }
    assert_eq!(live_digest(&src), live_digest(&replica));
    for raw in &current {
        let mut legacy: Json = serde_json::from_str(raw).unwrap();
        for record in legacy["records"].as_array_mut().unwrap() {
            record.as_object_mut().unwrap().remove("retracted_valid_at");
        }
        assert_eq!(
            applier
                .apply_frame(&mut replica, "quipu.changes", &legacy.to_string(), 2)
                .unwrap(),
            Outcome::Duplicate
        );
    }
}

#[test]
fn malformed_or_unsupported_close_metadata_refuses_without_advancing() {
    for bad in [
        None,
        Some(Json::Null),
        Some(json!(7)),
        Some(json!({"version": 2})),
        Some(json!("")),
        Some(json!("not-a-time")),
        Some(json!("2026-02-30")),
    ] {
        let src = source();
        let feed = pages(&src);
        let mut replica = Store::open_in_memory().unwrap();
        let mut applier = Applier::open(&replica).unwrap();
        for raw in &feed[..3] {
            applier
                .apply_frame(&mut replica, "quipu.changes", raw, 1)
                .unwrap();
        }
        let before = live_digest(&replica);
        let cursor = applier.progress().unwrap().source_tx;
        let mut page: Json = serde_json::from_str(&feed[3]).unwrap();
        if let Some(value) = &bad {
            page["records"][0]["retracted_valid_at"] = value.clone();
        } else {
            page["records"][0]
                .as_object_mut()
                .unwrap()
                .remove("retracted_valid_at");
        }
        let outcome = applier
            .apply_frame(&mut replica, "quipu.changes", &page.to_string(), 2)
            .unwrap();
        assert!(
            matches!(outcome, Outcome::Refused { .. }),
            "{bad:?}: {outcome:?}"
        );
        assert_eq!(live_digest(&replica), before);
        assert_eq!(applier.progress().unwrap().source_tx, cursor);
    }
}

#[test]
fn a_changed_close_on_redelivery_stops_as_a_page_or_chunk() {
    for chunked in [false, true] {
        let feed = pages(&source());
        let mut replica = Store::open_in_memory().unwrap();
        let mut applier = Applier::open(&replica).unwrap();
        for raw in &feed {
            applier
                .apply_frame(&mut replica, "quipu.changes", raw, 1)
                .unwrap();
        }
        let before = live_digest(&replica);
        let mut page: Json = serde_json::from_str(&feed[3]).unwrap();
        page["records"][0]["retracted_valid_at"] = json!(CORRECTION);
        let raw = page.to_string();
        let frames = if chunked {
            chunk_frames(&raw, 1, true)
        } else {
            vec![("quipu.changes", raw)]
        };
        let last = frames
            .iter()
            .map(|(e, d)| applier.apply_frame(&mut replica, e, d, 2).unwrap())
            .last()
            .unwrap();
        assert!(
            matches!(
                last,
                Outcome::Stopped {
                    reason: Stop::DigestMismatch { .. }
                }
            ),
            "{last:?}"
        );
        assert_eq!(live_digest(&replica), before);
    }
}

#[test]
fn assertions_keep_their_bound_and_reject_retraction_only_metadata() {
    let store = Store::open_in_memory().unwrap();
    let mut record = json!({"entity":"urn:example:e", "attribute":"urn:example:p", "op":"assert", "value":1, "valid_from":SEED, "valid_to":CORRECTION});
    let datum = super::super::record_to_datum(&store, &record, PROMOTION)
        .unwrap()
        .unwrap();
    assert_eq!(datum.valid_to.as_deref(), Some(CORRECTION));
    record["retracted_valid_at"] = json!(CORRECTION);
    assert!(
        super::super::record_to_datum(&store, &record, PROMOTION)
            .unwrap()
            .is_err()
    );
}

#[test]
fn feed_refuses_a_close_it_cannot_recover_instead_of_guessing() {
    let src = source();
    src.conn
        .execute(
            "UPDATE facts SET retracted_tx=NULL WHERE op=1 AND retracted_tx IS NOT NULL",
            [],
        )
        .unwrap();
    let err = src
        .changes_after(0, 100, Capture::OldAndNewValues, None)
        .unwrap_err();
    assert!(
        err.to_string().contains("no unambiguous valid-time close"),
        "{err}"
    );
}

#[test]
fn feed_refuses_disagreeing_closed_claims_instead_of_picking_one() {
    let mut src = Store::open_in_memory().unwrap();
    let mut datum = d(
        &src,
        "urn:example:e",
        "urn:example:p",
        Value::Int(1),
        Op::Assert,
    );
    let first = src
        .transact(&[datum.clone()], SEED, None, Some("producer-a"))
        .unwrap();
    src.transact(&[datum.clone()], SEED, None, Some("producer-b"))
        .unwrap();
    datum.op = Op::Retract;
    src.transact(&[datum], PROMOTION, None, None).unwrap();
    assert!(
        src.changes_after(0, 100, Capture::OldAndNewValues, None)
            .is_ok()
    );
    src.conn
        .execute(
            "UPDATE facts SET valid_to=?1 WHERE op=1 AND tx=?2",
            rusqlite::params![CORRECTION, first],
        )
        .unwrap();
    assert!(
        src.changes_after(0, 100, Capture::OldAndNewValues, None)
            .is_err()
    );
}
