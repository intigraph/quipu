use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn fixture(count: i64) -> (rusqlite::Connection, Arc<AtomicUsize>) {
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    let decoded = Arc::new(AtomicUsize::new(0));
    let observed = decoded.clone();
    connection
        .create_scalar_function(
            "observe_alias",
            1,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8,
            move |ctx| {
                observed.fetch_add(1, Ordering::Relaxed);
                ctx.get::<i64>(0)
            },
        )
        .unwrap();
    connection.execute_batch(
        "CREATE TABLE alias_source(alias_id INTEGER PRIMARY KEY, canonical_id INTEGER NOT NULL);
         CREATE VIEW term_alias AS SELECT observe_alias(alias_id) AS alias_id, canonical_id FROM alias_source;",
    ).unwrap();
    if count > 0 {
        connection
            .execute(
                "WITH RECURSIVE ids(id) AS (VALUES(1) UNION ALL SELECT id+1 FROM ids WHERE id < ?1)
             INSERT INTO alias_source SELECT id, id*10 FROM ids",
                [count],
            )
            .unwrap();
    }
    (connection, decoded)
}

#[test]
fn oversized_snapshot_does_not_decode_a_discarded_prefix() {
    let (connection, decoded) = fixture(4097);
    assert!(
        read_snapshot(&connection, &TemporalContext::default())
            .unwrap()
            .is_none()
    );
    assert_eq!(decoded.load(Ordering::Relaxed), 0);
}

#[test]
fn complete_boundary_snapshot_keeps_every_alias() {
    let (connection, decoded) = fixture(4096);
    let aliases = read_snapshot(&connection, &TemporalContext::default())
        .unwrap()
        .unwrap();
    assert_eq!(aliases.len(), 4096);
    assert_eq!(aliases[&1], 10);
    assert_eq!(aliases[&4096], 40960);
    assert_eq!(decoded.load(Ordering::Relaxed), 4096);
}

