use super::*;
use crate::rdf::ingest_rdf_to_graph;
use crate::sparql::{TemporalContext, query, query_temporal};
use oxrdfio::RdfFormat;
const G: &str = "http://example.org/search";
fn add(store: &mut Store, text: &str) {
    let graph = store.graph_create(G).unwrap();
    ingest_rdf_to_graph(
        store,
        text.as_bytes(),
        RdfFormat::Turtle,
        None,
        "2026-10-01T00:00:00Z",
        None,
        None,
        graph,
    )
    .unwrap();
}
fn fixture() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    add(
        &mut store,
        "<http://example.org/a> <http://example.org/name> \"Quipu Straße Éclair\" . <http://example.org/b> <http://example.org/name> \"unrelated\" .",
    );
    store
}
fn search(needle: &str) -> String {
    format!(
        "SELECT ?s WHERE {{ GRAPH <{G}> {{ ?s <http://example.org/name> ?text . FILTER(isLiteral(?text) && sameTerm(?text,STR(?text)) && CONTAINS(LCASE(STR(?text)), {})) }} }}",
        serde_json::to_string(needle).unwrap()
    )
}
fn finish(store: &Store) {
    for _ in 0..1000 {
        if store.backfill_substring_batch(1).unwrap().complete {
            assert!(store.substring_index_ready().unwrap());
            return;
        }
    }
    panic!("fixture backfill exceeded bound");
}
#[test]
fn incomplete_index_never_hides_matches_and_does_not_backfill_on_read() {
    let store = fixture();
    let before = query(&store, &search("quipu")).unwrap().rows().len();
    assert_eq!(before, 1);
    assert!(store.substring_progress().unwrap().is_none());
    store.initialize_substring_index().unwrap();
    assert!(!store.substring_index_ready().unwrap());
    let cursor = store.substring_progress().unwrap().unwrap().cursor;
    assert_eq!(
        query(&store, &search("quipu")).unwrap().rows().len(),
        before
    );
    assert_eq!(store.substring_progress().unwrap().unwrap().cursor, cursor);
    finish(&store);
    for (needle, count) in [
        ("quipu", 1),
        ("straße", 1),
        ("écl", 1),
        ("zz_no_hit", 0),
        ("qu", 1),
    ] {
        assert_eq!(
            query(&store, &search(needle)).unwrap().rows().len(),
            count,
            "{needle}"
        );
    }
}
#[test]
fn writes_update_index_in_the_same_transaction_and_rollback_preserves_it() {
    let mut store = fixture();
    finish(&store);
    add(
        &mut store,
        "<http://example.org/c> <http://example.org/name> \"new Quipu\" .",
    );
    assert_eq!(query(&store, &search("quipu")).unwrap().rows().len(), 2);
    let entity = store.lookup("http://example.org/a").unwrap().unwrap();
    let attribute = store.lookup("http://example.org/name").unwrap().unwrap();
    store
        .conn
        .execute_batch("SAVEPOINT test_substring_rollback")
        .unwrap();
    store
        .conn
        .execute(
            "UPDATE facts SET v=?1 WHERE e=?2 AND a=?3",
            params![
                Value::Str("changed source".into()).to_bytes(),
                entity,
                attribute
            ],
        )
        .unwrap();
    assert_eq!(query(&store, &search("quipu")).unwrap().rows().len(), 1);
    assert_eq!(query(&store, &search("source")).unwrap().rows().len(), 1);
    store
        .conn
        .execute_batch("ROLLBACK TO test_substring_rollback; RELEASE test_substring_rollback")
        .unwrap();
    assert_eq!(query(&store, &search("quipu")).unwrap().rows().len(), 2);
    assert_eq!(query(&store, &search("source")).unwrap().rows().len(), 0);
    store
        .conn
        .execute("DELETE FROM facts WHERE e=?1", [entity])
        .unwrap();
    assert_eq!(query(&store, &search("quipu")).unwrap().rows().len(), 1);
}
#[test]
fn retraction_keeps_historical_candidates_and_scope_is_exact() {
    let mut store = fixture();
    finish(&store);
    let other = store.graph_create("http://example.org/other").unwrap();
    ingest_rdf_to_graph(
        &mut store,
        &b"<http://example.org/d> <http://example.org/name> \"Quipu\" ."[..],
        RdfFormat::Turtle,
        None,
        "2026-10-01T00:00:00Z",
        None,
        None,
        other,
    )
    .unwrap();
    add(
        &mut store,
        "<http://example.org/e> <http://example.org/description> \"Quipu\" .",
    );
    assert_eq!(query(&store, &search("quipu")).unwrap().rows().len(), 1);
    let entity = store.lookup("http://example.org/a").unwrap().unwrap();
    store
        .conn
        .execute(
            "UPDATE facts SET valid_to='2026-10-10T00:00:00Z' WHERE e=?1",
            [entity],
        )
        .unwrap();
    assert_eq!(query(&store, &search("quipu")).unwrap().rows().len(), 0);
    let ctx = TemporalContext {
        valid_at: Some("2026-10-05T00:00:00Z".into()),
        ..TemporalContext::default()
    };
    assert_eq!(
        query_temporal(&store, &search("quipu"), &ctx)
            .unwrap()
            .rows()
            .len(),
        1
    );
}
#[test]
fn missing_maintenance_or_wrong_tokenizer_refuses_narrowing() {
    let store = fixture();
    finish(&store);
    store
        .conn
        .execute_batch("DROP TRIGGER substring_insert")
        .unwrap();
    assert!(!store.substring_index_ready().unwrap());
    assert_eq!(query(&store, &search("quipu")).unwrap().rows().len(), 1);
    store.drop_substring_index().unwrap();
    assert!(store.substring_progress().unwrap().is_none());
    assert_eq!(query(&store, &search("quipu")).unwrap().rows().len(), 1);
    store.initialize_substring_index().unwrap();
    finish(&store);
    store.conn.execute_batch("DROP TABLE substring_fts; CREATE VIRTUAL TABLE substring_fts USING fts5(scope,text,tokenize='unicode61')").unwrap();
    assert!(!store.substring_index_ready().unwrap());
    assert_eq!(query(&store, &search("quipu")).unwrap().rows().len(), 1);
}

