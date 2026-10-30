//! Offline cast-bound query profile: `just demo cast-filter`.
//! Two synthetic named graphs; no server or external database is opened.
use oxrdfio::RdfFormat;
use quipu::{Store, rdf::ingest_rdf_to_graph, sparql::query, statement_profile};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let count: usize = args.get(2).map_or(Ok(6000), |s| s.parse()).unwrap();
    let text = std::fs::read_to_string(&args[1]).unwrap();
    let mut store = Store::open_in_memory().unwrap();
    store.set_read_model_enabled(false);
    for (g, size) in [
        ("https://example.org/project/board", count),
        (
            "https://example.org/project/board#seeds-ephemeral",
            count / 4,
        ),
    ] {
        let graph = store.graph_create(g).unwrap();
        let mut data = String::new();
        for i in 0..size {
            let s = format!(
                "<http://example.org/item/{}/{i}>",
                if size == count { 0 } else { 1 }
            );
            let when = if i % 100 == 0 {
                "2026-10-29T12:00:00Z"
            } else {
                "2026-10-01T12:00:00Z"
            };
            for (p, o) in [
                (
                    "https://schema.org/endTime",
                    format!("\"{when}\"^^<http://www.w3.org/2001/XMLSchema#dateTime>"),
                ),
                (
                    "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
                    "<https://schema.org/Action>".into(),
                ),
                ("https://seeds.local/ontology/status", "\"closed\"".into()),
                (
                    "https://scbrown.github.io/quechua/ns#outcome",
                    "\"done\"".into(),
                ),
                ("https://schema.org/name", format!("\"item {i}\"")),
                ("https://schema.org/result", "\"proof retained\"".into()),
            ] {
                data.push_str(&format!("{s} <{p}> {o} .\n"));
            }
            if i % 200 == 0 {
                data.push_str(&format!("{s} <https://schema.org/keywords> \"inbox\" .\n"));
            }
        }
        ingest_rdf_to_graph(
            &mut store,
            data.as_bytes(),
            RdfFormat::NTriples,
            None,
            "2026-01-01T00:00:00Z",
            None,
            None,
            graph,
        )
        .unwrap();
    }
    store.enable_statement_profile();
    let parsed = spargebra::SparqlParser::new().parse_query(&text).unwrap();
    println!("ALGEBRA {parsed:#?}");
    let span = statement_profile::Span::arm();
    let start = Instant::now();
    let result = query(&store, &text);
    let elapsed = start.elapsed();
    let profile = span.finish().unwrap();
    println!(
        "MEASURE rows={:?} elapsed_ms={} statements={} sqlite_rows={} sqlite_ms={}",
        result.as_ref().map(|r| r.rows().len()),
        elapsed.as_millis(),
        profile.statements,
        profile.rows,
        profile.nanos / 1_000_000
    );
    for (sql, n, ns) in profile.top(12) {
        println!("SQL count={n} ms={} {sql}", ns / 1_000_000);
    }
    let mut rows: Vec<_> = result
        .unwrap()
        .rows()
        .iter()
        .map(|r| {
            let mut cells: Vec<_> = r.iter().collect();
            cells.sort_by_key(|x| x.0);
            format!("{cells:?}")
        })
        .collect();
    rows.sort();
    println!("ANSWER {rows:?}");
}
