use super::*;

#[test]
fn registry_open_does_not_canonicalize_historical_rows() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    for table in ["shapes", "ontologies"] {
        conn.execute_batch(&format!("CREATE TABLE {table}(name TEXT,turtle TEXT,valid_from TEXT,valid_to TEXT,tx INTEGER); INSERT INTO {table} VALUES('old','<urn:s> <urn:p> <urn:o> .','before','after',7);")).unwrap();
    }
    migrate_registry_digests(&conn).unwrap();
    let computed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM shapes WHERE digest IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        computed, 0,
        "opening must not canonicalize historical Turtle"
    );
}

fn legacy(store: &crate::Store, name: &str, text: &str) {
    store.conn.execute("INSERT INTO shapes(name,turtle,loaded_at,valid_from,tx) VALUES(?1,?2,'before','before',7)", rusqlite::params![name,text]).unwrap();
}

#[test]
fn pending_pins_fail_closed_then_exact_identity_and_history_survive() {
    let store = crate::Store::open_in_memory().unwrap();
    legacy(&store, "one", "<urn:s> <urn:p> <urn:o> .");
    assert!(
        store
            .current_shape_pins(RegistryKind::Shapes, "one")
            .is_err()
    );
    let work = store.next_registry_digest_work().unwrap().unwrap();
    assert!(store.apply_registry_digest_result(work.compute()).unwrap());
    let pins = store
        .current_shape_pins(RegistryKind::Shapes, "one")
        .unwrap();
    assert_eq!(
        pins[0].digest,
        canonical_shape_digest("<urn:s> <urn:p> <urn:o> .").unwrap()
    );
    assert_eq!(pins[0].valid_from, "before");
    let unchanged: (String, i64) = store
        .conn
        .query_row("SELECT turtle,tx FROM shapes WHERE name='one'", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(unchanged, ("<urn:s> <urn:p> <urn:o> .".into(), 7));
    assert!(store.next_registry_digest_work().unwrap().is_none());
}

#[test]
fn progress_and_failed_versions_persist_without_recanonicalizing_on_restart() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("registry.db");
    let path = path.to_str().unwrap();
    {
        let store = crate::Store::open(path).unwrap();
        legacy(&store, "a-valid", "<urn:s> <urn:p> <urn:o> .");
        legacy(&store, "z-invalid", "not RDF");
        // Crash before apply: a computed identity alone is not progress.
        let _unapplied = store
            .next_registry_digest_work()
            .unwrap()
            .unwrap()
            .compute();
    }
    {
        let store = crate::Store::open(path).unwrap();
        assert_eq!(store.registry_digest_backfill_status().unwrap().pending, 2);
        for _ in 0..2 {
            let work = store.next_registry_digest_work().unwrap().unwrap();
            assert!(store.apply_registry_digest_result(work.compute()).unwrap());
        }
    }
    let store = crate::Store::open(path).unwrap();
    assert!(store.next_registry_digest_work().unwrap().is_none());
    assert_eq!(
        store.registry_digest_backfill_status().unwrap(),
        RegistryDigestBackfillStatus {
            pending: 0,
            complete: 1,
            failed: 1
        }
    );
    assert!(
        store
            .current_shape_pins(RegistryKind::Shapes, "z-invalid")
            .is_err()
    );
}

#[test]
fn stale_same_version_replacements_and_rolled_back_progress_do_not_escape() {
    let store = crate::Store::open_in_memory().unwrap();
    legacy(&store, "one", "<urn:s> <urn:p> <urn:old> .");
    let stale = store
        .next_registry_digest_work()
        .unwrap()
        .unwrap()
        .compute();
    store
        .conn
        .execute(
            "UPDATE shapes SET turtle='<urn:s> <urn:p> <urn:new> .' WHERE name='one'",
            [],
        )
        .unwrap();
    assert!(!store.apply_registry_digest_result(stale).unwrap());
    let work = store.next_registry_digest_work().unwrap().unwrap();
    store.conn.execute_batch("BEGIN").unwrap();
    assert!(store.apply_registry_digest_result(work.compute()).unwrap());
    store.conn.execute_batch("ROLLBACK").unwrap();
    assert!(
        store
            .current_shape_pins(RegistryKind::Shapes, "one")
            .is_err()
    );
    let work = store.next_registry_digest_work().unwrap().unwrap();
    assert!(store.apply_registry_digest_result(work.compute()).unwrap());
    assert_eq!(
        store
            .current_shape_pins(RegistryKind::Shapes, "one")
            .unwrap()[0]
            .digest,
        canonical_shape_digest("<urn:s> <urn:p> <urn:new> .").unwrap()
    );
}

#[test]
fn new_registry_edit_keeps_its_identity_when_old_work_finishes_late() {
    let mut store = crate::Store::open_in_memory().unwrap();
    let timestamp = "2026-11-09T00:00:00Z";
    store
        .load_shapes("one", "<urn:s> <urn:p> <urn:old> .", timestamp)
        .unwrap();
    store
        .conn
        .execute("UPDATE shapes SET digest=NULL WHERE name='one'", [])
        .unwrap();
    let old = store
        .next_registry_digest_work()
        .unwrap()
        .unwrap()
        .compute();
    store
        .load_shapes("one", "<urn:s> <urn:p> <urn:new> .", timestamp)
        .unwrap();
    assert!(!store.apply_registry_digest_result(old).unwrap());
    assert_eq!(
        store
            .current_shape_pins(RegistryKind::Shapes, "one")
            .unwrap()[0]
            .digest,
        canonical_shape_digest("<urn:s> <urn:p> <urn:new> .").unwrap()
    );
    assert!(store.next_registry_digest_work().unwrap().is_none());
}

#[test]
fn oversized_valid_legacy_document_is_bounded_and_explicitly_unpinnable() {
    let store = crate::Store::open_in_memory().unwrap();
    let text = format!("<urn:s> <urn:p> <urn:o> .{}", " ".repeat(1 << 20));
    assert!(
        canonical_shape_digest(&text).is_ok(),
        "positive valid RDF control"
    );
    legacy(&store, "big", &text);
    let work = store.next_registry_digest_work().unwrap().unwrap();
    assert!(store.apply_registry_digest_result(work.compute()).unwrap());
    let status = store.registry_digest_backfill_status().unwrap();
    assert_eq!((status.pending, status.failed), (0, 1));
    assert!(
        store
            .current_shape_pins(RegistryKind::Shapes, "big")
            .is_err()
    );
    assert!(store.next_registry_digest_work().unwrap().is_none());
}

#[test]
fn both_current_registries_are_processed_before_shape_history() {
    let store = crate::Store::open_in_memory().unwrap();
    legacy(&store, "a-history", "<urn:s> <urn:p> <urn:o> .");
    store
        .conn
        .execute(
            "UPDATE shapes SET valid_to='after' WHERE name='a-history'",
            [],
        )
        .unwrap();
    store.conn.execute("INSERT INTO ontologies(name,turtle,loaded_at,valid_from,tx) VALUES('current','<urn:s> <urn:p> <urn:o> .','before','before',7)",[]).unwrap();
    let work = store.next_registry_digest_work().unwrap().unwrap();
    assert!(store.apply_registry_digest_result(work.compute()).unwrap());
    assert!(
        store
            .current_shape_pins(RegistryKind::Ontologies, "current")
            .is_ok()
    );
    let still_null: bool = store
        .conn
        .query_row(
            "SELECT digest IS NULL FROM shapes WHERE name='a-history'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        still_null,
        "history must wait for current identities in both registries"
    );
}
