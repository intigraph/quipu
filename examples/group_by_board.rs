//! GROUP BY cost on a seeds-shaped board (aegis-ek4).
//!
//!     cargo run --release --example group_by_board -- [ITEMS] [STORE.db]
//!
//! Builds (or reuses) a store with ITEMS work items (default 10,000) of about
//! twenty facts each in the seeds board graph, then times board aggregates:
//! plain and two-key GROUP BY, GROUP BY on an expression, the non-COUNT
//! aggregates, and the exact status/priority/assignee breakdown queries
//! `sd stats` sends. Prints one JSON line per query with the row count and a
//! digest of the sorted result, so two builds can be compared. No assertions:
//! it reports numbers. `QUIPU_READ_MODEL=1` enables the in-memory read model.

use quipu::sparql::QueryResult;
use quipu::{Store, Value};
use std::fmt::Write as _;
use std::time::Instant;

const GRAPH: &str = "https://seeds.local/project/aegis";
const SEEDS: &str = "https://seeds.local/ontology/";
const SCHEMA: &str = "https://schema.org/";
const PRINCIPAL: &str = "https://seeds.local/principal/";
const STATUSES: [&str; 6] = [
    "open",
    "in_progress",
    "blocked",
    "deferred",
    "closed",
    "tombstone",
];
const KINDS: [&str; 4] = ["task", "bug", "epic", "feature"];

fn fixture(n: usize) -> String {
    let mut ttl = String::new();
    for i in 0..n {
        let s = format!("<https://seeds.local/item/aegis-{i}>");
        let status = STATUSES[(i * 7) % STATUSES.len()];
        let kind = KINDS[(i * 3) % KINDS.len()];
        let day = 1 + (i % 28);
        let _ = write!(
            ttl,
            "{s} a <{SCHEMA}Action> ; <{SCHEMA}identifier> \"aegis-{i}\" ; \
             <{SEEDS}status> \"{status}\" ; <{SEEDS}issueType> \"{kind}\" ; \
             <{SCHEMA}name> \"Item {i} title\" ; \
             <http://www.w3.org/2000/01/rdf-schema#label> \"Item {i} title\" ; \
             <{SCHEMA}description> \"Description of work item {i}, long enough to be a real body.\" ; \
             <{SCHEMA}dateCreated> \"2026-10-{day:02}T10:00:00Z\"^^<http://www.w3.org/2001/XMLSchema#dateTime> ; \
             <{SCHEMA}dateModified> \"2026-10-{day:02}T12:00:00Z\"^^<http://www.w3.org/2001/XMLSchema#dateTime> ; \
             <{SCHEMA}creator> <{PRINCIPAL}crew{c}> ; \
             <{SCHEMA}keywords> \"area-{a}\" , \"lane-{l}\" ; \
             <{SEEDS}notes> \"notes {i}\" ; \
             <{SEEDS}revision> {i} ",
            c = i % 13,
            a = i % 11,
            l = i % 5,
        );
        // Most items carry a priority; some fall back to the default P2.
        if i % 10 != 0 {
            let _ = write!(ttl, "; <{SEEDS}priority> {} ", i % 5);
        }
        // About two thirds are assigned, across 40 principals.
        if i % 3 != 0 {
            let _ = write!(ttl, "; <{SCHEMA}agent> <{PRINCIPAL}agent{}> ", i % 40);
        }
        if status == "closed" {
            let _ = write!(
                ttl,
                "; <{SCHEMA}endTime> \"2026-10-{:02}T18:00:00Z\"^^<http://www.w3.org/2001/XMLSchema#dateTime> ",
                1 + ((i + 3) % 28)
            );
        }
        if i > 0 && i % 9 == 0 {
            let _ = write!(
                ttl,
                "; <{SCHEMA}isPartOf> <https://seeds.local/item/aegis-{}> ",
                i / 9
            );
        }
        ttl.push_str(".\n");
    }
    ttl
}