#[test]
fn native_query_reads_candidate_index_only_after_complete_admission() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let store = fixture();
    store.initialize_substring_index().unwrap();
    let reads = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&reads);
    store
        .conn
        .authorizer(Some(move |context: AuthContext<'_>| {
            if matches!(
                context.action,
                AuthAction::Read {
                    table_name: "substring_fts",
                    ..
                }
            ) {
                observed.fetch_add(1, Ordering::Relaxed);
            }
            Authorization::Allow
        }))
        .unwrap();
    assert_eq!(query(&store, &search("quipu")).unwrap().rows().len(), 1);
    assert_eq!(
        reads.load(Ordering::Relaxed),
        0,
        "partial index must not be consulted"
    );
    finish(&store);
    reads.store(0, Ordering::Relaxed);
    assert_eq!(query(&store, &search("quipu")).unwrap().rows().len(), 1);
    assert!(
        reads.load(Ordering::Relaxed) > 0,
        "actual native leaf must use the candidate index"
    );
}

#[test]
fn unproved_literal_guards_never_drop_iri_or_numeric_matches() {
    let mut store = fixture();
    add(
        &mut store,
        "<http://example.org/iri> <http://example.org/name> <http://example.org/quipu> . <http://example.org/number> <http://example.org/name> 12345 .",
    );
    finish(&store);
    for (needle, count) in [("quipu", 2), ("123", 1)] {
        let q = format!(
            "SELECT ?s WHERE {{ GRAPH <{G}> {{ ?s <http://example.org/name> ?text FILTER(CONTAINS(LCASE(STR(?text)),{})) }} }}",
            serde_json::to_string(needle).unwrap()
        );
        assert_eq!(query(&store, &q).unwrap().rows().len(), count);
    }
}

#[test]
fn unicode_nul_and_typed_strings_keep_exact_filter_semantics() {
    let mut store = fixture();
    add(
        &mut store,
        "<http://example.org/nul> <http://example.org/name> \"alpha\\u0000Quipu\" . <http://example.org/lang> <http://example.org/name> \"Quipu\"@en . <http://example.org/typed> <http://example.org/name> \"Quipu\"^^<http://www.w3.org/2001/XMLSchema#string> . <http://example.org/fold> <http://example.org/name> \"İstanbul\" .",
    );
    let needles = ["quipu", "alpha\0quipu", "i\u{307}st", "straße", "zz_no_hit"];
    let before: Vec<_> = needles
        .iter()
        .map(|needle| query(&store, &search(needle)).unwrap().rows().len())
        .collect();
    assert_eq!(before, vec![3, 1, 1, 1, 0]);
    finish(&store);
    for (needle, count) in needles.iter().zip(before) {
        assert_eq!(
            query(&store, &search(needle)).unwrap().rows().len(),
            count,
            "{needle}"
        );
    }
}

