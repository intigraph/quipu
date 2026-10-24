//! Cost of a declared board projection on a SCRATCH copy of a store.
//!
//!     cargo run --release --example projection_lab -- COPY.db GRAPH
//!
//! WRITES to COPY.db: never point it at a served store. Times a batch of
//! synthetic writes to GRAPH, installs the projection, times its build, times
//! the same batch again with the projection's triggers in place, then times the
//! aggregates a brief would issue. Prints one JSON line per step.

use quipu::Store;
use quipu::config::{ProjectionColumnConfig, ProjectionConfig};
use quipu::store::{Datum, ProjectionAggregate};
use quipu::types::{Op, Value};
use serde_json::json;
use std::time::Instant;

const SEEDS: &str = "https://seeds.local/ontology/";
const SCHEMA: &str = "https://schema.org/";

fn col(name: &str, predicate: &str, time: bool) -> ProjectionColumnConfig {
    ProjectionColumnConfig {
        name: name.into(),
        predicate: predicate.into(),
        time,
    }
}

/// `n` single-datum transactions touching status on synthetic items, the
/// shape of the board's busiest write.
fn batch(
    store: &mut Store,
    graph: i64,
    tag: &str,
    n: usize,
) -> Result<Vec<f64>, Box<dyn std::error::Error>> {
    let status = store.intern(&format!("{SEEDS}status"))?;
    let mut ms = Vec::with_capacity(n);
    for i in 0..n {
        let d = Datum {
            entity: store.intern(&format!("https://seeds.local/item/lab-{tag}-{i}"))?,
            attribute: status,
            value: Value::Str("open".into()),
            valid_from: "2026-10-24T00:00:00Z".into(),
            valid_to: None,
            op: Op::Assert,
        };
        let t = Instant::now();
        store.transact_to_graph(&[d], "2026-10-24T00:00:00Z", Some("lab"), None, graph)?;
        ms.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    ms.sort_by(f64::total_cmp);
    Ok(ms)
}

fn pct(ms: &[f64], p: f64) -> f64 {
    ms[((ms.len() - 1) as f64 * p) as usize]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: projection_lab COPY.db GRAPH")?;
    let graph_iri = args.next().ok_or("usage: projection_lab COPY.db GRAPH")?;
    let mut store = Store::open(&path)?;
    let graph = store
        .lookup(&graph_iri)?
        .ok_or("graph not in the dictionary")?;

    let before = batch(&mut store, graph, "before", 200)?;
    println!(
        "{}",
        json!({"step": "writes_without_projection", "n": before.len(), "p50_ms": pct(&before, 0.5), "p99_ms": pct(&before, 0.99)})
    );

    let def = ProjectionConfig {
        name: "board_items".into(),
        key: format!("{SEEDS}status"),
        columns: vec![
            col("priority", &format!("{SEEDS}priority"), false),
            col("created", &format!("{SCHEMA}dateCreated"), true),
            col("closed", &format!("{SCHEMA}endTime"), true),
            col("identifier", &format!("{SCHEMA}identifier"), false),
            col("title", &format!("{SCHEMA}name"), false),
            col("keywords", &format!("{SCHEMA}keywords"), false),
        ],
    };
    let t = Instant::now();
    let status = store.install_projections(std::slice::from_ref(&def))?;
    println!(
        "{}",
        json!({"step": "install", "ms": t.elapsed().as_millis(), "rows": status.iter().map(|s| s.rows).sum::<i64>()})
    );

    let after = batch(&mut store, graph, "after", 200)?;
    println!(
        "{}",
        json!({"step": "writes_with_projection", "n": after.len(), "p50_ms": pct(&after, 0.5), "p99_ms": pct(&after, 0.99)})
    );

    let active: Vec<serde_json::Value> = ["open", "hooked", "in_progress", "blocked", "deferred"]
        .iter()
        .map(|s| json!(s))
        .collect();
    let since = ("closed", "2026-10-01T00:00:00Z");
    let active_filter: [(&str, &[serde_json::Value]); 1] = [("key", &active)];
    let cases: Vec<(&str, ProjectionAggregate<'_>)> = vec![
        (
            "active_by_status",
            ProjectionAggregate {
                graph: &graph_iri,
                filters: &active_filter,
                group_by: Some("key"),
                max_groups: 100,
                ..Default::default()
            },
        ),
        (
            "closed_since_by_priority",
            ProjectionAggregate {
                graph: &graph_iri,
                since: Some(since),
                group_by: Some("priority"),
                max_groups: 100,
                ..Default::default()
            },
        ),
        (
            "all_by_status",
            ProjectionAggregate {
                graph: &graph_iri,
                group_by: Some("key"),
                max_groups: 100,
                ..Default::default()
            },
        ),
    ];
    for (name, q) in cases {
        let t = Instant::now();
        let answer = store.aggregate_projection(&def, &q);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        match answer {
            Ok(a) => println!(
                "{}",
                json!({"step": name, "ms": ms, "total": a["total"], "groups": a["groups"].as_array().map_or(0, Vec::len)})
            ),
            Err(e) => println!(
                "{}",
                json!({"step": name, "ms": ms, "error": e.to_string()})
            ),
        }
    }
    Ok(())
}
