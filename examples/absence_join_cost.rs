//! Cost of a correlated FILTER NOT EXISTS on a real board (aegis-xdvg.3).
//!
//!     cargo run --release --example absence_join_cost -- STORE.db [GRAPH]
//!
//! Opens STORE.db with `Store::open_immutable`, so the file and its siblings are
//! never written. Picks up to 250 open or in-progress work items from GRAPH
//! (default: the seeds board graph), rebuilds the plate's unblocked-items query
//! in the shape it had before the client worked around its cost (one VALUES
//! batch per status and graph, each with a correlated NOT EXISTS over active
//! blockers), and times it with the in-memory read model on and off. Prints one
//! JSON line per run with the row count, so both paths can be compared.

use quipu::sparql::{QueryResult, query};
use quipu::{Store, Value};
use std::time::Instant;

const SEEDS: &str = "https://seeds.local/";
const SCHEMA: &str = "https://schema.org/";
const BLOCKED_ON: &str = "https://scbrown.github.io/quechua/ns#blockedOn";
const DEFAULT_GRAPH: &str = "https://seeds.local/project/aegis";
const STATES: [&str; 2] = ["open", "in_progress"];
const RUNS: usize = 3;

fn text(store: &Store, value: &Value) -> String {
    match value {
        Value::Ref(id) => store.resolve(*id).unwrap_or_default(),
        Value::Str(s) => s.clone(),
        other => format!("{other:?}"),
    }
}

fn rows(
    store: &Store,
    result: QueryResult,
) -> Result<Vec<Vec<String>>, Box<dyn std::error::Error>> {
    match result {
        QueryResult::Select { variables, rows } => Ok(rows
            .into_iter()
            .map(|row| {
                variables
                    .iter()
                    .map(|v| {
                        row.get(v)
                            .map(|value| text(store, value))
                            .unwrap_or_default()
                    })
                    .collect()
            })
            .collect()),
        _ => Err("expected a SELECT result".into()),
    }
}

fn unblocked_query(graph: &str, candidates: &[(String, String)]) -> String {
    let graphs = [graph.to_string(), format!("{graph}#seeds-ephemeral")];
    let mut branches = Vec::new();
    for g in &graphs {
        for state in STATES {
            let bindings: Vec<String> = candidates
                .iter()
                .filter(|(_, s)| s == state)
                .map(|(iri, _)| format!("<{iri}>"))
                .collect();
            if bindings.is_empty() {
                continue;
            }
            branches.push(format!(
                "{{ GRAPH <{g}> {{ VALUES ?s {{ {} }} ?s <{SEEDS}ontology/status> \"{state}\" ; \
                 a <{SCHEMA}Action> ; <{SCHEMA}identifier> ?id . \
                 FILTER NOT EXISTS {{ ?s <{BLOCKED_ON}> ?blocker . \
                 ?blocker <{SEEDS}ontology/status> ?bs . \
                 FILTER(?bs != \"closed\" && ?bs != \"tombstone\") }} }} }}",
                bindings.join(" ")
            ));
        }
    }
    format!(
        "SELECT DISTINCT ?id WHERE {{ {} }} LIMIT 1000",
        branches.join(" UNION ")
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: absence_join_cost STORE.db [GRAPH]")?;
    let graph = std::env::args()
        .nth(2)
        .unwrap_or_else(|| DEFAULT_GRAPH.to_string());
    let mut store = Store::open_immutable(&path)?;
    // A one-off measurement, not a served query: model build time is reported
    // separately below rather than failing the run at the served 30 s budget.
    store.search_config_mut().query_timeout_ms = 0;
    if let Some(max) = std::env::var("QUIPU_BENCH_MAX_TRIPLES")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        store.set_read_model_max_triples(max);
    }
    let gid = store
        .lookup(&graph)?
        .ok_or("graph not in the term dictionary")?;
    let started = Instant::now();
    let facts = store.current_fact_count(gid)?;
    println!(
        "{{\"graph_facts\":{facts},\"count_ms\":{:.1},\"budget\":{}}}",
        started.elapsed().as_secs_f64() * 1000.0,
        store.read_model_max_triples()
    );

    let pick = format!(
        "SELECT ?s ?status WHERE {{ GRAPH <{graph}> {{ ?s <{SEEDS}ontology/status> ?status ; \
         a <{SCHEMA}Action> . FILTER(?status = \"open\" || ?status = \"in_progress\") }} }} \
         ORDER BY ?s LIMIT 250"
    );
    let candidates: Vec<(String, String)> = rows(&store, query(&store, &pick)?)?
        .into_iter()
        .filter_map(|r| {
            let iri = r.first()?.clone();
            let status = r.get(1)?.clone();
            Some((iri, status))
        })
        .collect();
    if candidates.is_empty() {
        return Err(
            format!("no open or in-progress items in {graph}; wrong graph or vocabulary").into(),
        );
    }
    let sparql = unblocked_query(&graph, &candidates);
    println!(
        "{{\"candidates\":{},\"query_bytes\":{},\"graph\":\"{graph}\"}}",
        candidates.len(),
        sparql.len()
    );

    if store.read_model_affordable(gid) {
        let started = Instant::now();
        let held = store.read_model_for(gid)?.len();
        println!(
            "{{\"model_build_ms\":{:.1},\"model_triples\":{held}}}",
            started.elapsed().as_secs_f64() * 1000.0
        );
    }
    for read_model in [true, false] {
        store.set_read_model_enabled(read_model);
        for run in 0..RUNS {
            let started = Instant::now();
            let found = rows(&store, query(&store, &sparql)?)?.len();
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            println!(
                "{{\"read_model\":{read_model},\"run\":{run},\"rows\":{found},\"ms\":{ms:.1}}}"
            );
        }
    }
    Ok(())
}
