//! Slice 2: a replica bootstrapped from an as-of share and tailed from its
//! watermark equals its source, and every share the follower cannot trust is
//! refused with nothing written.
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};

use super::super::{Applier, Outcome, live_digest};
use super::{SHARE_SCHEMA, share_graph_digest};
use crate::store::changes::Capture;
use crate::store::{Datum, Store};
use crate::types::{Op, Value};

const T: &str = "2026-10-24T00:00:00Z";
const BOARD: &str = "urn:graph:board";

fn d(store: &Store, e: &str, a: &str, v: Value, op: Op, valid_to: Option<&str>) -> Datum {
    Datum {
        entity: store.intern(e).unwrap(),
        attribute: store.intern(a).unwrap(),
        value: v,
        valid_from: T.into(),
        valid_to: valid_to.map(Into::into),
        op,
    }
}

/// ROOT and board writes: bytes, a bounded valid interval, a reference and a
/// retraction that a share must NOT carry.
fn source() -> Store {
    let mut s = Store::open_in_memory().unwrap();
    let open = d(
        &s,
        "urn:item:1",
        "urn:p:status",
        Value::Str("open".into()),
        Op::Assert,
        None,
    );
    let blob = d(
        &s,
        "urn:item:1",
        "urn:p:blob",
        Value::Bytes(vec![0, 1, 255]),
        Op::Assert,
        None,
    );
    let window = d(
        &s,
        "urn:item:1",
        "urn:p:window",
        Value::Str("q4".into()),
        Op::Assert,
        Some("2026-12-31T00:00:00Z"),
    );
    s.transact(&[open, blob, window], T, None, None).unwrap();
    let g = s.graph_create(BOARD).unwrap();
    let link = Value::Ref(s.intern("urn:item:2").unwrap());
    let blocks = d(&s, "urn:item:1", "urn:p:blocks", link, Op::Assert, None);
    s.transact_to_graph(&[blocks], T, None, None, g).unwrap();
    let retract = d(
        &s,
        "urn:item:1",
        "urn:p:status",
        Value::Str("open".into()),
        Op::Retract,
        None,
    );
    let closed = d(
        &s,
        "urn:item:1",
        "urn:p:status",
        Value::Str("closed".into()),
        Op::Assert,
        None,
    );
    s.transact(&[retract, closed], T, None, None).unwrap();
    s
}

/// The cut a producer emits: every live fact as an assert record in the
/// `/changes` form, plus its manifest.
fn cut(s: &Store) -> (Json, Vec<Json>) {
    let mut stmt = s
        .conn
        .prepare(
            "SELECT f.tx, f.e, f.a, f.v, f.g, f.valid_from, f.valid_to, t.timestamp \
             FROM facts f JOIN transactions t ON t.id = f.tx \
             WHERE f.op = 1 AND f.retracted_tx IS NULL ORDER BY f.g, f.e, f.a, f.rowid",
        )
        .unwrap();
    type Row = (
        i64,
        i64,
        i64,
        Vec<u8>,
        i64,
        String,
        Option<String>,
        Option<String>,
    );
    let rows: Vec<Row> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let records: Vec<Json> = rows
        .into_iter()
        .enumerate()
        .map(|(i, (tx, e, a, v, g, from, to, ts))| {
            json!({
                "tx": tx, "sequence": i, "timestamp": ts, "op": "assert",
                "graph": if g == 0 { "ROOT".to_string() } else { s.resolve(g).unwrap() },
                "entity": s.resolve(e).unwrap(), "attribute": s.resolve(a).unwrap(),
                "value": s.value_json(&Value::from_bytes(&v).unwrap()).unwrap(),
                "valid_from": from, "valid_to": to,
            })
        })
        .collect();
    let mut graphs = serde_json::Map::new();
    for graph in ["ROOT", BOARD] {
        let held: Vec<&Json> = records.iter().filter(|r| r["graph"] == graph).collect();
        graphs.insert(
            graph.into(),
            json!({"records": held.len(), "digest": format!("sha256:{}", share_graph_digest(&held)), "frozen": null}),
        );
    }
    let manifest = json!({
        "schema": SHARE_SCHEMA,
        "source": {"store_id": s.store_id().unwrap(), "log_epoch": s.log_epoch().unwrap()},
        "watermark_tx": s.latest_tx_id().unwrap(),
        "completeness": {"valid_time": true, "bytes": "lossless", "graphs": graphs},
    });
    (manifest, records)
}