#[test]
fn reopened_writer_registers_trigger_codec_without_implicit_rebuild() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("store.db");
    {
        let mut store = Store::open(path.to_str().unwrap()).unwrap();
        add(
            &mut store,
            "<http://example.org/a> <http://example.org/name> \"Quipu\" .",
        );
        finish(&store);
    }
    let mut store = Store::open(path.to_str().unwrap()).unwrap();
    assert!(store.substring_index_ready().unwrap());
    add(
        &mut store,
        "<http://example.org/b> <http://example.org/name> \"Quipu new\" .",
    );
    assert_eq!(query(&store, &search("quipu")).unwrap().rows().len(), 2);
}

#[test]
fn candidate_rowids_drive_native_scan_instead_of_retesting_the_predicate() {
    let mut store = fixture();
    let extra=(0..1000).map(|n|format!("<http://example.org/unrelated-{n}> <http://example.org/name> \"irrelevant text {n}\" .\n")).collect::<String>();
    add(&mut store, &extra);
    crate::sparql::string_pushdown::value_visits(true);
    assert_eq!(query(&store, &search("quipu")).unwrap().rows().len(), 1);
    let before = crate::sparql::string_pushdown::value_visits(true);
    assert!(before >= 1000, "populated predicate-scan control: {before}");
    assert!(store.backfill_substring_batch(10_000).unwrap().complete);
    assert!(store.substring_index_ready().unwrap());
    crate::sparql::string_pushdown::value_visits(true);
    assert_eq!(query(&store, &search("quipu")).unwrap().rows().len(), 1);
    let after = crate::sparql::string_pushdown::value_visits(false);
    assert!(
        after <= 4,
        "candidate lookup still retested {after} predicate rows (before {before})"
    );
}

#[test]
fn optional_substring_tables_are_declared_local_and_never_carried() {
    let store = fixture();
    finish(&store);
    let tables = store
        .conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name LIKE 'substring_%'")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(
        tables.len() >= 6,
        "actual optional FTS schema must be visible"
    );
    for table in tables {
        assert_eq!(
            crate::share_completeness::disposition(&table),
            Some(crate::share_completeness::Disposition::Excluded),
            "{table}"
        );
        assert!(!crate::share_completeness::carried().contains(&table.as_str()));
    }
    assert!(crate::share_completeness::carried().contains(&"facts"));
    store.drop_substring_index().unwrap();
    assert_eq!(query(&store, &search("quipu")).unwrap().rows().len(), 1);
}

#[test]
fn subject_only_projection_uses_complete_index_without_reading_wide_facts() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    let store = fixture();
    finish(&store);
    store
        .conn
        .authorizer(Some(|context: AuthContext<'_>| {
            if matches!(
                context.action,
                AuthAction::Read {
                    table_name: "facts",
                    ..
                }
            ) {
                Authorization::Deny
            } else {
                Authorization::Allow
            }
        }))
        .unwrap();
    let q = format!(
        "SELECT ?s WHERE {{ GRAPH <{G}> {{ {{ SELECT ?s WHERE {{ ?s <http://example.org/name> ?text . FILTER(isLiteral(?text) && sameTerm(?text,STR(?text)) && CONTAINS(LCASE(STR(?text)),\"quipu\")) }} }} }} }}"
    );
    let result = query(&store, &q);
    assert!(
        result.is_ok(),
        "complete derived projection must not read facts: {result:?}"
    );
    assert_eq!(result.unwrap().rows().len(), 1);
}