fn text(store: &Store, value: &Value) -> String {
    match value {
        Value::Ref(id) => store.resolve(*id).unwrap_or_default(),
        other => format!("{other:?}"),
    }
}

// Seeds' effective-cohort pattern, verbatim in shape (seeds summary.rs).
fn effective(fields: &str) -> String {
    let eph = format!("{GRAPH}#seeds-ephemeral");
    let base = format!("?s a <{SCHEMA}Action> ; <{SCHEMA}identifier> ?id .");
    format!(
        "{{ GRAPH <{GRAPH}> {{ {base} {fields} FILTER(isLiteral(?id) && sameTerm(?id, STR(?id))) }} \
         FILTER NOT EXISTS {{ GRAPH <{eph}> {{ ?shadow a <{SCHEMA}Action> ; <{SCHEMA}identifier> ?id }} }} }} \
         UNION {{ GRAPH <{eph}> {{ {base} {fields} FILTER(isLiteral(?id) && sameTerm(?id, STR(?id))) }} }}"
    )
}

fn status_fields() -> String {
    format!(
        "?s <{SEEDS}status> ?status . FILTER(isLiteral(?status) && sameTerm(?status, STR(?status)))"
    )
}

fn normalized(fields: &str) -> String {
    let missing = format!(
        "FILTER NOT EXISTS {{ ?s <{SEEDS}status> ?raw_status FILTER(isLiteral(?raw_status) && sameTerm(?raw_status, STR(?raw_status))) }} BIND(\"open\" AS ?status)"
    );
    format!(
        "{{ {} }} UNION {{ {} }}",
        effective(fields),
        effective(&fields.replace(&status_fields(), &missing))
    )
}

fn sd_grouped(fields: &str, key: &str) -> String {
    format!(
        "SELECT ?{key} (COUNT(DISTINCT ?id) AS ?n) WHERE {{ {} }} GROUP BY ?{key} LIMIT 10000",
        normalized(fields)
    )
}