fn one_frame(manifest: &Json, records: &[Json]) -> String {
    json!({"manifest": manifest, "records": records}).to_string()
}

/// The same share as begin, parts of `per_part` records, and commit.
fn chunked(manifest: &Json, records: &[Json], per_part: usize) -> Vec<(&'static str, String)> {
    let tx = manifest["watermark_tx"].as_i64().unwrap();
    let mut hasher = Sha256::new();
    let mut parts = Vec::new();
    for (index, chunk) in records.chunks(per_part).enumerate() {
        let before = hex::encode(hasher.clone().finalize());
        let data =
            json!({"source_tx": tx, "index": index, "digest_before": before, "records": chunk})
                .to_string();
        hasher.update(data.as_bytes());
        parts.push(data);
    }
    let digest = hex::encode(hasher.finalize());
    let mut frames = vec![(
        "quipu.asof.begin",
        json!({"manifest": manifest, "parts": parts.len(), "records": records.len(), "digest": digest}).to_string(),
    )];
    frames.extend(parts.into_iter().map(|p| ("quipu.asof.part", p)));
    frames.push((
        "quipu.asof.commit",
        json!({"source_tx": tx, "digest": digest}).to_string(),
    ));
    frames
}

/// Pages after `since`, one transaction each, as the stream sends them.
fn tail(s: &Store, since: i64) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = since;
    loop {
        let page = s
            .changes_after(cursor, 1, Capture::OldAndNewValues, None)
            .unwrap();
        if page.records.is_empty() {
            return out;
        }
        cursor = page.next_tx;
        out.push(page.to_json().to_string());
    }
}

fn empty() -> (Store, Applier) {
    let replica = Store::open_in_memory().unwrap();
    let applier = Applier::open(&replica).unwrap();
    (replica, applier)
}

#[test]
fn a_bootstrapped_replica_equals_its_source_and_tails_to_equality() {
    let mut src = source();
    let (manifest, records) = cut(&src);
    let (mut replica, mut applier) = empty();
    let o = applier
        .apply_frame(
            &mut replica,
            "quipu.asof",
            &one_frame(&manifest, &records),
            1,
        )
        .unwrap();
    let watermark = manifest["watermark_tx"].as_i64().unwrap();
    assert_eq!(
        o,
        Outcome::Bootstrapped {
            source_tx: watermark
        }
    );
    assert_eq!(
        live_digest(&src),
        live_digest(&replica),
        "equal at the watermark, valid time and bytes included"
    );
    // Writes after the cut reach the replica through the tail alone.
    let g = src.graph_create(BOARD).unwrap();
    let more = d(
        &src,
        "urn:item:3",
        "urn:p:status",
        Value::Str("open".into()),
        Op::Assert,
        None,
    );
    src.transact_to_graph(&[more], T, None, None, g).unwrap();
    let gone = d(
        &src,
        "urn:item:1",
        "urn:p:blob",
        Value::Bytes(vec![0, 1, 255]),
        Op::Retract,
        None,
    );
    src.transact(&[gone], T, None, None).unwrap();
    for page in tail(&src, watermark) {
        let o = applier
            .apply_frame(&mut replica, "quipu.changes", &page, 2)
            .unwrap();
        assert!(matches!(o, Outcome::Applied { .. }), "{o:?}");
    }
    assert_eq!(live_digest(&src), live_digest(&replica));
    // The cursor survives reopening.
    let reopened = Applier::open(&replica).unwrap();
    assert_eq!(
        reopened.progress().unwrap().source_tx,
        src.latest_tx_id().unwrap()
    );
}

#[test]
fn a_chunked_share_equals_the_single_frame_and_applies_only_at_commit() {
    let src = source();
    let (manifest, records) = cut(&src);
    let frames = chunked(&manifest, &records, 2);
    assert!(frames.len() > 3);
    let (mut replica, mut applier) = empty();
    for (event, data) in &frames[..frames.len() - 1] {
        let o = applier.apply_frame(&mut replica, event, data, 1).unwrap();
        assert!(matches!(o, Outcome::Buffered { .. }), "{o:?}");
        // Parts are written as they arrive, inside the open savepoint; the
        // replica has no cursor yet, so it is not a replica a reader may use.
        assert!(!replica.conn.is_autocommit(), "one open local transaction");
        assert_eq!(applier.freshness(1).source_tx, None);
    }
    let (event, data) = frames.last().unwrap();
    let o = applier.apply_frame(&mut replica, event, data, 2).unwrap();
    assert!(matches!(o, Outcome::Bootstrapped { .. }), "{o:?}");
    assert_eq!(live_digest(&src), live_digest(&replica));
}

