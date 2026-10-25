//! Cost of each branch of the seeds owner-status discovery query (aegis-t9n).
//!
//!     cargo run --release --example t9n_branch_cost -- STORE.db BRANCHES.json [RUNS]
//!
//! Opens STORE.db with `Store::open_immutable`, so the file and its siblings are
//! never written. BRANCHES.json is a list of `{"name", "query"}` objects: the
//! whole discovery query and each of its leaves. Prints one JSON line per
//! branch with its row count and the elapsed time of every run, so a planner
//! change can be compared on the same store and the same text.

use quipu::Store;
use quipu::sparql::{QueryResult, query};
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let (Some(db), Some(branches)) = (args.get(1), args.get(2)) else {
        return Err("usage: t9n_branch_cost STORE.db BRANCHES.json [RUNS]".into());
    };
    let runs: usize = args.get(3).map_or(Ok(3), |r| r.parse())?;
    let branches: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(branches)?)?;
    let store = Store::open_immutable(db)?;
    for branch in branches.as_array().ok_or("BRANCHES.json is not a list")? {
        let name = branch["name"].as_str().ok_or("branch without a name")?;
        let text = branch["query"].as_str().ok_or("branch without a query")?;
        let mut elapsed = Vec::with_capacity(runs);
        let mut count = None;
        for _ in 0..runs {
            let start = Instant::now();
            let result = query(&store, text)?;
            elapsed.push(start.elapsed().as_secs_f64());
            let QueryResult::Select { rows, .. } = result else {
                return Err(format!("{name}: not a SELECT").into());
            };
            if count.is_some_and(|c| c != rows.len()) {
                return Err(format!("{name}: row count changed between runs").into());
            }
            count = Some(rows.len());
        }
        println!(
            "{}",
            serde_json::json!({"name": name, "rows": count, "elapsed_s": elapsed})
        );
    }
    Ok(())
}