#[test]
fn empty_snapshot_is_complete() {
    let (connection, _) = fixture(0);
    assert!(
        read_snapshot(&connection, &TemporalContext::default())
            .unwrap()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn expired_snapshot_does_not_decode_rows() {
    let (connection, decoded) = fixture(4097);
    let ctx = TemporalContext {
        deadline: Some(crate::time::Deadline::after_millis(0)),
        ..Default::default()
    };
    assert!(matches!(
        read_snapshot(&connection, &ctx),
        Err(crate::Error::QueryTimeout { .. })
    ));
    assert_eq!(decoded.load(Ordering::Relaxed), 0);
}

fn aliased_store() -> (tempfile::TempDir, Store, Arc<AtomicUsize>, Vec<(i64, i64)>) {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.db");
    let raw = dir.path().join("raw.db");
    let pack = dir.path().join("pack.db");
    let writer = Store::open(main.to_str().unwrap()).unwrap();
    let provider = Store::open(raw.to_str().unwrap()).unwrap();
    let iris = ["http://example.org/first", "http://example.org/second"];
    for iri in iris {
        writer.intern(iri).unwrap();
        provider.intern(iri).unwrap();
    }
    drop(writer);
    drop(provider);
    crate::store::respace::respace_file(&raw, &pack, 1).unwrap();
    let store = Store::open_with_attachments(
        main.to_str().unwrap(),
        &[crate::store::attach::Attachment::read_only(
            "archive",
            pack.to_str().unwrap(),
        )],
    )
    .unwrap();
    let ids = iris
        .into_iter()
        .map(|iri| {
            let canonical = store.lookup(iri).unwrap().unwrap();
            let aliases = store.lookup_all(iri).unwrap();
            assert_eq!(aliases.len(), 2, "positive physical alias control");
            let alias = aliases.into_iter().find(|id| *id != canonical).unwrap();
            (canonical, alias)
        })
        .collect();
    let reads = Arc::new(AtomicUsize::new(0));
    let observed = reads.clone();
    store
        .conn
        .create_scalar_function(
            "observe_canonical",
            1,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8,
            move |ctx| {
                observed.fetch_add(1, Ordering::Relaxed);
                ctx.get::<i64>(0)
            },
        )
        .unwrap();
    store.conn.execute_batch(
        "ALTER TABLE term_alias RENAME TO observed_alias_source;
         CREATE TEMP VIEW term_alias AS SELECT alias_id, observe_canonical(canonical_id) AS canonical_id FROM observed_alias_source;",
    ).unwrap();
    (dir, store, reads, ids)
}

#[test]
fn indexed_canonicalization_reuses_one_leafs_mapping() {
    let (_dir, store, reads, ids) = aliased_store();
    let resolver = Canonicalizer::new(&store, None);
    assert_eq!(resolver.resolve(ids[0].1).unwrap(), ids[0].0);
    assert_eq!(resolver.resolve(ids[0].1).unwrap(), ids[0].0);
    assert_eq!(reads.load(Ordering::Relaxed), 1);
}

#[test]
fn indexed_memo_is_bounded_and_misses_still_use_the_store() {
    let (_dir, store, reads, ids) = aliased_store();
    let resolver = Canonicalizer::new(&store, None);
    assert_eq!(resolver.resolve(ids[0].1).unwrap(), ids[0].0);
    for id in 1_000_000_000_000..1_000_000_000_600 {
        assert_eq!(resolver.resolve(id).unwrap(), id);
    }
    assert_eq!(resolver.indexed_cache.as_ref().unwrap().borrow().len(), 512);
    assert_eq!(resolver.resolve(ids[1].1).unwrap(), ids[1].0);
    assert_eq!(resolver.resolve(ids[1].1).unwrap(), ids[1].0);
    assert_eq!(
        reads.load(Ordering::Relaxed),
        3,
        "uncached alias must still resolve"
    );
    let next_leaf = Canonicalizer::new(&store, None);
    assert_eq!(next_leaf.resolve(ids[0].1).unwrap(), ids[0].0);
    assert_eq!(
        reads.load(Ordering::Relaxed),
        4,
        "no mapping crosses a leaf lifetime"
    );
}

#[test]
fn complete_alias_maps_and_main_identity_keep_their_paths() {
    let (_dir, store, reads, ids) = aliased_store();
    let map = read_snapshot(&store.conn, &TemporalContext::default())
        .unwrap()
        .unwrap();
    assert!(!map.is_empty());
    assert!(reads.load(Ordering::Relaxed) > 0);
    reads.store(0, Ordering::Relaxed);
    let resolver = Canonicalizer::new(&store, Some(map));
    assert!(resolver.indexed_cache.is_none());
    assert_eq!(resolver.resolve(ids[0].1).unwrap(), ids[0].0);
    assert_eq!(reads.load(Ordering::Relaxed), 0);
    let main = Store::open_in_memory().unwrap();
    let identity = Canonicalizer::new(&main, None);
    assert!(identity.indexed_cache.is_none());
    assert_eq!(identity.resolve(77).unwrap(), 77);
}

pub(super) fn preceding_snapshot(
    connection: &rusqlite::Connection,
    ctx: &TemporalContext,
) -> Result<Option<HashMap<i64, i64>>> {
    let mut statement =
        connection.prepare("SELECT alias_id, canonical_id FROM term_alias LIMIT 4097")?;
    let mut rows = statement.query([])?;
    let mut aliases = HashMap::new();
    while let Some(row) = rows.next()? {
        check_eval_budget(ctx, aliases.len(), 0)?;
        aliases.insert(row.get(0)?, row.get(1)?);
        if aliases.len() > 4096 {
            return Ok(None);
        }
    }
    Ok(Some(aliases))
}

#[test]
fn preceding_mode_restores_after_unwind() {
    assert!(!PRECEDING.get());
    let result = std::panic::catch_unwind(|| {
        with_preceding(|| {
            assert!(PRECEDING.get());
            panic!("owned preceding-mode unwind control");
        })
    });
    assert!(result.is_err());
    assert!(!PRECEDING.get());
}

#[test]
#[ignore = "instrumented alias snapshot demonstration; run explicitly"]
fn alias_snapshot_demo() {
    let (connection, decoded) = fixture(4097);
    for mode in ["preceding", "candidate"] {
        decoded.store(0, Ordering::Relaxed);
        let start = std::time::Instant::now();
        let snapshot = if mode == "preceding" {
            preceding_snapshot(&connection, &TemporalContext::default())
        } else {
            read_snapshot(&connection, &TemporalContext::default())
        }
        .unwrap();
        let elapsed = start.elapsed().as_micros();
        assert!(snapshot.is_none());
        let count = decoded.load(Ordering::Relaxed);
        assert_eq!(count, if mode == "preceding" { 4097 } else { 0 });
        eprintln!(
            "ALIAS_SNAPSHOT_DEMO mode={mode} aliases=4097 declined=true decoded={count} elapsed_us={elapsed}"
        );
    }
    let (boundary, _) = fixture(4096);
    let before = preceding_snapshot(&boundary, &TemporalContext::default())
        .unwrap()
        .unwrap();
    let after = read_snapshot(&boundary, &TemporalContext::default())
        .unwrap()
        .unwrap();
    assert_eq!(before, after);
    assert_eq!(after.len(), 4096);
    eprintln!("ALIAS_SNAPSHOT_DEMO boundary=4096 complete=true mappings_equal=true");
    let (_dir, store, reads, ids) = aliased_store();
    let start = std::time::Instant::now();
    for _ in 0..1000 {
        assert_eq!(store.canonical_id(ids[0].1).unwrap(), ids[0].0);
    }
    let elapsed = start.elapsed().as_micros();
    assert_eq!(reads.load(Ordering::Relaxed), 1000);
    eprintln!(
        "ALIAS_SNAPSHOT_DEMO mode=preceding_indexed rows=1000 lookups=1000 elapsed_us={elapsed} mapping_correct=true"
    );
    reads.store(0, Ordering::Relaxed);
    let resolver = Canonicalizer::new(&store, None);
    let start = std::time::Instant::now();
    for _ in 0..1000 {
        assert_eq!(resolver.resolve(ids[0].1).unwrap(), ids[0].0);
    }
    let elapsed = start.elapsed().as_micros();
    assert_eq!(reads.load(Ordering::Relaxed), 1);
    eprintln!(
        "ALIAS_SNAPSHOT_DEMO mode=memoized_indexed rows=1000 lookups=1 elapsed_us={elapsed} mapping_correct=true"
    );
}
