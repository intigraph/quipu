//! The seam between the as-of share producer (`asof_share::cut`) and the
//! shared applier's bootstrap: the frames a real cut sends, fed to an empty
//! replica, bootstrap it to the source's live facts, and a tampered part is
//! refused. Single frame and chunked, built the way the server sends them.
use serde_json::{Value as Json, json};

use super::{Applier, Outcome, live_digest};
use crate::asof_share::{Limits, Request, cut};
use crate::store::{Datum, Store};
use crate::types::{Op, Value};

const T: &str = "2026-10-24T00:00:00Z";
const BOARD: &str = "urn:graph:seam-board";

fn source(dir: &std::path::Path, n: usize) -> Store {
    let mut s = Store::open(dir.join("source.db").to_str().unwrap()).unwrap();
    let g = s.graph_create(BOARD).unwrap();
    for i in 0..n {
        let d = |a: &str, v: Value| Datum {
            entity: s.intern(&format!("urn:item:{i}")).unwrap(),
            attribute: s.intern(a).unwrap(),
            value: v,
            valid_from: T.into(),
            valid_to: None,
            op: Op::Assert,
        };
        let rows = vec![
            d(
                "urn:p:status",
                Value::Str(if i % 3 == 0 { "closed" } else { "open" }.into()),
            ),
            d("urn:p:priority", Value::Int((i % 4) as i64)),
            d(
                "urn:p:title",
                Value::Str(format!("item \"{i}\" \u{e9}\u{1f600}")),
            ),
            d("urn:p:blob", Value::Bytes(vec![0, 1, (i % 256) as u8])),
        ];
        s.transact_to_graph(&rows, T, None, None, g).unwrap();
    }
    // A superseded value: only the live one belongs in the cut.
    let e = s.intern("urn:item:1").unwrap();
    let a = s.intern("urn:p:status").unwrap();
    let close = |op| Datum {
        entity: e,
        attribute: a,
        value: Value::Str("open".into()),
        valid_from: T.into(),
        valid_to: None,
        op,
    };
    s.transact_to_graph(&[close(Op::Retract)], T, None, None, g)
        .unwrap();
    s
}

/// The frames the server sends for a cut (src/server/asof.rs).
fn frames(
    store: &Store,
    dir: &std::path::Path,
    part_max_len: usize,
) -> Vec<(&'static str, String)> {
    let request = Request {
        graphs: vec![BOARD.into()],
        context_graphs: vec![],
        tx: None,
        expected_store_id: store.store_id().unwrap(),
    };
    let limits = Limits {
        part_max_len,
        disk_floor_bytes: 0,
        ..Limits::default()
    };
    let spool = dir.join("spool");
    std::fs::create_dir_all(&spool).unwrap();
    let c = cut(store, &request, &spool, &limits).expect("cut");
    let lines: Vec<String> = std::fs::read_to_string(&c.spool)
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    if c.parts.parts <= 1 {
        let records = lines.first().map_or(json!([]), |l| {
            serde_json::from_str::<Json>(l).unwrap()["records"].clone()
        });
        return vec![(
            "quipu.asof",
            json!({"manifest": c.manifest, "records": records}).to_string(),
        )];
    }
    let source = json!(c.manifest.source);
    let tx = c.manifest.watermark_tx;
    let mut out = vec![("quipu.asof.begin", json!({
        "source_tx": tx, "parts": c.parts.parts, "records": c.parts.records, "bytes": c.parts.bytes,
        "digest": c.parts.digest, "source": source, "manifest": c.manifest, "manifest_sha256": c.manifest_sha256,
    }).to_string())];
    out.extend(lines.into_iter().map(|l| ("quipu.asof.part", l)));
    out.push(("quipu.asof.commit", json!({
        "source_tx": tx, "parts": c.parts.parts, "records": c.parts.records, "digest": c.parts.digest,
        "source": source, "manifest_sha256": c.manifest_sha256,
    }).to_string()));
    out
}

fn live(store: &Store) -> std::collections::BTreeSet<String> {
    live_digest(store)
}

fn bootstrap(frames: &[(&str, String)]) -> (Store, Outcome) {
    let mut replica = Store::open_in_memory().unwrap();
    let mut applier = Applier::open(&replica).unwrap();
    let mut last = Outcome::Ignored;
    for (event, data) in frames {
        last = applier.apply_frame(&mut replica, event, data, 1).unwrap();
    }
    (replica, last)
}

#[test]
fn a_real_single_frame_cut_bootstraps_an_equal_replica() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), 20);
    let f = frames(&src, dir.path(), usize::MAX / 2);
    assert_eq!(f.len(), 1, "one frame");
    let (replica, outcome) = bootstrap(&f);
    assert!(
        matches!(outcome, Outcome::Bootstrapped { .. }),
        "{outcome:?}"
    );
    assert_eq!(live(&src), live(&replica));
}

#[test]
fn a_real_chunked_cut_bootstraps_an_equal_replica_and_a_tampered_part_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), 300);
    let f = frames(&src, dir.path(), 4096);
    assert!(f.len() > 4, "chunked: {} frames", f.len());
    let (replica, outcome) = bootstrap(&f);
    assert!(
        matches!(outcome, Outcome::Bootstrapped { .. }),
        "{outcome:?}"
    );
    assert_eq!(live(&src), live(&replica));
    // One record altered inside a part, with the chain re-signed is not
    // possible for an attacker without breaking digest_before; alter the
    // record and leave the chain: the chain check refuses it.
    let mut bad = f.clone();
    bad[1].1 = bad[1].1.replacen("open", "OPEN", 1);
    let (replica, outcome) = bootstrap(&bad);
    assert!(
        !matches!(outcome, Outcome::Bootstrapped { .. }),
        "{outcome:?}"
    );
    assert!(live(&replica).is_empty(), "nothing of a refused cut stays");
}
