//! Peak-RSS control for the applier's streamed bootstrap (aegis-s24.2).
//!
//! `cargo run --example applier_bootstrap_rss -- <records> <per_part> <dir>`
//!
//! Generates a synthetic one-graph as-of share of `<records>` facts and feeds
//! it to an empty file-backed replica in `<dir>` as begin / part / commit
//! frames. The producer side holds one part at a time as well: records are
//! generated in canonical-line order, so both digests stream in two passes.
//! Prints the outcome, the replica's fact count and this process's peak RSS
//! (`VmHWM`). Run it at two sizes: a streamed bootstrap's peak must not grow
//! with the share, where a buffered one grows with it.

use std::time::Instant;

use quipu::applier::{Applier, Outcome};
use quipu::store::Store;
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};

const T: &str = "2026-10-24T00:00:00Z";

/// Record `i`. Entity ids are zero-padded, and `attribute` and `entity` are
/// the first two canonical keys, so generation order IS sorted line order.
fn record(i: usize) -> Json {
    json!({
        "tx": 1, "sequence": i, "timestamp": T, "op": "assert", "graph": "ROOT",
        "entity": format!("urn:e:{i:010}"), "attribute": "urn:p:a",
        "value": format!("value {i:010} {}", "x".repeat(64)),
        "valid_from": T, "valid_to": null,
    })
}

/// Compact JSON with sorted keys: the share's canonical line.
fn canonical(v: &Json) -> String {
    match v {
        Json::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let body: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{}:{}", Json::String(k.clone()), canonical(&map[k])))
                .collect();
            format!("{{{}}}", body.join(","))
        }
        Json::Array(items) => format!(
            "[{}]",
            items.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        other => other.to_string(),
    }
}

fn part(index: usize, per_part: usize, n: usize, before: &str) -> String {
    let records: Vec<Json> = (index * per_part..((index + 1) * per_part).min(n))
        .map(record)
        .collect();
    json!({"source_tx": 1, "index": index, "digest_before": before, "records": records}).to_string()
}

/// One `/proc/self/status` field in KiB (0 if unreadable).
fn status_kib(field: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix(field))
        .and_then(|v| {
            v.trim_start_matches(':')
                .trim()
                .trim_end_matches("kB")
                .trim()
                .parse()
                .ok()
        })
        .unwrap_or(0)
}

fn vm_hwm_kib() -> u64 {
    status_kib("VmHWM")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(100_000);
    let per_part: usize = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(1_000);
    let dir = args.get(3).cloned().unwrap_or_else(|| ".".into());
    let parts = n.div_ceil(per_part);

    // Pass 1: the per-graph line digest and the part chain digest.
    let mut lines = Sha256::new();
    for i in 0..n {
        if i > 0 {
            lines.update(b"\n");
        }
        lines.update(canonical(&record(i)).as_bytes());
    }
    let mut chain = Sha256::new();
    for index in 0..parts {
        let before = hex::encode(chain.clone().finalize());
        chain.update(part(index, per_part, n, &before).as_bytes());
    }
    let digest = hex::encode(chain.finalize());
    let manifest = json!({
        "schema": "quipu-asof-share/v1",
        "source": {"store_id": "rss-source", "log_epoch": "rss-epoch"},
        "watermark_tx": 1,
        "completeness": {"valid_time": true, "bytes": "lossless", "graphs": {
            "ROOT": {"records": n, "digest": format!("sha256:{}", hex::encode(lines.finalize()))}
        }},
    });
    let after_producer = vm_hwm_kib();

    // Pass 2: the frames, one part in memory at a time.
    let path = format!("{dir}/replica-{n}.db");
    let _ = std::fs::remove_file(&path);
    let mut replica = Store::open(&path).expect("replica store");
    let mut applier = Applier::open(&replica).expect("applier");
    let started = Instant::now();
    let begin = json!({"manifest": manifest, "parts": parts, "records": n, "digest": digest});
    applier
        .apply_frame(&mut replica, "quipu.asof.begin", &begin.to_string(), 1)
        .expect("begin");
    let mut chain = Sha256::new();
    for index in 0..parts {
        let before = hex::encode(chain.clone().finalize());
        let data = part(index, per_part, n, &before);
        chain.update(data.as_bytes());
        let outcome = applier
            .apply_frame(&mut replica, "quipu.asof.part", &data, 1)
            .expect("part");
        assert!(matches!(outcome, Outcome::Buffered { .. }), "{outcome:?}");
    }
    let commit = json!({"source_tx": 1, "digest": digest}).to_string();
    let outcome = applier
        .apply_frame(&mut replica, "quipu.asof.commit", &commit, 2)
        .expect("commit");
    let ms = started.elapsed().as_millis();
    // Anonymous memory is the applier's own; file-backed pages are the
    // store's mmap of its database file, capped by mmap_size.
    let (anon, file) = (status_kib("RssAnon"), status_kib("RssFile"));
    drop(replica);
    let facts: i64 = rusqlite::Connection::open(&path)
        .and_then(|c| c.query_row("SELECT COUNT(*) FROM facts", [], |r| r.get(0)))
        .unwrap_or(-1);
    println!(
        "{}",
        json!({
            "records": n, "per_part": per_part, "outcome": format!("{outcome:?}"),
            "facts": facts, "ms": ms,
            "peak_rss_kib_after_producer": after_producer, "peak_rss_kib": vm_hwm_kib(), "rss_anon_kib": anon, "rss_file_kib": file,
        })
    );
    let _ = std::fs::remove_file(&path);
}