fn queries() -> Vec<(&'static str, String)> {
    let g = |body: &str| format!("GRAPH <{GRAPH}> {{ {body} }}");
    let item =
        format!("?s a <{SCHEMA}Action> ; <{SCHEMA}identifier> ?id ; <{SEEDS}status> ?status .");
    let active = status_fields() + " FILTER(?status != \"tombstone\")";
    let guard = |v: &str| {
        format!(
            "sameTerm({v} - {v}, 0) && sameTerm({v}, {v} + 0) && {v} >= 0 && {v} <= 9223372036854775807"
        )
    };
    let principal = |v: &str| {
        format!(
            "(isIRI({v}) && STRSTARTS(STR({v}), \"{PRINCIPAL}\")) || (isLiteral({v}) && sameTerm({v}, STR({v})))"
        )
    };
    vec![
        (
            "status_count",
            format!(
                "SELECT ?status (COUNT(*) AS ?n) WHERE {{ {} }} GROUP BY ?status",
                g(&item)
            ),
        ),
        (
            "status_priority_count",
            format!(
                "SELECT ?status ?p (COUNT(*) AS ?n) WHERE {{ {} }} GROUP BY ?status ?p",
                g(&format!("{item} ?s <{SEEDS}priority> ?p ."))
            ),
        ),
        (
            "agent_expr_count",
            format!(
                "SELECT ?a (COUNT(?s) AS ?n) WHERE {{ {} }} GROUP BY (STR(?agent) AS ?a)",
                g(&format!("{item} ?s <{SCHEMA}agent> ?agent ."))
            ),
        ),
        (
            "status_sum_min_max",
            format!(
                "SELECT ?status (SUM(?p) AS ?sum) (MIN(?p) AS ?lo) (MAX(?p) AS ?hi) (SAMPLE(?id) AS ?one) WHERE {{ {} }} GROUP BY ?status",
                g(&format!("{item} ?s <{SEEDS}priority> ?p ."))
            ),
        ),
        (
            "status_group_concat",
            format!(
                "SELECT ?status (COUNT(DISTINCT ?p) AS ?np) (GROUP_CONCAT(DISTINCT ?p ; separator=\",\") AS ?ps) WHERE {{ {} }} GROUP BY ?status",
                g(&format!("{item} ?s <{SEEDS}priority> ?p ."))
            ),
        ),
        (
            "closed_day_bucket",
            format!(
                "SELECT ?d (COUNT(?s) AS ?n) WHERE {{ {} }} GROUP BY (SUBSTR(STR(?c), 1, 10) AS ?d)",
                g(&format!("?s <{SCHEMA}endTime> ?c ."))
            ),
        ),
        ("sd_stats_status", sd_grouped(&status_fields(), "status")),
        (
            "sd_stats_priority",
            sd_grouped(
                &(active.clone()
                    + &format!(
                        " OPTIONAL {{ ?s <{SEEDS}priority> ?raw_key . FILTER({} && ?raw_key <= 255) }} BIND(CONCAT(\"P\", STR(COALESCE(?raw_key, 2))) AS ?key)",
                        guard("?raw_key")
                    )),
                "key",
            ),
        ),
        (
            "sd_stats_assignee",
            sd_grouped(
                &(active
                    + &format!(
                        " OPTIONAL {{ ?s <{SCHEMA}agent> ?raw_key . FILTER({}) }} BIND(COALESCE(?raw_key, \"(unassigned)\") AS ?key)",
                        principal("?raw_key")
                    )),
                "key",
            ),
        ),
    ]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let n: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(10_000);
    let path = args
        .next()
        .unwrap_or_else(|| format!("group_by_board_{n}.db"));
    if !std::path::Path::new(&path).exists() {
        let mut store = Store::open(&path)?;
        let g = store.graph_create(GRAPH)?;
        let started = Instant::now();
        let (_, triples) = quipu::rdf::ingest_rdf_to_graph(
            &mut store,
            fixture(n).as_bytes(),
            oxrdfio::RdfFormat::Turtle,
            None,
            "2026-10-27T00:00:00Z",
            None,
            None,
            g,
        )?;
        eprintln!(
            "fixture: {n} items, {triples} triples, {:.1}s",
            started.elapsed().as_secs_f64()
        );
    }
    let mut store = Store::open(&path)?;
    store.search_config_mut().query_timeout_ms = 600_000;
    if std::env::var("QUIPU_READ_MODEL").is_ok() {
        store.set_read_model_enabled(true);
    }
    let only = std::env::var("ONLY").ok();
    for (name, sparql) in queries() {
        if only.as_deref().is_some_and(|o| !name.contains(o)) {
            continue;
        }
        let mut best = f64::MAX;
        let mut summary = String::new();
        for _ in 0..3 {
            let t = Instant::now();
            let result = quipu::sparql::query(&store, &sparql);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            best = best.min(ms);
            summary = match result {
                Ok(QueryResult::Select { variables, rows }) => {
                    let mut lines: Vec<String> = rows
                        .iter()
                        .map(|row| {
                            variables
                                .iter()
                                .map(|v| row.get(v).map(|x| text(&store, x)).unwrap_or_default())
                                .collect::<Vec<_>>()
                                .join("|")
                        })
                        .collect();
                    lines.sort();
                    let digest = lines.join("\n");
                    let hash = digest.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
                        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
                    });
                    format!("\"rows\":{},\"digest\":\"{hash:016x}\"", rows.len())
                }
                Ok(_) => "\"rows\":null".into(),
                Err(e) => format!("\"error\":{:?}", e.to_string()),
            };
        }
        println!("{{\"query\":\"{name}\",\"items\":{n},\"best_ms\":{best:.1},{summary}}}");
    }
    Ok(())
}
