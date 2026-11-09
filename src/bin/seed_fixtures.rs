//! Seed binary — generates test-fixtures/test-store.db from static assets.
//!
//! Run with: `cargo run --bin seed-fixtures --features shacl`
//! Or via:   `just seed --base-ns https://example.org/fixtures/`
//! With no flag, the namespace comes from the project configuration.

use quipu::episode::{Episode, ingest_episode};
use quipu::namespace::DEFAULT_BASE_NS;
use quipu::rdf::ingest_rdf;
use quipu::store::{Datum, Store};
use quipu::types::{Op, Value};

use oxrdfio::RdfFormat;
use std::fs;
use std::path::Path;

const DB_PATH: &str = "test-fixtures/test-store.db";
const SHAPES_PATH: &str = "test-fixtures/test-shapes.ttl";
const EPISODES_PATH: &str = "test-fixtures/test-episodes.json";
const INFRA_PATH: &str = "test-fixtures/seed-infra.ttl";
const AGENTS_PATH: &str = "test-fixtures/seed-agents.ttl";
const KNOWLEDGE_PATH: &str = "test-fixtures/seed-knowledge.ttl";

fn main() {
    if let Err(e) = run() {
        eprintln!("seed-fixtures failed: {e}");
        std::process::exit(1);
    }
}

fn run() -> quipu::Result<()> {
    let mut args = std::env::args().skip(1);
    let namespace = match args.next().as_deref() {
        None => quipu::QuipuConfig::load_strict(Path::new("."))?.base_ns,
        Some("--base-ns") => args
            .next()
            .ok_or_else(|| quipu::Error::InvalidValue("--base-ns requires a namespace".into()))?,
        Some(_) => {
            return Err(quipu::Error::InvalidValue(
                "usage: seed-fixtures [--base-ns IRI]".into(),
            ));
        }
    };
    if args.next().is_some() {
        return Err(quipu::Error::InvalidValue(
            "unexpected seed-fixtures argument".into(),
        ));
    }
    quipu::namespace::require_base_ns(&namespace)?;
    seed(&namespace)
}

fn seed(ns: &str) -> quipu::Result<()> {
    // 1. Delete existing DB.
    if Path::new(DB_PATH).exists() {
        fs::remove_file(DB_PATH).expect("failed to remove existing DB");
        println!("  removed existing {DB_PATH}");
    }

    // 2. Open file-backed store.
    let mut store = Store::open(DB_PATH)?;
    store.set_base_ns(ns);
    println!("  opened {DB_PATH}");

    // 3. Infrastructure subgraph (~30 entities).
    let (tx, n) = ingest_ttl(
        &mut store,
        INFRA_PATH,
        "2026-01-15T10:00:00Z",
        "fixture:infra",
        ns,
    )?;
    println!("  infra:      tx={tx}  triples={n}");

    // 4. Agent platform subgraph (~15 entities).
    let (tx, n) = ingest_ttl(
        &mut store,
        AGENTS_PATH,
        "2026-02-01T10:00:00Z",
        "fixture:agents",
        ns,
    )?;
    println!("  agents:     tx={tx}  triples={n}");

    // 5. Knowledge subgraph (~10 entities).
    let (tx, n) = ingest_ttl(
        &mut store,
        KNOWLEDGE_PATH,
        "2026-02-15T10:00:00Z",
        "fixture:knowledge",
        ns,
    )?;
    println!("  knowledge:  tx={tx}  triples={n}");

    // 6. Temporal mutations.
    temporal_mutations(&mut store, ns)?;

    // 7. Ingest episodes from JSON.
    let episodes_json = fs::read_to_string(EPISODES_PATH)
        .unwrap_or_else(|e| panic!("failed to read {EPISODES_PATH}: {e}"));
    let episodes: Vec<Episode> = serde_json::from_str(&episodes_json)
        .unwrap_or_else(|e| panic!("failed to parse {EPISODES_PATH}: {e}"));

    let timestamps = [
        "2026-01-20T10:00:00Z",
        "2026-03-27T09:00:00Z",
        "2026-03-29T14:00:00Z",
        "2026-02-05T10:00:00Z",
        "2026-04-01T12:00:00Z",
        "2026-04-05T16:00:00Z",
        "2026-04-10T09:00:00Z",
    ];
    for (i, ep) in episodes.iter().enumerate() {
        let ts = timestamps.get(i).copied().unwrap_or("2026-04-12T00:00:00Z");
        let (tx, n) = ingest_episode(&mut store, ep, ts, ns)?;
        println!("  episode[{}]: tx={tx}  triples={n}  name={}", i, ep.name);
    }

    // 8. Load SHACL shapes.
    let shapes = fs::read_to_string(SHAPES_PATH)
        .unwrap_or_else(|e| panic!("failed to read {SHAPES_PATH}: {e}"));
    store.load_shapes(
        "test-fixtures",
        &fixture_namespace(&shapes, ns),
        "2026-04-12T00:00:00Z",
    )?;
    println!("  shapes:     loaded from {SHAPES_PATH}");

    // 9. Layout seed metadata.
    let meta_entity = store.intern(&format!("{ns}__layout_metadata"))?;
    let seed_attr = store.intern(&format!("{ns}layoutSeed"))?;
    store.transact(
        &[Datum {
            entity: meta_entity,
            attribute: seed_attr,
            value: Value::Int(42),
            valid_from: "2026-04-12T00:00:00Z".into(),
            valid_to: None,
            op: Op::Assert,
        }],
        "2026-04-12T00:00:00Z",
        Some("seed"),
        Some("fixture:metadata"),
    )?;
    println!("  metadata:   layoutSeed=42");

    // 10. Verification.
    verify(&store, ns)?;

    println!("\nseed-fixtures complete: {DB_PATH}");
    Ok(())
}