fn projected(needle: &str) -> String {
    format!(
        "SELECT ?s WHERE {{ GRAPH <{G}> {{ {{ SELECT ?s WHERE {{ ?s <http://example.org/name> ?text . FILTER(isLiteral(?text) && sameTerm(?text,STR(?text)) && CONTAINS(LCASE(STR(?text)),{})) }} }} }} }}",
        serde_json::to_string(needle).unwrap()
    )
}
fn projected_bag(store: &Store, q: &str, ctx: &TemporalContext) -> Vec<String> {
    let result = query_temporal(store, q, ctx).unwrap();
    let mut rows: Vec<_> = result.rows().iter().map(|row| format!("{row:?}")).collect();
    rows.sort();
    rows
}
#[test]
fn compact_projection_preserves_rdf_bags_even_with_forced_hash_collisions() {
    let mut store = fixture();
    add(
        &mut store,
        r#"<http://example.org/a> <http://example.org/name> "Quipu second" .
      <http://example.org/c> <http://example.org/name> "Quipu third"^^<http://www.w3.org/2001/XMLSchema#string> .
      <http://example.org/lang> <http://example.org/name> "Quipu"@en .
      <http://example.org/ref> <http://example.org/name> <http://example.org/Quipu> ."#,
    );
    let entity = store.lookup("http://example.org/a").unwrap().unwrap();
    store
        .conn
        .execute(
            "INSERT INTO transactions(timestamp) VALUES('2026-10-03T00:00:00Z')",
            [],
        )
        .unwrap();
    let tx = store.conn.last_insert_rowid();
    store.conn.execute("INSERT INTO facts(e,a,v,g,tx,valid_from,valid_to,op,retracted_tx) SELECT e,a,v,g,?1,valid_from,valid_to,op,retracted_tx FROM facts WHERE e=?2 LIMIT 1",rusqlite::params![tx,entity]).unwrap();
    let expected = projected_bag(&store, &projected("quipu"), &TemporalContext::default());
    assert_eq!(
        expected.len(),
        3,
        "two distinct a terms plus c, not a duplicate assertion or lang/ref"
    );
    finish(&store);
    assert_eq!(
        projected_bag(&store, &projected("quipu"), &TemporalContext::default()),
        expected
    );
    store
        .conn
        .execute("UPDATE substring_facts SET value_hash=zeroblob(32)", [])
        .unwrap();
    assert_eq!(
        projected_bag(&store, &projected("quipu"), &TemporalContext::default()),
        expected,
        "fingerprints cannot decide equality"
    );
    assert!(projected_bag(&store, &projected("zz_no_hit"), &TemporalContext::default()).is_empty());
}
#[test]
fn compact_projection_keeps_nul_unicode_graph_and_time_boundaries() {
    let mut store = fixture();
    add(
        &mut store,
        r#"<http://example.org/nul> <http://example.org/name> "a\u0000b after ÉCLAIR" ."#,
    );
    finish(&store);
    let entity = store.lookup("http://example.org/a").unwrap().unwrap();
    store
        .conn
        .execute(
            "UPDATE facts SET valid_to='2026-10-10T00:00:00Z',retracted_tx=2 WHERE e=?1",
            [entity],
        )
        .unwrap();
    for needle in [
        "quipu",
        "straße",
        "écl",
        "after",
        "a b",
        "a\0b",
        "zz_no_hit",
    ] {
        for ctx in [
            TemporalContext::default(),
            TemporalContext {
                valid_at: Some("2026-10-05T00:00:00Z".into()),
                ..TemporalContext::default()
            },
            TemporalContext {
                as_of_tx: Some(1),
                ..TemporalContext::default()
            },
            TemporalContext {
                as_of_tx: Some(2),
                ..TemporalContext::default()
            },
        ] {
            assert_eq!(
                projected_bag(&store, &projected(needle), &ctx),
                projected_bag(&store, &search(needle), &ctx),
                "{needle}/{ctx:?}"
            );
        }
    }
    let other = store.graph_create("http://example.org/other").unwrap();
    ingest_rdf_to_graph(
        &mut store,
        &b"<http://example.org/foreign> <http://example.org/name> \"Quipu\" ."[..],
        RdfFormat::Turtle,
        None,
        "2026-10-01T00:00:00Z",
        None,
        None,
        other,
    )
    .unwrap();
    assert!(projected_bag(&store, &projected("quipu"), &TemporalContext::default()).is_empty());
}
#[test]
fn modified_maintenance_never_authorizes_compact_reads() {
    let store = fixture();
    finish(&store);
    store.conn.execute_batch("DROP TRIGGER substring_update;CREATE TRIGGER substring_update AFTER UPDATE ON facts BEGIN SELECT 1;END;").unwrap();
    assert!(!store.substring_index_ready().unwrap());
    assert_eq!(
        query(&store, &projected("quipu")).unwrap().rows().len(),
        1,
        "fallback retains ordinary facts"
    );
}

