//! As-of share conformance (as-of-share.md §Verification plan; aegis-s24.1).
//!
//! A follower bootstraps from a cut at `T`, then tails `/changes` from `T`.
//! It must equal its source at `T` and again at the tail's end, by the same
//! live-fact digest the feed-replay tests use.

use std::path::Path;

use serde_json::Value as Json;

use super::replay_tests::{apply_tx, datum, live_digest, replay_onto};
use crate::asof_share::{AbortReason, Cut, CutError, Limits, Request, cut, cut_with_control};
use crate::store::Store;
use crate::types::{Op, Value};

const G: &str = "http://example.org/graph/hosts";
const TS1: &str = "2026-04-01T00:00:00Z";
const TS2: &str = "2026-04-02T00:00:00Z";
const TS3: &str = "2026-04-03T00:00:00Z";

fn request(store: &Store, tx: Option<i64>) -> Request {
    Request {
        graphs: vec![G.into()],
        context_graphs: Vec::new(),
        tx,
        expected_store_id: store.store_id().unwrap(),
    }
}

/// Every record the spool holds, in order, after checking each part's
/// running digest and the cut's final one.
fn spooled_records(cut: &Cut) -> Vec<Json> {
    use sha2::{Digest, Sha256};
    let text = std::fs::read_to_string(&cut.spool).unwrap();
    let mut hasher = Sha256::new();
    let mut records = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let part: Json = serde_json::from_str(line).unwrap();
        assert_eq!(part["index"], index, "{line}");
        assert_eq!(
            part["digest_before"],
            hex::encode(hasher.clone().finalize()),
            "part {index} digest_before"
        );
        assert_eq!(part["source_tx"], cut.manifest.watermark_tx);
        hasher.update(line.as_bytes());
        records.extend(part["records"].as_array().unwrap().iter().cloned());
    }
    assert_eq!(hex::encode(hasher.finalize()), cut.parts.digest);
    assert_eq!(text.lines().count(), cut.parts.parts);
    assert_eq!(records.len(), cut.parts.records);
    records
}

/// Load a cut into an empty replica: one transaction per graph.
fn bootstrap(cut: &Cut) -> Store {
    let mut replica = Store::open_in_memory().unwrap();
    let records = spooled_records(cut);
    let mut by_graph: Vec<(String, Vec<&Json>)> = Vec::new();
    for record in &records {
        let graph = record["graph"].as_str().unwrap().to_string();
        match by_graph.last_mut() {
            Some((last, list)) if *last == graph => list.push(record),
            _ => by_graph.push((graph, vec![record])),
        }
    }
    for (_, records) in by_graph {
        apply_tx(&mut replica, &records, TS1);
    }
    replica
}

fn assert_equal(source: &Store, replica: &Store, when: &str) {
    let (want, got) = (live_digest(source), live_digest(replica));
    assert!(!want.is_empty(), "fixture wrote nothing live");
    let missing: Vec<_> = want.difference(&got).collect();
    let extra: Vec<_> = got.difference(&want).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "{when}: replica diverges from source\n  missing: {missing:#?}\n  extra: {extra:#?}"
    );
}

/// A graph with bounded and open valid time, a byte value, a retract, and a
/// ref, all in `G`.
fn fixture(store: &mut Store) -> i64 {
    let g = store.graph_create(G).unwrap();
    let host = store.intern("http://example.org/korimako").unwrap();
    let cores = store.intern("http://example.org/cpuCores").unwrap();
    let lease = store.intern("http://example.org/lease").unwrap();
    let blob = store.intern("http://example.org/body").unwrap();
    let runs = store.intern("http://example.org/runsOn").unwrap();
    let node = store.intern("http://example.org/pve1").unwrap();
    let mut bounded = datum(
        host,
        lease,
        Value::Str("globex".into()),
        "2024-01-01T00:00:00Z",
        Op::Assert,
    );
    bounded.valid_to = Some("2024-12-31T00:00:00Z".into());
    store
        .transact_to_graph(
            &[
                datum(host, cores, Value::Int(4), TS1, Op::Assert),
                datum(
                    host,
                    lease,
                    Value::Str("acme".into()),
                    "2025-01-01T00:00:00Z",
                    Op::Assert,
                ),
                bounded,
                datum(
                    host,
                    blob,
                    Value::Bytes(vec![0, 159, 146, 150]),
                    TS1,
                    Op::Assert,
                ),
                datum(host, runs, Value::Ref(node), TS1, Op::Assert),
            ],
            TS1,
            None,
            None,
            g,
        )
        .unwrap();
    store
        .transact_to_graph(
            &[datum(host, cores, Value::Int(4), TS2, Op::Retract)],
            TS2,
            None,
            None,
            g,
        )
        .unwrap();
    g
}