// ── Temporal mutations ────────────────────────────────────────────

fn temporal_mutations(store: &mut Store, ns: &str) -> quipu::Result<()> {
    let status_attr = store.intern(&format!("{ns}status"))?;

    // korimako: online (01-15) -> down (03-27T09:00)
    let korimako = store
        .lookup(&format!("{ns}korimako"))?
        .expect("korimako not found");
    store.retract_entity(
        korimako,
        Some(status_attr),
        "2026-03-27T09:00:00Z",
        Some("seed"),
    )?;
    store.transact(
        &[status_datum(
            korimako,
            status_attr,
            "down",
            "2026-03-27T09:00:00Z",
        )],
        "2026-03-27T09:00:00Z",
        Some("seed"),
        Some("fixture:temporal"),
    )?;

    // korimako: down -> online (03-29T14:00)
    store.retract_entity(
        korimako,
        Some(status_attr),
        "2026-03-29T14:00:00Z",
        Some("seed"),
    )?;
    store.transact(
        &[status_datum(
            korimako,
            status_attr,
            "online",
            "2026-03-29T14:00:00Z",
        )],
        "2026-03-29T14:00:00Z",
        Some("seed"),
        Some("fixture:temporal"),
    )?;

    // postgresql: running -> crashed (03-27T09:15)
    let pg = store
        .lookup(&format!("{ns}postgresql"))?
        .expect("postgresql not found");
    store.retract_entity(pg, Some(status_attr), "2026-03-27T09:15:00Z", Some("seed"))?;
    store.transact(
        &[status_datum(
            pg,
            status_attr,
            "crashed",
            "2026-03-27T09:15:00Z",
        )],
        "2026-03-27T09:15:00Z",
        Some("seed"),
        Some("fixture:temporal"),
    )?;

    // postgresql: crashed -> running (03-27T11:00)
    store.retract_entity(pg, Some(status_attr), "2026-03-27T11:00:00Z", Some("seed"))?;
    store.transact(
        &[status_datum(
            pg,
            status_attr,
            "running",
            "2026-03-27T11:00:00Z",
        )],
        "2026-03-27T11:00:00Z",
        Some("seed"),
        Some("fixture:temporal"),
    )?;

    // traefik: healthy -> degraded (03-27T09:05)
    let traefik = store
        .lookup(&format!("{ns}traefik"))?
        .expect("traefik not found");
    store.retract_entity(
        traefik,
        Some(status_attr),
        "2026-03-27T09:05:00Z",
        Some("seed"),
    )?;
    store.transact(
        &[status_datum(
            traefik,
            status_attr,
            "degraded",
            "2026-03-27T09:05:00Z",
        )],
        "2026-03-27T09:05:00Z",
        Some("seed"),
        Some("fixture:temporal"),
    )?;

    // traefik: degraded -> healthy (03-29T14:30)
    store.retract_entity(
        traefik,
        Some(status_attr),
        "2026-03-29T14:30:00Z",
        Some("seed"),
    )?;
    store.transact(
        &[status_datum(
            traefik,
            status_attr,
            "healthy",
            "2026-03-29T14:30:00Z",
        )],
        "2026-03-29T14:30:00Z",
        Some("seed"),
        Some("fixture:temporal"),
    )?;

    println!("  temporal:   6 mutations (korimako, postgresql, traefik)");
    Ok(())
}

fn status_datum(entity: i64, attribute: i64, value: &str, valid_from: &str) -> Datum {
    Datum {
        entity,
        attribute,
        value: Value::Str(value.into()),
        valid_from: valid_from.into(),
        valid_to: None,
        op: Op::Assert,
    }
}

