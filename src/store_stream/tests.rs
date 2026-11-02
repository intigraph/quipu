use super::*;
use crate::Store;

#[test]
fn journal_is_explicit_and_does_not_self_initialize_on_a_read() {
    let store = Store::open_in_memory().unwrap();
    let error = read_revision(&store, 0, 100).unwrap_err();
    assert!(error.to_string().contains("not initialized"), "{error}");
    assert!(
        !store
            .conn
            .prepare("SELECT 1 FROM sqlite_schema WHERE name='store_replication_state'")
            .unwrap()
            .exists([])
            .unwrap()
    );
    let state = initialize(&store).unwrap();
    assert_eq!(state, initialize(&store).unwrap());
    assert_eq!(read_revision(&store, 0, 100).unwrap().through, 0);
}

#[test]
fn metadata_without_a_new_fact_transaction_is_journaled_and_rolls_back_with_its_writer() {
    let store = Store::open_in_memory().unwrap();
    initialize(&store).unwrap();
    let head = store.latest_tx_id().unwrap();
    store
        .conn
        .execute_batch(
            "SAVEPOINT private_change; \
        INSERT INTO terms(iri) VALUES('urn:private:rolled-back'); \
        ROLLBACK TO private_change; RELEASE private_change",
        )
        .unwrap();
    assert_eq!(read_revision(&store, 0, 100).unwrap().through, 0);
    store
        .conn
        .execute("INSERT INTO terms(iri) VALUES('urn:private:committed')", [])
        .unwrap();
    let batch = read_revision(&store, 0, 100).unwrap();
    assert_eq!(batch.source_tx, head);
    assert_eq!(batch.changes.len(), 1);
    assert_eq!(batch.changes[0].table, "terms");
    assert_eq!(batch.changes[0].operation, "put");
    assert_eq!(
        batch.changes[0].row.as_ref().unwrap()[1].value,
        "urn:private:committed"
    );
    assert!(read_revision(&store, 0, 0).is_err());
    assert!(
        journal::read_bounded(&store, 0, 100, 1)
            .unwrap_err()
            .to_string()
            .contains("byte cap")
    );
    assert_eq!(
        read_revision(&store, 0, 100).unwrap().through,
        batch.through
    );
}