/// Positive control and the core claim: a cut at the head equals the source,
/// and tailing after later writes keeps it equal.
#[test]
fn a_cut_then_the_tail_equals_the_source() {
    let mut source = Store::open_in_memory().unwrap();
    let g = fixture(&mut source);
    let spool = tempfile::tempdir().unwrap();
    let cut = cut(
        &source,
        &request(&source, None),
        spool.path(),
        &Limits::default(),
    )
    .unwrap();
    let replica = bootstrap(&cut);
    assert_equal(&source, &replica, "at T");

    let host = source.intern("http://example.org/korimako").unwrap();
    let lease = source.intern("http://example.org/lease").unwrap();
    let mem = source.intern("http://example.org/memGiB").unwrap();
    source
        .transact_to_graph(
            &[
                datum(host, mem, Value::Int(32), TS3, Op::Assert),
                datum(
                    host,
                    lease,
                    Value::Str("acme".into()),
                    "2025-01-01T00:00:00Z",
                    Op::Retract,
                ),
            ],
            TS3,
            None,
            None,
            g,
        )
        .unwrap();
    let replica = replay_onto(&source, cut.manifest.watermark_tx, replica);
    assert_equal(&source, &replica, "after the tail");
}

/// A cut pinned before a retract carries the fact as live; the tail then
/// delivers the retract once.
#[test]
fn a_cut_pinned_before_a_retract_carries_the_fact_and_the_tail_closes_it() {
    let mut source = Store::open_in_memory().unwrap();
    fixture(&mut source);
    let before_retract = source.latest_tx_id().unwrap() - 1;
    let spool = tempfile::tempdir().unwrap();
    let cut = cut(
        &source,
        &request(&source, Some(before_retract)),
        spool.path(),
        &Limits::default(),
    )
    .unwrap();
    assert_eq!(cut.manifest.watermark_tx, before_retract);
    let cores: Vec<_> = spooled_records(&cut)
        .into_iter()
        .filter(|r| r["attribute"] == "http://example.org/cpuCores")
        .collect();
    assert_eq!(cores.len(), 1, "the later-retracted fact is live at T");
    assert!(cores[0]["valid_to"].is_null(), "{cores:?}");
    let replica = replay_onto(&source, cut.manifest.watermark_tx, bootstrap(&cut));
    assert_equal(&source, &replica, "after the tail");
}

fn file_store(dir: &Path) -> (Store, String) {
    let path = dir.join("source.db").to_str().unwrap().to_string();
    (Store::open(&path).unwrap(), path)
}

/// A write committed while the cut's read transaction is open is not in the
/// cut and arrives exactly once in the tail.
#[test]
fn a_write_during_the_cut_arrives_once_in_the_tail() {
    let dir = tempfile::tempdir().unwrap();
    let (mut source, path) = file_store(dir.path());
    fixture(&mut source);
    let spool = tempfile::tempdir().unwrap();
    let mut wrote = None;
    let cut = cut_with_control(
        &source,
        &request(&source, None),
        spool.path(),
        &Limits::default(),
        |seen| {
            if seen == 0 && wrote.is_none() {
                let mut writer = Store::open(&path).unwrap();
                let g = writer.graph_create(G).unwrap();
                let host = writer.intern("http://example.org/korimako").unwrap();
                let disk = writer.intern("http://example.org/diskGiB").unwrap();
                wrote = Some(
                    writer
                        .transact_to_graph(
                            &[datum(host, disk, Value::Int(512), TS3, Op::Assert)],
                            TS3,
                            None,
                            None,
                            g,
                        )
                        .unwrap(),
                );
            }
            Ok(())
        },
    )
    .unwrap();
    let wrote = wrote.expect("the hook wrote during the cut");
    assert!(cut.manifest.watermark_tx < wrote, "{:?}", cut.manifest);
    assert!(
        !spooled_records(&cut)
            .iter()
            .any(|r| r["attribute"] == "http://example.org/diskGiB"),
        "a write after T leaked into the cut"
    );
    let replica = replay_onto(&source, cut.manifest.watermark_tx, bootstrap(&cut));
    assert_equal(&source, &replica, "after the tail");
    let disk = live_digest(&replica)
        .into_iter()
        .filter(|f| f.contains("diskGiB"))
        .count();
    assert_eq!(disk, 1, "the concurrent write arrived exactly once");
}