// ── Verification ──────────────────────────────────────────────────

fn verify(store: &Store, ns: &str) -> quipu::Result<()> {
    // Count entities via SPARQL.
    let result = quipu::sparql_query(
        store,
        "SELECT (COUNT(DISTINCT ?s) AS ?c) WHERE { ?s ?p ?o }",
    )?;
    let entity_count = count_from_row(result.rows().first(), "c");
    assert!(
        entity_count >= 50,
        "expected 50+ entities, found {entity_count}"
    );
    println!("\n  verify: {entity_count} distinct subjects (>= 50)");

    // korimako edge count.
    let koror_id = store
        .lookup(&format!("{ns}korimako"))?
        .expect("korimako not found");
    let koror_facts = store.entity_facts(koror_id)?;
    let outgoing: i64 = koror_facts
        .iter()
        .filter(|f| matches!(f.value, Value::Ref(_)))
        .count()
        .try_into()
        .unwrap_or(0);

    // Count incoming edges via SPARQL.
    let incoming_q = format!("SELECT (COUNT(?s) AS ?c) WHERE {{ ?s ?p <{ns}korimako> }}");
    let incoming_result = quipu::sparql_query(store, &incoming_q)?;
    let incoming = count_from_row(incoming_result.rows().first(), "c");
    let total_edges = outgoing + incoming;
    assert!(
        total_edges >= 8,
        "korimako expected 8+ edges, found {total_edges} ({outgoing} out + {incoming} in)"
    );
    println!("  verify: korimako has {total_edges} edges ({outgoing} out + {incoming} in) (>= 8)");

    // korimako status history.
    let status_attr = store.lookup(&format!("{ns}status"))?.expect("status attr");
    let koror_history = store.attribute_history(koror_id, status_attr)?;
    assert!(
        koror_history.len() >= 5,
        "korimako status history expected 5+ entries, found {}",
        koror_history.len()
    );
    println!(
        "  verify: korimako status history has {} entries (>= 5)",
        koror_history.len()
    );

    // Episode transactions: check that episode provenance entities exist.
    let ep_q = "SELECT (COUNT(?e) AS ?c) WHERE { ?e a <http://www.w3.org/ns/prov#Activity> }";
    let ep_result = quipu::sparql_query(store, ep_q)?;
    let ep_count = count_from_row(ep_result.rows().first(), "c");
    assert!(
        ep_count >= 7,
        "expected 7+ episode transactions, found {ep_count}"
    );
    println!("  verify: {ep_count} episode transactions (>= 7)");

    // Shapes loaded.
    let shapes = store.list_shapes()?;
    assert!(!shapes.is_empty(), "shapes not loaded");
    println!("  verify: shapes loaded ({})", shapes.len());

    Ok(())
}

fn count_from_row(row: Option<&quipu::sparql::Bindings>, var: &str) -> i64 {
    row.and_then(|r| r.get(var))
        .and_then(|v| match v {
            Value::Int(n) => Some(*n),
            _ => None,
        })
        .unwrap_or(0)
}

fn ingest_ttl(
    store: &mut Store,
    path: &str,
    timestamp: &str,
    source: &str,
    ns: &str,
) -> quipu::Result<(i64, usize)> {
    let turtle = fs::read_to_string(path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"));
    ingest_rdf(
        store,
        fixture_namespace(&turtle, ns).as_bytes(),
        RdfFormat::Turtle,
        None,
        timestamp,
        Some("seed"),
        Some(source),
    )
}

// This renderer is only for the fixed development fixture assets, never a
// policy-pack loader or existing-store namespace migration.
fn fixture_namespace(turtle: &str, ns: &str) -> String {
    turtle.replace(DEFAULT_BASE_NS, ns)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_assets_use_one_namespace_and_legacy_bytes_are_unchanged() {
        for asset in [
            include_str!("../../test-fixtures/seed-infra.ttl"),
            include_str!("../../test-fixtures/seed-agents.ttl"),
            include_str!("../../test-fixtures/seed-knowledge.ttl"),
            include_str!("../../test-fixtures/test-shapes.ttl"),
        ] {
            assert!(asset.contains(DEFAULT_BASE_NS));
            assert_eq!(
                fixture_namespace(asset, DEFAULT_BASE_NS).as_bytes(),
                asset.as_bytes()
            );
            let rendered = fixture_namespace(asset, "https://example.org/fixture#");
            assert!(!rendered.contains(DEFAULT_BASE_NS));
            assert!(rendered.contains("https://example.org/fixture#"));
        }
    }
}