#[test]
fn actual_stored_query_load_replace_and_remove_preserve_metadata_history() {
    use crate::store::queries::{StoredParam, StoredQuery};
    let store = Store::open_in_memory().unwrap();
    initialize(&store).unwrap();
    let head = store.latest_tx_id().unwrap();
    let mut query = StoredQuery {
        name: "private_journal_probe".into(),
        description: "first definition".into(),
        template: "SELECT ?s WHERE { ?s <urn:public:predicate> <{entity}> }".into(),
        dataset: None,
        params: vec![StoredParam {
            name: "entity".into(),
            kind: "iri".into(),
            required: true,
            default: None,
            description: "selected entity".into(),
        }],
    };
    store.query_load(&query, "2026-01-01T00:00:00Z").unwrap();
    let first = read_revision(&store, 0, 100).unwrap();
    assert_eq!(first.source_tx, head);
    assert!(first.changes.iter().any(|c| c.table == "queries"));
    assert!(first.changes.iter().any(|c| c.table == "query_params"));
    query.description = "replacement definition".into();
    store.query_load(&query, "2026-01-02T00:00:00Z").unwrap();
    assert_eq!(store.query_get(&query.name).unwrap(), Some(query.clone()));
    let second = read_revision(&store, first.through, 100).unwrap();
    assert_eq!(second.source_tx, head);
    assert!(
        second
            .changes
            .iter()
            .any(|c| c.table == "queries" && c.operation == "delete")
    );
    assert!(
        store
            .query_remove(&query.name, "2026-01-03T00:00:00Z")
            .unwrap()
    );
    assert!(store.query_get(&query.name).unwrap().is_none());
    let third = read_revision(&store, second.through, 100).unwrap();
    assert_eq!(third.source_tx, head);
    assert!(!third.changes.is_empty());
    let definitions: i64 = store
        .conn
        .query_row(
            "SELECT count(*) FROM queries WHERE name=?1 AND valid_to IS NOT NULL",
            [&query.name],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(definitions, 2);
}

#[test]
fn unknown_schema_and_missing_triggers_refuse_instead_of_omitting_changes() {
    let store = Store::open_in_memory().unwrap();
    initialize(&store).unwrap();
    store
        .conn
        .execute_batch("DROP TRIGGER qsr_terms_insert")
        .unwrap();
    assert!(
        read_revision(&store, 0, 100)
            .unwrap_err()
            .to_string()
            .contains("trigger")
    );
    let other = Store::open_in_memory().unwrap();
    other
        .conn
        .execute_batch("CREATE TABLE future_data(id INTEGER PRIMARY KEY, value TEXT)")
        .unwrap();
    assert!(
        initialize(&other)
            .unwrap_err()
            .to_string()
            .contains("future_data")
    );
}

#[test]
fn logical_snapshot_has_one_cut_for_more_than_64_graphs_and_a_later_graph_tail() {
    use crate::store_replica::{SnapshotHeader, SnapshotRow};
    use sha2::{Digest, Sha256};
    let store = Store::open_in_memory().unwrap();
    for i in 0..70 {
        store
            .graph_create(&format!("urn:public:graph:{i}"))
            .unwrap();
    }
    initialize(&store).unwrap();
    let before = store.conn.total_changes();
    let mut bytes = Vec::new();
    let receipt = write_snapshot(&store, &mut bytes, 1024 * 1024).unwrap();
    assert_eq!(
        store.conn.total_changes(),
        before,
        "snapshot must only read"
    );
    assert_eq!(receipt.bytes, bytes.len() as u64);
    assert_eq!(receipt.sha256, hex::encode(Sha256::digest(&bytes)));
    let text = String::from_utf8(bytes).unwrap();
    let mut lines = text.lines();
    let header: SnapshotHeader = serde_json::from_str(lines.next().unwrap()).unwrap();
    assert_eq!(header.revision, receipt.header.revision);
    assert_eq!(header.source_tx, store.latest_tx_id().unwrap());
    assert!(
        header
            .catalog
            .iter()
            .all(|t| t.name != "ownership_enforcement")
    );
    let rows = lines
        .map(|line| serde_json::from_str::<SnapshotRow>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(receipt.rows, rows.len() as u64);
    assert!(rows.iter().filter(|r| r.table == "graphs").count() >= 70);
    let source_graphs: i64 = store
        .conn
        .query_row("SELECT count(*) FROM graphs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        rows.iter().filter(|r| r.table == "graphs").count() as i64,
        source_graphs
    );
    let later = store.graph_create("urn:public:graph:later").unwrap();
    let tail = read_revision(&store, header.revision, 100).unwrap();
    assert!(
        tail.changes
            .iter()
            .any(|c| c.table == "graphs" && c.row.as_ref().is_some_and(|r| r[0].value == later))
    );
}

#[test]
fn incomplete_snapshot_has_no_completion_receipt_and_releases_its_read_snapshot() {
    let store = Store::open_in_memory().unwrap();
    initialize(&store).unwrap();
    assert!(write_snapshot(&store, &mut Vec::new(), 1).is_err());
    assert!(store.conn.is_autocommit());
    struct BrokenWriter;
    impl std::io::Write for BrokenWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("private interrupted spool"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    assert!(write_snapshot(&store, &mut BrokenWriter, 1024 * 1024).is_err());
    assert!(store.conn.is_autocommit());
    assert!(write_snapshot(&store, &mut Vec::new(), 1024 * 1024).is_ok());
}

#[test]
fn completed_logical_cut_requires_exact_external_descriptor_and_source_key() {
    use crate::store_replica::{FullStoreManifest, SnapshotTrust, manifest_hash, sign_snapshot};
    let store = Store::open_in_memory().unwrap();
    initialize(&store).unwrap();
    let receipt = write_snapshot(&store, &mut Vec::new(), 1024 * 1024).unwrap();
    let manifest =
        FullStoreManifest::new(receipt.header, receipt.sha256, receipt.rows, receipt.bytes);
    let folder = tempfile::tempdir().unwrap();
    let key =
        crate::signing::SigningIdentity::load(&folder.path().join("fixture-source"), "fixture")
            .unwrap();
    let other =
        crate::signing::SigningIdentity::load(&folder.path().join("fixture-other"), "fixture")
            .unwrap();
    let trust = SnapshotTrust {
        public_key_hex: key.public_key_hex(),
        source: manifest.header.source.clone(),
        journal_generation: manifest.header.journal.generation.clone(),
        catalog_digest: manifest.header.journal.catalog_digest.clone(),
        manifest_sha256: manifest_hash(&manifest).unwrap(),
    };
    let signature = sign_snapshot(&manifest, &key).unwrap();
    assert!(trust.accepts(&manifest, &signature));
    assert!(!trust.accepts(&manifest, &sign_snapshot(&manifest, &other).unwrap()));
    assert!(!trust.accepts(&manifest, "malformed"));
    for field in ["epoch", "revision", "bytes", "spool", "catalog", "policy"] {
        let mut changed = manifest.clone();
        match field {
            "epoch" => changed.header.source.log_epoch.push_str("changed"),
            "revision" => changed.header.revision += 1,
            "bytes" => changed.bytes += 1,
            "spool" => changed.spool_sha256 = "0".repeat(64),
            "catalog" => changed.header.catalog[0].create_sql.push_str(" changed"),
            "policy" => changed.policy = "unsupported-profile".into(),
            _ => unreachable!(),
        }
        assert!(!trust.accepts(&changed, &signature), "{field}");
    }
    let mut policy = manifest.clone();
    policy.policy = "unsupported-profile".into();
    let mut new_pin = trust.clone();
    new_pin.manifest_sha256 = manifest_hash(&policy).unwrap();
    assert!(!new_pin.accepts(&policy, &sign_snapshot(&policy, &key).unwrap()));
}