#[test]
fn a_broken_chunked_share_resumes_and_writes_nothing() {
    let src = source();
    let (manifest, records) = cut(&src);
    let frames = chunked(&manifest, &records, 2);
    let (mut replica, mut applier) = empty();
    // Part 0 is dropped.
    applier
        .apply_frame(&mut replica, frames[0].0, &frames[0].1, 1)
        .unwrap();
    let o = applier
        .apply_frame(&mut replica, frames[2].0, &frames[2].1, 2)
        .unwrap();
    assert_eq!(o, Outcome::Resume { from: 0 });
    let (event, data) = frames.last().unwrap();
    assert_eq!(
        applier.apply_frame(&mut replica, event, data, 3).unwrap(),
        Outcome::Resume { from: 0 }
    );
    assert!(live_digest(&replica).is_empty());
    assert!(applier.progress().is_none());
    // A restarted cut converges.
    for (event, data) in &frames {
        applier.apply_frame(&mut replica, event, data, 4).unwrap();
    }
    assert_eq!(live_digest(&src), live_digest(&replica));
}

/// Each case must refuse and leave the replica exactly empty.
fn refuses(manifest: &Json, records: &[Json], needle: &str) {
    let (mut replica, mut applier) = empty();
    let o = applier
        .apply_frame(&mut replica, "quipu.asof", &one_frame(manifest, records), 1)
        .unwrap();
    match &o {
        Outcome::Refused { reason } => assert!(reason.contains(needle), "{needle}: {reason}"),
        other => panic!("{needle}: expected a refusal, got {other:?}"),
    }
    assert!(
        live_digest(&replica).is_empty(),
        "{needle}: nothing written"
    );
    assert!(applier.progress().is_none(), "{needle}: no cursor");
    assert!(
        Applier::open(&replica).unwrap().progress().is_none(),
        "{needle}: none persisted"
    );
}

#[test]
fn every_untrustworthy_share_is_refused_with_nothing_written() {
    let src = source();
    let (manifest, records) = cut(&src);
    let mut m = manifest.clone();
    m["completeness"]["bytes"] = json!("length_only");
    refuses(&m, &records, "valid time and lossless bytes");
    let mut m = manifest.clone();
    m["completeness"]["valid_time"] = json!(false);
    refuses(&m, &records, "valid time and lossless bytes");
    let mut m = manifest.clone();
    m["schema"] = json!("quipu-acquisition/v1");
    refuses(&m, &records, "share schema");
    let mut m = manifest.clone();
    m.as_object_mut().unwrap().remove("source");
    refuses(&m, &records, "source");
    // A record altered in transit: the graph's digest no longer matches.
    let mut tampered = records.clone();
    tampered[0]["value"] = json!("changed");
    refuses(&manifest, &tampered, "count and digest");
    // A record dropped: the count no longer matches.
    refuses(&manifest, &records[1..], "count and digest");
    // A record in a graph the manifest does not list.
    let mut stray = records.clone();
    stray[0]["graph"] = json!("urn:graph:elsewhere");
    refuses(&manifest, &stray, "outside the share's scope");
    // A share carries live facts only.
    let mut retracting = records.clone();
    retracting[0]["op"] = json!("retract");
    refuses(&manifest, &retracting, "live facts only");
    // A length-only byte value fails the whole share, not just its graph.
    let i = records
        .iter()
        .position(|r| r["value"].get("base64").is_some())
        .unwrap();
    let mut lossy = records.clone();
    lossy[i]["value"] = json!({"bytes_len": 3});
    let mut m = manifest.clone();
    let graph = lossy[i]["graph"].as_str().unwrap().to_string();
    let held: Vec<&Json> = lossy
        .iter()
        .filter(|r| r["graph"] == graph.as_str())
        .collect();
    m["completeness"]["graphs"][&graph]["digest"] = json!(share_graph_digest(&held));
    refuses(&m, &lossy, "length only");
}