/// A cut larger than one part arrives as numbered parts whose running
/// digests chain to the final digest, and reassembles to the same replica.
#[test]
fn a_cut_over_the_part_budget_splits_and_reassembles() {
    let mut source = Store::open_in_memory().unwrap();
    fixture(&mut source);
    let spool = tempfile::tempdir().unwrap();
    let limits = Limits {
        part_max_len: 400,
        ..Limits::default()
    };
    let cut = cut(&source, &request(&source, None), spool.path(), &limits).unwrap();
    assert!(cut.parts.parts > 1, "{:?}", cut.parts);
    assert_equal(&source, &bootstrap(&cut), "reassembled");
}

/// The manifest names the source, scope and per-graph totals, and its hash
/// names the spool.
#[test]
fn the_manifest_binds_source_scope_and_counts() {
    let mut source = Store::open_in_memory().unwrap();
    fixture(&mut source);
    let spool = tempfile::tempdir().unwrap();
    let cut = cut(
        &source,
        &request(&source, None),
        spool.path(),
        &Limits::default(),
    )
    .unwrap();
    let m = &cut.manifest;
    assert_eq!(m.schema, "quipu-asof-share/v1");
    assert_eq!(m.source.store_id, source.store_id().unwrap());
    assert_eq!(m.source.log_epoch, source.log_epoch().unwrap());
    assert_eq!(m.watermark_tx, source.latest_tx_id().unwrap());
    assert_eq!(m.scope, vec![G.to_string()]);
    assert!(m.completeness.valid_time);
    assert_eq!(m.completeness.bytes, "lossless");
    // Four live facts: cpuCores was retracted.
    assert_eq!(m.completeness.graphs[G].records, 4);
    assert!(
        cut.spool
            .to_str()
            .unwrap()
            .contains(cut.manifest_sha256.trim_start_matches("sha256:"))
    );
    // The same source at the same T cuts identically.
    let again = cut_again(&source, spool.path());
    assert_eq!(again.manifest_sha256, cut.manifest_sha256);
    assert_eq!(again.parts.digest, cut.parts.digest);
}

fn cut_again(source: &Store, spool: &Path) -> Cut {
    cut(source, &request(source, None), spool, &Limits::default()).unwrap()
}

fn aborted(result: Result<Cut, CutError>) -> AbortReason {
    match result {
        Err(CutError::Aborted(reason)) => reason,
        other => panic!("expected an abort, got {other:?}"),
    }
}

fn spool_is_empty(dir: &Path) -> bool {
    std::fs::read_dir(dir).unwrap().next().is_none()
}

#[test]
fn a_time_ceiling_aborts_the_cut_and_leaves_no_spool() {
    let mut source = Store::open_in_memory().unwrap();
    fixture(&mut source);
    let spool = tempfile::tempdir().unwrap();
    let limits = Limits {
        read_timeout: std::time::Duration::ZERO,
        ..Limits::default()
    };
    let reason = aborted(cut(&source, &request(&source, None), spool.path(), &limits));
    assert_eq!(reason, AbortReason::TimeCeiling);
    assert!(spool_is_empty(spool.path()));
}

