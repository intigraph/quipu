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

#[test]
fn graph_membership_streamed_reads_keep_the_existing_plan() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::atomic::{AtomicUsize, Ordering};

    let (store, graph) = fixture();
    index(&store);
    let (pattern, mut ctx) = shape();
    ctx.graph = GraphScope::Named(vec![graph]);
    let metadata_reads = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&metadata_reads);
    store
        .conn
        .authorizer(Some(move |context: AuthContext<'_>| {
            if matches!(
                context.action,
                AuthAction::Read {
                    table_name: "sqlite_master",
                    column_name: "sql"
                }
            ) {
                observed.fetch_add(1, Ordering::Relaxed);
            }
            Authorization::Allow
        }))
        .unwrap();
    let mut streamed = Vec::new();
    let count = crate::sparql::triple::visit_triple_pattern(
        &store,
        &pattern,
        &Bindings::new(),
        &ctx,
        &mut |row| {
            streamed.push(row);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(count, 2);
    assert_eq!(
        metadata_reads.load(Ordering::Relaxed),
        0,
        "streaming must not probe the new membership-index definition"
    );
    let retained = crate::sparql::triple::eval_triple_pattern_limited(
        &store,
        &pattern,
        &Bindings::new(),
        &ctx,
        None,
    )
    .unwrap();
    assert_eq!(retained, streamed);
    assert!(
        metadata_reads.load(Ordering::Relaxed) > 0,
        "positive retained control proves the authorizer sees the index probe"
    );
}

#[test]
fn bound_subject_metadata_uses_same_rowid_cover_without_wide_fact_reads() {
    let (store,graph)=fixture();index(&store);
    let subject=store.lookup(&format!("{EX}one")).unwrap().unwrap();
    let title=store.lookup(&format!("{EX}title")).unwrap().unwrap();
    let parsed=SparqlParser::new().parse_query("SELECT ?v WHERE { ?s <https://example.org/title> ?v }").unwrap();
    let Query::Select{pattern,..}=parsed else {unreachable!()};
    let GraphPattern::Project{inner,..}=pattern else {unreachable!()};
    let GraphPattern::Bgp{patterns}=*inner else {unreachable!()};
    let seed=Bindings::from([("s".to_owned(),crate::Value::Ref(subject))]);
    let ctx=TemporalContext{graph:GraphScope::Named(vec![graph]),..Default::default()};
    let mut params:Vec<Box<dyn rusqlite::types::ToSql>>=vec![Box::new(title),Box::new(subject)];
    let membership=predicate(&store,&ctx,&patterns[0],&seed,&[graph],&mut params).unwrap().expect("bound metadata should use proven membership");
    let sql=|scope:&str|format!("SELECT e,a,v FROM facts INDEXED BY idx_current_aev WHERE a=?1 AND e=?2 AND {scope} AND op=1 AND valid_to IS NULL GROUP BY e,a,quipu_term_key(v)");
    assert!(table_columns(&store,&sql("g=?3"),&[title,subject,graph])>0);
    assert_eq!(table_columns(&store,&sql(&membership),&[title,subject,graph]),0);
    assert_eq!(table_columns(&store,&sql(&membership).replace(" INDEXED BY idx_current_aev", ""),&[title,subject,graph]),0,"actual unhinted caller plan must cover");
    let collect=|sql:String|store.conn.prepare(&sql).unwrap().query_map([title,subject,graph],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,Vec<u8>>(2)?))).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
    assert_eq!(collect(sql("g=?3")),collect(sql(&membership)));
}

#[test]
fn bound_metadata_preserves_variable_constant_and_reference_bags() {
    let (store,graph)=fixture();
    let queries=[
      format!("SELECT ?s ?v WHERE {{ VALUES ?s {{ <{EX}one> <{EX}two> <{EX}foreign> }} GRAPH <{EX}g> {{ ?s <{EX}title> ?v }} }} ORDER BY STR(?s) STR(?v)"),
      format!("SELECT ?s WHERE {{ VALUES ?s {{ <{EX}one> <{EX}two> }} GRAPH <{EX}g> {{ ?s <{EX}title> \"SEEDS alpha\" }} }} ORDER BY STR(?s)"),
      format!("SELECT ?s WHERE {{ VALUES ?s {{ <{EX}one> <{EX}two> }} GRAPH <{EX}g> {{ ?s <{EX}type> <{EX}Action> }} }} ORDER BY STR(?s)"),
    ];
    let rows=|q:&str|query(&store,q).unwrap().rows().iter().map(|r|format!("{:?}",r.iter().collect::<std::collections::BTreeMap<_,_>>())).collect::<Vec<_>>();
    let before=queries.iter().map(|q|rows(q)).collect::<Vec<_>>();assert_eq!(before.iter().map(Vec::len).collect::<Vec<_>>(),[2,1,1]);
    // Two assertions for one triple still produce one RDF triple, as before.
    store.conn.execute("INSERT INTO transactions(timestamp) VALUES('2026-01-02T00:00:00Z')",[]).unwrap();
    let tx=store.conn.last_insert_rowid();
    store.conn.execute("INSERT INTO facts(e,a,v,g,tx,valid_from,op) SELECT e,a,v,g,?3,valid_from,op FROM facts WHERE e=?1 AND g=?2",rusqlite::params![store.lookup(&format!("{EX}one")).unwrap().unwrap(),graph,tx]).unwrap();
    assert_eq!(queries.iter().map(|q|rows(q)).collect::<Vec<_>>(),before);
    index(&store);
    assert_eq!(queries.iter().map(|q|rows(q)).collect::<Vec<_>>(),before);
}