#[test]
fn only_an_empty_replica_is_bootstrapped() {
    let src = source();
    let (manifest, records) = cut(&src);
    let (mut replica, mut applier) = empty();
    applier
        .apply_frame(
            &mut replica,
            "quipu.asof",
            &one_frame(&manifest, &records),
            1,
        )
        .unwrap();
    let before = live_digest(&replica);
    let again = applier
        .apply_frame(
            &mut replica,
            "quipu.asof",
            &one_frame(&manifest, &records),
            2,
        )
        .unwrap();
    assert!(matches!(again, Outcome::Refused { .. }), "{again:?}");
    assert_eq!(before, live_digest(&replica));
    // A store holding facts but no cursor is not empty either.
    let mut other = Store::open_in_memory().unwrap();
    let x = d(
        &other,
        "urn:x",
        "urn:p",
        Value::Str("v".into()),
        Op::Assert,
        None,
    );
    other.transact(&[x], T, None, None).unwrap();
    let mut a = Applier::open(&other).unwrap();
    let o = a
        .apply_frame(&mut other, "quipu.asof", &one_frame(&manifest, &records), 1)
        .unwrap();
    assert!(matches!(o, Outcome::Refused { .. }), "{o:?}");
}

#[test]
fn a_new_epoch_after_bootstrap_means_rebootstrap() {
    let mut src = source();
    let (manifest, records) = cut(&src);
    let watermark = manifest["watermark_tx"].as_i64().unwrap();
    let (mut replica, mut applier) = empty();
    applier
        .apply_frame(
            &mut replica,
            "quipu.asof",
            &one_frame(&manifest, &records),
            1,
        )
        .unwrap();
    let more = d(
        &src,
        "urn:item:9",
        "urn:p:status",
        Value::Str("open".into()),
        Op::Assert,
        None,
    );
    src.transact(&[more], T, None, None).unwrap();
    let mut page: Json = serde_json::from_str(&tail(&src, watermark)[0]).unwrap();
    page["source"]["log_epoch"] = json!("another-epoch");
    let before = live_digest(&replica);
    let o = applier
        .apply_frame(&mut replica, "quipu.changes", &page.to_string(), 2)
        .unwrap();
    assert!(matches!(o, Outcome::Rebootstrap { .. }), "{o:?}");
    assert_eq!(before, live_digest(&replica));
}

#[test]
fn the_graph_digest_ignores_key_order_and_record_order_but_not_content() {
    let a = json!({"b": 1, "a": {"y": 2, "x": [1, {"q": 1, "p": 2}]}});
    let b: Json = serde_json::from_str(r#"{"a":{"x":[1,{"p":2,"q":1}],"y":2},"b":1}"#).unwrap();
    let c = json!({"a": 1});
    assert_eq!(share_graph_digest(&[&a, &c]), share_graph_digest(&[&c, &b]));
    let changed = json!({"b": 2, "a": {"y": 2, "x": [1, {"q": 1, "p": 2}]}});
    assert_ne!(
        share_graph_digest(&[&a, &c]),
        share_graph_digest(&[&changed, &c])
    );
}

#[test]
fn a_share_is_streamed_never_held_and_a_reopen_mid_share_discards_it() {
    let src = source();
    let (manifest, records) = cut(&src);
    let frames = chunked(&manifest, &records, 2);
    let (mut replica, mut applier) = empty();
    for (event, data) in &frames[..frames.len() - 1] {
        applier.apply_frame(&mut replica, event, data, 1).unwrap();
        // The applier keeps no records: a part's facts are already in the
        // store, so its memory is one part, not the share.
        let held = applier
            .share
            .as_ref()
            .and_then(|s| s.chain.as_ref())
            .map_or(0, |c| c.buffered.len());
        assert_eq!(held, 0);
    }
    assert!(!live_digest(&replica).is_empty(), "parts were written");
    drop(applier);
    // The applier went away mid-share: a new one rolls the share back.
    let reopened = Applier::open(&replica).unwrap();
    assert!(replica.conn.is_autocommit());
    assert!(
        live_digest(&replica).is_empty(),
        "nothing of the share stays"
    );
    assert!(reopened.progress().is_none());
}

#[test]
fn a_store_error_mid_share_leaves_no_fact_visible_even_on_the_same_connection() {
    let src = source();
    let (manifest, records) = cut(&src);
    let (mut replica, mut applier) = empty();
    // The progress write is the last step before the release: make it fail.
    replica
        .conn
        .execute_batch("DROP TABLE replica_progress")
        .unwrap();
    let frame = one_frame(&manifest, &records);
    assert!(
        applier
            .apply_frame(&mut replica, "quipu.asof", &frame, 1)
            .is_err()
    );
    assert!(
        replica.conn.is_autocommit(),
        "the share's savepoint is still open"
    );
    assert!(
        live_digest(&replica).is_empty(),
        "facts of the failed share are visible"
    );
    assert!(applier.progress().is_none());
}