#[test]
fn exact_text_is_sparse_but_nul_normalization_never_decides_a_match() {
    let mut store = fixture();
    add(
        &mut store,
        r#"<http://example.org/nul> <http://example.org/name> "a\u0000b after ÉCLAIR" ."#,
    );
    finish(&store);
    let ordinary: (i64,i64) = store.conn.query_row(
        "SELECT count(*),sum(exact_text IS NULL) FROM substring_fts WHERE instr(text,char(0))=0 AND text NOT LIKE '%a b after%'",
        [],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert!(ordinary.0 >= 2, "populated ordinary-string control");
    assert_eq!(
        ordinary.0, ordinary.1,
        "ordinary strings must not store a duplicate exact lowercase value"
    );
    let exact: Option<String> = store
        .conn
        .query_row(
            "SELECT exact_text FROM substring_fts WHERE text='a b after éclair'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(exact.as_deref(), Some("a\0b after éclair"));
    for needle in [
        "quipu",
        "straße",
        "écl",
        "after",
        "a b",
        "a\0b",
        "zz_no_hit",
    ] {
        assert_eq!(
            projected_bag(&store, &projected(needle), &TemporalContext::default()),
            projected_bag(&store, &search(needle), &TemporalContext::default()),
            "{needle:?}"
        );
    }
}

#[test]
fn old_dense_index_version_refuses_sparse_readiness_without_upgrading() {
    let store = fixture();
    finish(&store);
    store
        .conn
        .execute("UPDATE substring_progress SET version=2", [])
        .unwrap();
    assert!(!store.substring_index_ready().unwrap());
    let before: (i64, i64) = store
        .conn
        .query_row("SELECT cursor,version FROM substring_progress", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(
        projected_bag(&store, &projected("quipu"), &TemporalContext::default()),
        projected_bag(&store, &search("quipu"), &TemporalContext::default())
    );
    let after: (i64, i64) = store
        .conn
        .query_row("SELECT cursor,version FROM substring_progress", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(
        before, after,
        "queries must not initialize or upgrade derived custody"
    );
}

#[test]
fn ordinary_projection_does_not_fetch_fts_text_after_exact_phrase_match() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    let store = fixture();
    finish(&store);
    store
        .conn
        .authorizer(Some(|context: AuthContext<'_>| {
            if matches!(
                context.action,
                AuthAction::Read {
                    table_name: "substring_fts",
                    column_name: "text",
                    ..
                }
            ) {
                Authorization::Deny
            } else {
                Authorization::Allow
            }
        }))
        .unwrap();
    let result = query(&store, &projected("quipu"));
    assert!(
        result.is_ok(),
        "ordinary exact phrase must not fetch stored text: {result:?}"
    );
    assert_eq!(result.unwrap().rows().len(), 1);
}

#[test]
fn exact_trigram_phrase_matches_lowercase_substrings_including_punctuation() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(FTS).unwrap();
    let alphabet = ['a', 'B', 'é', 'İ', 'ß', '🐟', ' ', '"', '\n', '\0'];
    let mut documents = Vec::new();
    for n in 0..100 {
        let text: String = (0..9)
            .map(|p| alphabet[(n * 7 + p * p + 3 * p) % alphabet.len()])
            .collect();
        documents.push(text.to_lowercase());
    }
    documents.extend(
        [
            "a b",
            "a\0b",
            "quoted \"word\"",
            "Straße ÉCLAIR İstanbul",
            "🐟 fish",
        ]
        .map(str::to_owned),
    );
    let mut needles = std::collections::BTreeSet::new();
    for (i, text) in documents.iter().enumerate() {
        conn.execute(
            "INSERT INTO substring_fts(rowid,text,exact_text) VALUES(?1,?2,?3)",
            params![
                i as i64 + 1,
                text.replace('\0', " "),
                text.contains('\0').then_some(text)
            ],
        )
        .unwrap();
        let chars: Vec<_> = text.chars().collect();
        for start in 0..chars.len() {
            for len in 3..=5 {
                if start + len <= chars.len() {
                    let needle: String = chars[start..start + len].iter().collect();
                    if !needle.contains('\0') {
                        needles.insert(needle);
                    }
                }
            }
        }
    }
    needles.extend(["absent control", "a b", "word", "écl", "🐟 f"].map(str::to_owned));
    assert!(needles.len() > 40, "positive generated coverage");
    for needle in needles {
        let phrase = format!("text:\"{}\"", needle.replace('"', "\"\""));
        let mut stmt=conn.prepare("SELECT rowid FROM substring_fts WHERE substring_fts MATCH ?1 AND (exact_text IS NULL OR instr(exact_text,?2)>0) ORDER BY rowid").unwrap();
        let actual: Vec<i64> = stmt
            .query_map(params![phrase, needle], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let expected: Vec<i64> = documents
            .iter()
            .enumerate()
            .filter(|(_, text)| text.contains(&needle))
            .map(|(i, _)| i as i64 + 1)
            .collect();
        assert_eq!(actual, expected, "{needle:?}");
    }
}

#[test]
fn ordinary_projection_never_opens_fts_content_pages() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    let store = fixture();
    finish(&store);
    store
        .conn
        .authorizer(Some(|context: AuthContext<'_>| {
            if matches!(
                context.action,
                AuthAction::Read {
                    table_name: "substring_fts_content",
                    ..
                }
            ) {
                Authorization::Deny
            } else {
                Authorization::Allow
            }
        }))
        .unwrap();
    let result = query(&store, &projected("quipu"));
    assert!(
        result.is_ok(),
        "ordinary phrase projection must not open FTS content: {result:?}"
    );
    assert_eq!(result.unwrap().rows().len(), 1);
}