#[test]
fn bound_metadata_declines_unknown_subject_history_and_unproved_scope() {
    let (store,graph)=fixture();index(&store);let (pattern,_)=shape();
    let id=store.lookup(&format!("{EX}one")).unwrap().unwrap();
    let seed=Bindings::from([("s".to_owned(),crate::Value::Ref(id))]);
    let check=|ctx:TemporalContext,seed:&Bindings,graphs:&[i64]|{
      let mut params=Vec::new();assert!(predicate(&store,&ctx,&pattern,seed,graphs,&mut params).unwrap().is_none());assert!(params.is_empty());
    };
    check(TemporalContext::default(),&Bindings::new(),&[graph]);
    check(TemporalContext::default(),&Bindings::from([("s".to_owned(),crate::Value::Ref(-1))]),&[graph]);
    check(TemporalContext::default(),&Bindings::from([("s".to_owned(),crate::Value::Str("not an IRI".into()))]),&[graph]);
    check(TemporalContext{as_of_tx:Some(1),..Default::default()},&seed,&[graph]);
    check(TemporalContext{valid_at:Some("2026-01-01T00:00:00Z".into()),..Default::default()},&seed,&[graph]);
    check(TemporalContext::default(),&seed,&[0]);check(TemporalContext::default(),&seed,&[graph,0]);
    store.conn.execute_batch("DROP INDEX idx_current_g;CREATE INDEX idx_current_g ON facts(g) WHERE op=1 AND valid_to IS NULL AND g=0").unwrap();
    check(TemporalContext::default(),&seed,&[graph]);
}

