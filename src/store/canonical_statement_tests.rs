//! A prepared VM must never become a cache of canonical term mappings.
use super::{Store, attach::Attachment};
use rusqlite::params;

fn composed() -> (tempfile::TempDir, Store) {
    let directory = tempfile::tempdir().unwrap();
    let main = directory.path().join("main.db");
    let source = directory.path().join("layer-source.db");
    let layer = directory.path().join("layer.db");
    {
        let store = Store::open(&main.to_string_lossy()).unwrap();
        store.intern("urn:canonical-test:shared").unwrap();
        store.intern("urn:canonical-test:other").unwrap();
        let store = Store::open(&source.to_string_lossy()).unwrap();
        store.intern("urn:canonical-test:shared").unwrap();
    }
    super::respace::respace_file(&source, &layer, 31).unwrap();
    let store = Store::open_with_attachments(
        &main.to_string_lossy(),
        &[Attachment::read_only("layer", &layer.to_string_lossy())],
    )
    .unwrap();
    (directory, store)
}

#[test]
fn canonical_statement_observes_updates_and_rollback() {
    let (_directory, store) = composed();
    let (alias, original): (i64, i64) = store.conn.query_row(
        "SELECT alias_id, canonical_id FROM temp.term_alias WHERE alias_id != canonical_id LIMIT 1",
        [], |r| Ok((r.get(0)?, r.get(1)?)),
    ).unwrap();
    let other = store.intern("urn:canonical-test:other").unwrap();
    assert_ne!(original, other);
    for _ in 0..20 {
        assert_eq!(store.canonical_id(alias).unwrap(), original);
    }
    store
        .conn
        .execute_batch("SAVEPOINT mapping_change")
        .unwrap();
    store
        .conn
        .execute(
            "UPDATE temp.term_alias SET canonical_id=?1 WHERE alias_id=?2",
            params![other, alias],
        )
        .unwrap();
    assert_eq!(store.canonical_id(alias).unwrap(), other);
    store
        .conn
        .execute_batch("ROLLBACK TO mapping_change; RELEASE mapping_change")
        .unwrap();
    assert_eq!(store.canonical_id(alias).unwrap(), original);
}

#[test]
fn canonical_statement_does_not_cache_misses_or_deleted_mappings() {
    let (_directory, store) = composed();
    let missing = i64::MAX - 17;
    let target = store.intern("urn:canonical-test:shared").unwrap();
    assert_eq!(store.canonical_id(missing).unwrap(), missing);
    store
        .conn
        .execute(
            "INSERT INTO temp.term_alias(alias_id, canonical_id) VALUES(?1, ?2)",
            params![missing, target],
        )
        .unwrap();
    assert_eq!(store.canonical_id(missing).unwrap(), target);
    store
        .conn
        .execute(
            "DELETE FROM temp.term_alias WHERE alias_id=?1",
            params![missing],
        )
        .unwrap();
    assert_eq!(store.canonical_id(missing).unwrap(), missing);
    // Schema errors remain errors, rather than reusing the previous miss.
    store
        .conn
        .execute_batch("DROP TABLE temp.term_alias")
        .unwrap();
    assert!(store.canonical_id(missing).is_err());
}

#[test]
fn canonical_statement_keeps_unattached_fast_path() {
    let store = Store::open_in_memory().unwrap();
    store
        .conn
        .execute_batch("DROP TABLE IF EXISTS temp.term_alias")
        .unwrap();
    assert_eq!(store.canonical_id(123).unwrap(), 123);
    assert_eq!(store.canonical_id(-1).unwrap(), -1);
}