#[test]
fn sparse_v3_without_exact_flag_refuses_readiness_without_upgrade() {
    let store = fixture();
    finish(&store);
    store
        .conn
        .execute("UPDATE substring_progress SET version=3", [])
        .unwrap();
    assert!(!store.substring_index_ready().unwrap());
    assert_eq!(query(&store, &projected("quipu")).unwrap().rows().len(), 1);
    assert_eq!(
        store
            .conn
            .query_row("SELECT version FROM substring_progress", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        3
    );
}

#[test]
fn exact_flag_changes_and_rolls_back_with_original_nul_value() {
    let store = fixture();
    finish(&store);
    let entity = store.lookup("http://example.org/a").unwrap().unwrap();
    assert_eq!(
        store
            .conn
            .query_row("SELECT sum(needs_exact) FROM substring_facts", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    store.conn.execute_batch("SAVEPOINT nul_flag_test").unwrap();
    store
        .conn
        .execute(
            "UPDATE facts SET v=?1 WHERE e=?2",
            params![Value::Str("a\0b after".into()).to_bytes(), entity],
        )
        .unwrap();
    assert_eq!(
        store
            .conn
            .query_row("SELECT sum(needs_exact) FROM substring_facts", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(query(&store, &projected("a b")).unwrap().rows().is_empty());
    assert_eq!(query(&store, &projected("after")).unwrap().rows().len(), 1);
    store
        .conn
        .execute_batch("ROLLBACK TO nul_flag_test;RELEASE nul_flag_test")
        .unwrap();
    assert_eq!(
        store
            .conn
            .query_row("SELECT sum(needs_exact) FROM substring_facts", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(query(&store, &projected("quipu")).unwrap().rows().len(), 1);
}

#[test]
fn substring_readiness_is_pinned_and_not_cached_after_drop() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ready.db");
    let mut reader = Store::open(path.to_str().unwrap()).unwrap();
    reader
        .conn
        .execute_batch("PRAGMA journal_mode=WAL")
        .unwrap();
    add(
        &mut reader,
        "<http://example.org/a> <http://example.org/name> \"quipu control\" .",
    );
    assert!(reader.substring_read().is_err());
    assert!(reader.conn.is_autocommit());
    finish(&reader);
    let writer = Store::open(path.to_str().unwrap()).unwrap();
    let transaction = reader.substring_read().unwrap();
    assert!(!reader.conn.is_autocommit());
    assert!(reader.substring_read().is_err(), "nested scope refuses");
    writer.drop_substring_index().unwrap();
    assert!(
        reader.substring_index_ready().unwrap(),
        "read snapshot retains admitted schema"
    );
    assert_eq!(query(&reader, &projected("quipu")).unwrap().rows().len(), 1);
    transaction.commit().unwrap();
    assert!(
        reader.substring_read().is_err(),
        "new request must observe dropped index"
    );
    assert!(reader.conn.is_autocommit());
}