#[test]
fn bound_streamed_metadata_uses_same_membership_without_changing_rows() {
    use rusqlite::hooks::{AuthAction,AuthContext,Authorization};
    use std::sync::atomic::{AtomicUsize,Ordering};
    let (store,graph)=fixture();index(&store);let (pattern,mut ctx)=shape();ctx.graph=GraphScope::Named(vec![graph]);ctx.string_narrows=None;
    let seed=Bindings::from([("s".to_owned(),crate::Value::Ref(store.lookup(&format!("{EX}one")).unwrap().unwrap()))]);
    let reads=Arc::new(AtomicUsize::new(0));let observed=Arc::clone(&reads);
    store.conn.authorizer(Some(move|c:AuthContext<'_>| {if matches!(c.action,AuthAction::Read{table_name:"sqlite_master",column_name:"sql"}) {observed.fetch_add(1,Ordering::Relaxed);}Authorization::Allow})).unwrap();
    let mut streamed=Vec::new();let count=crate::sparql::triple::visit_triple_pattern(&store,&pattern,&seed,&ctx,&mut|r|{streamed.push(r);Ok(())}).unwrap();
    assert_eq!(count,1);assert!(reads.load(Ordering::Relaxed)>0,"bound streaming must check validated membership index");
    let retained=crate::sparql::triple::eval_triple_pattern_limited(&store,&pattern,&seed,&ctx,None).unwrap();assert_eq!(streamed,retained);
}

#[test]
fn bound_object_membership_uses_covering_graph_index_without_payload_columns() {
    let (store,graph)=fixture();index(&store);
    let (pattern,_)=shape();
    let ctx=TemporalContext {graph:GraphScope::Named(vec![graph]),..TemporalContext::default()};
    let seed=Bindings::from([("text".to_owned(),crate::Value::Str("SEEDS alpha".into()))]);
    let title=store.lookup(&format!("{EX}title")).unwrap().unwrap();
    let mut params:Vec<Box<dyn rusqlite::types::ToSql>>=vec![Box::new(title)];
    let scope=predicate(&store,&ctx,&pattern,&seed,&[graph],&mut params).unwrap()
        .expect("bound object must keep graph checks on the covering rowid index");
    let bytes=crate::Value::Str("SEEDS alpha".into()).to_bytes();
    let hex:String=bytes.iter().map(|b|format!("{b:02x}")).collect();
    let sql=|scope:&str|format!("SELECT e,a,v FROM facts INDEXED BY idx_current_aev WHERE a=?1 AND v=X'{hex}' AND {scope} AND op=1 AND valid_to IS NULL GROUP BY e,a,quipu_term_key(v)");
    assert!(table_columns(&store,&sql("g=?2"),&[title,graph])>0);
    assert_eq!(table_columns(&store,&sql(&scope),&[title,graph]),0);
    let q=format!("SELECT ?s WHERE {{ GRAPH <{EX}g> {{ ?s <{EX}title> \"SEEDS alpha\" }} }}");
    assert_eq!(query(&store,&q).unwrap().rows().len(),1);
    let foreign=format!("SELECT ?s WHERE {{ GRAPH <{EX}g> {{ ?s <{EX}title> \"seeds FOREIGN ONLY\" }} }}");
    assert!(query(&store,&foreign).unwrap().rows().is_empty());
}

#[test]
fn retained_object_lookup_seeks_the_value_leading_index() {
    let (store,graph)=fixture();index(&store);
    let (pattern,_)=shape();
    let ctx=TemporalContext {graph:GraphScope::Named(vec![graph]),..TemporalContext::default()};
    let seed=Bindings::from([("text".to_owned(),crate::Value::Str("SEEDS alpha".into()))]);
    let a=store.lookup(&format!("{EX}title")).unwrap().unwrap();
    let source=object_source(&store,&ctx,&pattern,&seed).unwrap().unwrap_or("facts");
    let sql=format!("EXPLAIN QUERY PLAN SELECT e,a,v FROM {source} WHERE a=?1 AND v IN (?2,?3) AND op=1 AND valid_to IS NULL GROUP BY e,a,quipu_term_key(v)");
    let value=crate::Value::Str("SEEDS alpha".into()).to_bytes();
    let plans:Vec<String>=store.conn.prepare(&sql).unwrap().query_map(rusqlite::params![a,value,value],|r|r.get(3)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
    assert!(plans.iter().any(|p|p.contains("idx_vaet")&&p.contains("v=?")&&p.contains("a=?")),"object point seek required: {plans:?}");
    let q=format!("SELECT ?s WHERE {{ GRAPH <{EX}g> {{ ?s <{EX}title> \"SEEDS alpha\" }} }}");
    assert_eq!(query(&store,&q).unwrap().rows().len(),1);
}

#[test]
fn value_source_declines_unproved_shapes_and_index() {
    let (store, graph) = fixture();
    index(&store);
    let (pattern, _) = shape();
    let ctx = TemporalContext { graph: GraphScope::Named(vec![graph]), ..Default::default() };
    let seed = Bindings::from([("text".to_owned(), crate::Value::Str("SEEDS alpha".into()))]);
    assert!(object_source(&store, &ctx, &pattern, &seed).unwrap().is_some());
    assert!(object_source(&store, &ctx, &pattern, &Bindings::new()).unwrap().is_none());
    let mut bound = seed.clone();
    bound.insert("s".to_owned(), crate::Value::Ref(1));
    assert!(object_source(&store, &ctx, &pattern, &bound).unwrap().is_none());
    for graph in [GraphScope::Default(vec![0]), GraphScope::Named(vec![0]), GraphScope::Named(vec![graph, 0])] {
        let other = TemporalContext { graph, ..Default::default() };
        assert!(object_source(&store, &other, &pattern, &seed).unwrap().is_none());
    }
    let historic = TemporalContext { graph: ctx.graph.clone(), as_of_tx: Some(1), ..Default::default() };
    assert!(object_source(&store, &historic, &pattern, &seed).unwrap().is_none());
    store.conn.execute_batch("DROP INDEX idx_vaet; CREATE INDEX idx_vaet ON facts(a,v,e,valid_from)").unwrap();
    assert!(object_source(&store, &ctx, &pattern, &seed).unwrap().is_none());
    let q = format!("SELECT ?s WHERE {{ GRAPH <{EX}g> {{ ?s <{EX}title> \"SEEDS alpha\" }} }}");
    assert_eq!(query(&store, &q).unwrap().rows().len(), 1);
    store.conn.execute_batch("DROP INDEX idx_vaet").unwrap();
    assert!(object_source(&store, &ctx, &pattern, &seed).unwrap().is_none());
    assert_eq!(query(&store, &q).unwrap().rows().len(), 1);
}

#[test]
fn streamed_bound_object_avoids_the_predicate_range_scan() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let (mut store, graph) = fixture();
    let data: String = (0..1500).map(|i| format!("<{EX}bulk{i}> <{EX}title> \"unrelated\" .\n")).collect();
    ingest_rdf_to_graph(&mut store, data.as_bytes(), RdfFormat::NTriples, None,
        "2026-01-01T00:00:00Z", None, None, graph).unwrap();
    index(&store);
    let (pattern, _) = shape();
    let ctx = TemporalContext { graph: GraphScope::Named(vec![graph]), ..Default::default() };
    let seed = Bindings::from([("text".to_owned(), crate::Value::Str("SEEDS alpha".into()))]);
    let steps = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&steps);
    store.conn.progress_handler(100, Some(move || { observed.fetch_add(1, Ordering::Relaxed); false })).unwrap();
    let mut rows = Vec::new();
    let count = crate::sparql::triple::visit_triple_pattern(&store, &pattern, &seed, &ctx,
        &mut |row| { rows.push(row); Ok(()) }).unwrap();
    store.conn.progress_handler(0, None::<fn() -> bool>).unwrap();
    assert_eq!(count, 1);
    assert_eq!(rows.len(), 1);
    assert!(steps.load(Ordering::Relaxed) < 50, "object point lookup exceeded 5000 VM steps: {}", steps.load(Ordering::Relaxed));
}