/// WAL growth past the ceiling aborts the cut; afterwards the read
/// transaction is closed, so a checkpoint can complete.
#[test]
fn a_wal_ceiling_aborts_the_cut_and_releases_the_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let (mut source, path) = file_store(dir.path());
    fixture(&mut source);
    let spool = tempfile::tempdir().unwrap();
    let limits = Limits {
        wal_growth_max_bytes: 0,
        ..Limits::default()
    };
    let reason = aborted(cut_with_control(
        &source,
        &request(&source, None),
        spool.path(),
        &limits,
        |seen| {
            if seen == 0 {
                let mut writer = Store::open(&path).unwrap();
                let host = writer.intern("http://example.org/korimako").unwrap();
                let disk = writer.intern("http://example.org/diskGiB").unwrap();
                writer
                    .transact(
                        &[datum(host, disk, Value::Int(512), TS3, Op::Assert)],
                        TS3,
                        None,
                        None,
                    )
                    .unwrap();
            }
            Ok(())
        },
    ));
    assert_eq!(reason, AbortReason::WalCeiling);
    assert!(spool_is_empty(spool.path()));
    let (busy, _, _): (i64, i64, i64) = source
        .conn
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!(busy, 0, "a checkpoint is blocked after the abort");
}

#[test]
fn a_full_spool_quota_refuses_the_cut() {
    let mut source = Store::open_in_memory().unwrap();
    fixture(&mut source);
    let spool = tempfile::tempdir().unwrap();
    let limits = Limits {
        spool_quota_bytes: 1,
        ..Limits::default()
    };
    let reason = aborted(cut(&source, &request(&source, None), spool.path(), &limits));
    assert_eq!(reason, AbortReason::SpoolQuota);
    assert!(spool_is_empty(spool.path()));
}

#[test]
fn invalid_selections_are_refused() {
    let mut source = Store::open_in_memory().unwrap();
    fixture(&mut source);
    let spool = tempfile::tempdir().unwrap();
    let refuse = |request: Request| {
        assert!(
            matches!(
                cut(&source, &request, spool.path(), &Limits::default()),
                Err(CutError::Refused(_))
            ),
            "{request:?}"
        );
    };
    let mut r = request(&source, None);
    r.graphs = Vec::new();
    refuse(r);
    let mut r = request(&source, None);
    r.graphs = vec![G.into(), G.into()];
    refuse(r);
    let mut r = request(&source, None);
    r.graphs = vec!["http://example.org/graph/unregistered".into()];
    refuse(r);
    let mut r = request(&source, None);
    r.context_graphs = vec![G.into()];
    refuse(r);
    let mut r = request(&source, None);
    r.expected_store_id = "urn:uuid:not-this-store".into();
    refuse(r);
    assert!(spool_is_empty(spool.path()));
}

/// The scan rides idx_geav and needs no sort, so it holds no temporary
/// B-tree however large the graph is.
#[test]
fn the_cut_scan_needs_no_sort() {
    let mut source = Store::open_in_memory().unwrap();
    fixture(&mut source);
    let mut stmt = source
        .conn
        .prepare(
            "EXPLAIN QUERY PLAN SELECT e, a, v, tx, valid_from, \
                    CASE WHEN retracted_tx IS NULL THEN valid_to END \
             FROM facts INDEXED BY idx_geav \
             WHERE g = ?1 AND op = 1 AND tx <= ?2 \
               AND (retracted_tx IS NULL OR retracted_tx > ?2) \
             ORDER BY e, a, v",
        )
        .unwrap();
    let plan: Vec<String> = stmt
        .query_map([1, 1], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        !plan.iter().any(|step| step.contains("TEMP B-TREE")),
        "{plan:?}"
    );
}

/// A cut pinned at an earlier `T` leaves out an assert committed after `T`;
/// the tail delivers it once.
#[test]
fn a_pinned_cut_leaves_out_later_asserts_for_the_tail() {
    let mut source = Store::open_in_memory().unwrap();
    let g = fixture(&mut source);
    let pinned = source.latest_tx_id().unwrap();
    let host = source.intern("http://example.org/korimako").unwrap();
    let mem = source.intern("http://example.org/memGiB").unwrap();
    source
        .transact_to_graph(
            &[datum(host, mem, Value::Int(32), TS3, Op::Assert)],
            TS3,
            None,
            None,
            g,
        )
        .unwrap();
    let spool = tempfile::tempdir().unwrap();
    let cut = cut(
        &source,
        &request(&source, Some(pinned)),
        spool.path(),
        &Limits::default(),
    )
    .unwrap();
    assert!(
        !spooled_records(&cut)
            .iter()
            .any(|r| r["attribute"] == "http://example.org/memGiB"),
        "an assert after T is in the cut"
    );
    let replica = replay_onto(&source, cut.manifest.watermark_tx, bootstrap(&cut));
    assert_equal(&source, &replica, "after the tail");
}
