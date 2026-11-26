//! aegis-oav5: local timing of a plain-literal object lookup under `as_of_tx`.
//! Ignored by default; run explicitly against a COPY of a large store, e.g.
//! `OAV5_DB=/path/copy.db cargo test --test oav5_asof_bench -- --ignored --nocapture`.
//! No network, no production state.
#![allow(clippy::unwrap_used, clippy::print_stdout)]

use quipu::Store;
use quipu::sparql::{TemporalContext, query_temporal};
use std::time::Instant;

const QUERIES: &[(&str, &str)] = &[
    (
        "one_triple_literal",
        "SELECT ?s WHERE { GRAPH <https://seeds.local/project/aegis> { ?s <https://schema.org/agent> \"malcolm\" } }",
    ),
    (
        "one_triple_status_open",
        "SELECT ?s WHERE { GRAPH <https://seeds.local/project/aegis> { ?s <https://seeds.local/ontology/status> \"open\" } } LIMIT 10000",
    ),
];

fn median_ms(store: &Store, q: &str, as_of_tx: Option<i64>) -> (f64, usize) {
    let ctx = TemporalContext {
        as_of_tx,
        ..TemporalContext::default()
    };
    let mut times = Vec::new();
    let mut rows = 0;
    for _ in 0..5 {
        let start = Instant::now();
        rows = query_temporal(store, q, &ctx).unwrap().rows().len();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    (times[2], rows)
}

#[test]
#[ignore = "local bench: needs OAV5_DB pointing at a store copy"]
fn oav5_asof_literal_bench() {
    let path = std::env::var("OAV5_DB").unwrap();
    let store = Store::open(&path).unwrap();
    let head = store.latest_tx_id().unwrap();
    for (name, q) in QUERIES {
        let (asof, asof_rows) = median_ms(&store, q, Some(head));
        let (none, none_rows) = median_ms(&store, q, None);
        assert_eq!(
            asof_rows, none_rows,
            "{name}: as_of=head must equal current"
        );
        println!("OAV5 {name} rows={none_rows} as_of_head_ms={asof:.2} none_ms={none:.2}");
    }
}
