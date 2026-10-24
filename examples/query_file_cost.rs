//! Cost of one SPARQL query file against a store opened immutable.
//!
//!     cargo run --release --example query_file_cost -- STORE.db QUERY.rq [RUNS]
//!
//! Opens STORE.db with `Store::open_immutable`, so the file and its siblings are
//! never written. Runs the query RUNS times (default 3) with the in-memory read
//! model on and then off, and prints one JSON line per run with the elapsed time
//! and a digest of the sorted result rows, so both paths can be compared.
//! `QUIPU_BENCH_MAX_TRIPLES` overrides the read-model budget;
//! `QUIPU_BENCH_PROFILE=1` adds `SQLite` statement totals per run;
//! `QUIPU_BENCH_SHOW=1` prints the first ten result rows.

use quipu::Store;
use quipu::sparql::{QueryResult, query};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Instant;

fn digest(result: &QueryResult) -> (usize, u64) {
    let QueryResult::Select { variables, rows } = result else {
        return (0, 0);
    };
    let mut lines: Vec<String> = rows
        .iter()
        .map(|row| {
            variables
                .iter()
                .map(|v| row.get(v).map(|x| format!("{x:?}")).unwrap_or_default())
                .collect::<Vec<_>>()
                .join("\t")
        })
        .collect();
    lines.sort();
    let mut h = DefaultHasher::new();
    lines.hash(&mut h);
    (lines.len(), h.finish())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let usage = "usage: query_file_cost STORE.db QUERY.rq [RUNS]";
    let path = args.next().ok_or(usage)?;
    let sparql = std::fs::read_to_string(args.next().ok_or(usage)?)?;
    let runs: usize = args.next().map_or(Ok(3), |r| r.parse())?;
    let mut store = Store::open_immutable(&path)?;
    store.search_config_mut().query_timeout_ms = 0;
    if let Some(max) = std::env::var("QUIPU_BENCH_MAX_TRIPLES")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        store.set_read_model_max_triples(max);
    }
    let profile = std::env::var_os("QUIPU_BENCH_PROFILE").is_some();
    if profile {
        store.enable_statement_profile();
    }
    for model in [true, false] {
        store.set_read_model_enabled(model);
        for run in 0..runs {
            if profile {
                quipu::statement_profile::begin();
            }
            let started = Instant::now();
            let result = query(&store, &sparql)?;
            if let Some(t) = profile.then(quipu::statement_profile::take).flatten() {
                println!(
                    "{{\"statements\":{},\"rows\":{},\"sqlite_ms\":{},\"top\":{:?}}}",
                    t.statements,
                    t.rows,
                    t.nanos / 1_000_000,
                    t.top(3)
                );
            }
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            let (rows, hash) = digest(&result);
            if std::env::var_os("QUIPU_BENCH_SHOW").is_some()
                && let QueryResult::Select { variables, rows } = &result
            {
                for row in rows.iter().take(10) {
                    let cells: Vec<String> = variables
                        .iter()
                        .map(|v| row.get(v).map(|x| format!("{x:?}")).unwrap_or_default())
                        .collect();
                    println!("  {}", cells.join(" | "));
                }
            }
            println!(
                "{{\"read_model\":{model},\"run\":{run},\"rows\":{rows},\"digest\":\"{hash:016x}\",\"ms\":{ms:.1}}}"
            );
        }
    }
    Ok(())
}
