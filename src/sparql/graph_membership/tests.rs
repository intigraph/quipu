use std::collections::HashSet;
use std::sync::Arc;

use oxrdfio::RdfFormat;
use spargebra::{Query, SparqlParser, algebra::GraphPattern};

use super::*;
use crate::rdf::ingest_rdf_to_graph;
use crate::sparql::{GraphScope, query, string_pushdown};

const EX: &str = "https://example.org/";

fn shape() -> (TriplePattern, TemporalContext) {
    let parsed = SparqlParser::new()
        .parse_query("SELECT ?s ?text WHERE { ?s <https://example.org/title> ?text FILTER(CONTAINS(LCASE(STR(?text)), \"seeds\")) }")
        .unwrap();
    let Query::Select { pattern, .. } = parsed else {
        unreachable!()
    };
    let GraphPattern::Project { inner, .. } = pattern else {
        unreachable!()
    };
    let GraphPattern::Filter { expr, inner } = *inner else {
        unreachable!()
    };
    let narrows = string_pushdown::collect(&expr, &inner, &Bindings::new());
    let GraphPattern::Bgp { patterns } = *inner else {
        unreachable!()
    };
    (
        patterns[0].clone(),
        TemporalContext {
            string_narrows: Some(Arc::new(narrows)),
            ..TemporalContext::default()
        },
    )
}

fn fixture() -> (Store, i64) {
    let mut store = Store::open_in_memory().unwrap();
    let graph = store.intern(&format!("{EX}g")).unwrap();
    let other = store.intern(&format!("{EX}other")).unwrap();
    for (gid, data) in [
        (
            graph,
            "<https://example.org/one> <https://example.org/title> \"SEEDS alpha\" .\n\
            <https://example.org/two> <https://example.org/title> \"seeds beta\"@en .\n\
            <https://example.org/three> <https://example.org/title> \"other\" .\n\
            <https://example.org/one> <https://example.org/type> <https://example.org/Action> .",
        ),
        (
            other,
            "<https://example.org/one> <https://example.org/title> \"seeds FOREIGN\" .\n\
            <https://example.org/foreign> <https://example.org/title> \"seeds FOREIGN ONLY\" .",
        ),
    ] {
        ingest_rdf_to_graph(
            &mut store,
            data.as_bytes(),
            RdfFormat::NTriples,
            None,
            "2026-01-01T00:00:00Z",
            None,
            None,
            gid,
        )
        .unwrap();
    }
    store
        .conn
        .execute_batch("DROP INDEX IF EXISTS idx_current_g; DROP INDEX IF EXISTS idx_current_aev;")
        .unwrap();
    (store, graph)
}

fn index(store: &Store) {
    store
        .conn
        .execute_batch(
            "CREATE INDEX idx_current_g ON facts(g) WHERE op=1 AND valid_to IS NULL;
        CREATE INDEX idx_current_aev ON facts(a,e,v) WHERE op=1 AND valid_to IS NULL;",
        )
        .unwrap();
}

fn rows(store: &Store) -> Vec<String> {
    let result = query(
        store,
        &format!(
            "SELECT ?s ?text WHERE {{ GRAPH <{EX}g> {{
        ?s <{EX}title> ?text FILTER(CONTAINS(LCASE(STR(?text)), \"seeds\"))
        }} }} ORDER BY STR(?s) STR(?text)"
        ),
    )
    .unwrap();
    result
        .rows()
        .iter()
        .map(|row| format!("{:?}/{:?}", row["s"], row["text"]))
        .collect()
}

fn table_columns(store: &Store, sql: &str, args: &[i64]) -> usize {
    let root: i64 = store
        .conn
        .query_row(
            "SELECT rootpage FROM sqlite_master WHERE name='facts'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut stmt = store.conn.prepare(&format!("EXPLAIN {sql}")).unwrap();
    let instructions: Vec<(String, i64, i64)> = stmt
        .query_map(rusqlite::params_from_iter(args), |r| {
            Ok((r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .unwrap()
        .map(|row| row.unwrap())
        .collect();
    let tables: HashSet<i64> = instructions
        .iter()
        .filter(|(op, _, page)| op == "OpenRead" && *page == root)
        .map(|(_, cursor, _)| *cursor)
        .collect();
    instructions
        .iter()
        .filter(|(op, cursor, _)| op == "Column" && tables.contains(cursor))
        .count()
}

#[test]
fn graph_membership_preserves_rows_and_removes_payload_column_reads() {
    let (store, graph) = fixture();
    let before = rows(&store);
    assert_eq!(
        before.len(),
        2,
        "positive titles, including language literal"
    );
    assert!(before.iter().all(|row| !row.contains("FOREIGN")));
    index(&store);
    assert_eq!(rows(&store), before);
    let (pattern, mut ctx) = shape();
    ctx.graph = GraphScope::Named(vec![graph]);
    let title = store.lookup(&format!("{EX}title")).unwrap().unwrap();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(title)];
    let narrowed = predicate(
        &store,
        &ctx,
        &pattern,
        &Bindings::new(),
        &[graph],
        &mut params,
    )
    .unwrap()
    .expect("eligible positive control must use index membership");
    let sql = |scope: &str| {
        format!(
            "SELECT e,a,v FROM facts INDEXED BY idx_current_aev WHERE a=?1 AND {scope} AND op=1 AND valid_to IS NULL GROUP BY e,a,quipu_term_key(v)"
        )
    };
    assert!(table_columns(&store, &sql("g=?2"), &[title, graph]) > 0);
    assert_eq!(table_columns(&store, &sql(&narrowed), &[title, graph]), 0);
}

#[test]
fn graph_membership_falls_back_for_unproved_index_history_and_scope() {
    let (store, graph) = fixture();
    let (pattern, ctx) = shape();
    let check = |context: &TemporalContext, graphs: &[i64]| {
        let mut params = Vec::new();
        let result = predicate(
            &store,
            context,
            &pattern,
            &Bindings::new(),
            graphs,
            &mut params,
        )
        .unwrap();
        assert!(
            params.is_empty(),
            "fallback must not append unused parameters"
        );
        assert!(result.is_none());
    };
    check(&ctx, &[graph]); // Missing profile index.
    store
        .conn
        .execute_batch(
            "CREATE INDEX idx_current_g ON facts(g) WHERE op=1 AND valid_to IS NULL AND g=0",
        )
        .unwrap();
    let expected = rows(&store);
    check(&ctx, &[graph]); // Same name, incomplete graph membership.
    assert_eq!(rows(&store), expected);
    store
        .conn
        .execute_batch("DROP INDEX idx_current_g")
        .unwrap();
    index(&store);
    check(
        &TemporalContext {
            as_of_tx: Some(1),
            ..ctx.clone()
        },
        &[graph],
    );
    check(
        &TemporalContext {
            valid_at: Some("2026-01-01T00:00:00Z".into()),
            ..ctx.clone()
        },
        &[graph],
    );
    check(&ctx, &[0]);
    check(&ctx, &[]);
    check(&ctx, &[graph, 0]);
    check(&TemporalContext::default(), &[graph]);
}
