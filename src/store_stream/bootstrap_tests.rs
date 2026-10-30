use super::*;
use crate::Store;

#[test]
fn shared_applier_logical_bootstrap_preserves_stored_ask_dataset_and_history() {
    use crate::applier::{Applier, Outcome};
    use crate::store::{
        Datum,
        datasets::DatasetMember,
        queries::{StoredParam, StoredQuery},
    };
    use crate::store_replica::{FullStoreManifest, SnapshotTrust, manifest_hash, sign_snapshot};
    use crate::types::{Op, Value};
    let mut source = Store::open_in_memory().unwrap();
    let graph = source.graph_create("urn:public:board").unwrap();
    let entity = source.intern("urn:public:item").unwrap();
    let attribute = source.intern("urn:public:status").unwrap();
    let datum = |value: &str, op, from: &str| Datum {
        entity,
        attribute,
        value: Value::Str(value.into()),
        valid_from: from.into(),
        valid_to: None,
        op,
    };
    source
        .transact_to_graph(
            &[datum("open", Op::Assert, "2020-01-01T00:00:00Z")],
            "2026-01-01T00:00:00Z",
            None,
            None,
            graph,
        )
        .unwrap();
    source
        .transact_to_graph(
            &[
                datum("open", Op::Retract, "2022-01-01T00:00:00Z"),
                datum("closed", Op::Assert, "2022-01-01T00:00:00Z"),
            ],
            "2026-01-02T00:00:00Z",
            None,
            None,
            graph,
        )
        .unwrap();
    source
        .dataset_create(
            "urn:public:dataset",
            &[DatasetMember::new("urn:public:board")],
            "2026-01-03T00:00:00Z",
            None,
        )
        .unwrap();
    let query = StoredQuery {
        name: "logical_status".into(),
        description: "scoped status".into(),
        template: "SELECT ?value WHERE { <{entity}> <urn:public:status> ?value }".into(),
        dataset: Some("urn:public:dataset".into()),
        params: vec![StoredParam {
            name: "entity".into(),
            kind: "iri".into(),
            required: true,
            default: None,
            description: "selected item".into(),
        }],
    };
    source.query_load(&query, "2026-01-04T00:00:00Z").unwrap();
    // Paired baseline: the ordinary live-fact bootstrap cannot reproduce the
    // stored competency query/dataset contract. It remains a distinct profile.
    let folder = tempfile::tempdir().unwrap();
    let legacy_cut = crate::asof_share::cut(
        &source,
        &crate::asof_share::Request {
            graphs: vec!["ROOT".into(), "urn:public:board".into()],
            context_graphs: vec![],
            tx: None,
            expected_store_id: source.store_id().unwrap(),
        },
        folder.path(),
        &crate::asof_share::Limits {
            disk_floor_bytes: 0,
            ..Default::default()
        },
    )
    .unwrap();
    let records = std::fs::read_to_string(&legacy_cut.spool)
        .unwrap()
        .lines()
        .flat_map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap()["records"]
                .as_array()
                .unwrap()
                .clone()
        })
        .collect::<Vec<_>>();
    let mut legacy = Store::open_in_memory().unwrap();
    let mut legacy_applier = Applier::open(&legacy).unwrap();
    legacy_applier
        .apply_frame(
            &mut legacy,
            "quipu.asof",
            &serde_json::json!({
        "manifest":legacy_cut.manifest,"records":records})
            .to_string(),
            1,
        )
        .unwrap();
    assert!(legacy.query_get(&query.name).unwrap().is_none());
    initialize(&source).unwrap();
    let mut spool = Vec::new();
    let receipt = write_snapshot(&source, &mut spool, 1024 * 1024).unwrap();
    let manifest =
        FullStoreManifest::new(receipt.header, receipt.sha256, receipt.rows, receipt.bytes);
    let folder = tempfile::tempdir().unwrap();
    let key = crate::signing::SigningIdentity::load(&folder.path().join("fixture-key"), "fixture")
        .unwrap();
    let trust = SnapshotTrust {
        public_key_hex: key.public_key_hex(),
        source: manifest.header.source.clone(),
        journal_generation: manifest.header.journal.generation.clone(),
        catalog_digest: manifest.header.journal.catalog_digest.clone(),
        manifest_sha256: manifest_hash(&manifest).unwrap(),
    };
    let mut replica = Store::open_in_memory().unwrap();
    let mut applier = Applier::open(&replica).unwrap();
    applier.require_logical_store(trust.clone()).unwrap();
    let signature = sign_snapshot(&manifest, &key).unwrap();
    let mut occupied = Store::open_in_memory().unwrap();
    let retained_graph = occupied
        .graph_create("urn:public:destination-owned")
        .unwrap();
    let mut occupied_applier = Applier::open(&occupied).unwrap();
    occupied_applier
        .require_logical_store(trust.clone())
        .unwrap();
    let before = occupied.conn.total_changes();
    assert!(
        occupied_applier
            .bootstrap_logical_store(
                &mut occupied,
                &manifest,
                &signature,
                spool
                    .split_inclusive(|b| *b == b'\n')
                    .map(|line| Ok(line.to_vec()))
            )
            .is_err()
    );
    assert_eq!(occupied.conn.total_changes(), before);
    assert_eq!(
        occupied
            .registered_graph_id("urn:public:destination-owned")
            .unwrap(),
        Some(retained_graph)
    );
    assert!(occupied_applier.progress().is_none());
    let original_terms: i64 = replica
        .conn
        .query_row("SELECT count(*) FROM terms", [], |r| r.get(0))
        .unwrap();
    let mut interrupted = spool.clone();
    interrupted.truncate(interrupted.len() - 3);
    assert!(
        applier
            .bootstrap_logical_store(
                &mut replica,
                &manifest,
                &signature,
                interrupted
                    .split_inclusive(|b| *b == b'\n')
                    .map(|line| Ok(line.to_vec()))
            )
            .is_err()
    );
    assert!(replica.conn.is_autocommit());
    assert!(applier.progress().is_none());
    assert!(applier.logical_revision().is_none());
    assert!(replica.query_get(&query.name).unwrap().is_none());
    let retained_terms: i64 = replica
        .conn
        .query_row("SELECT count(*) FROM terms", [], |r| r.get(0))
        .unwrap();
    assert_eq!(retained_terms, original_terms);
    assert!(matches!(
        applier
            .bootstrap_logical_store(
                &mut replica,
                &manifest,
                &signature,
                spool
                    .split_inclusive(|b| *b == b'\n')
                    .map(|line| Ok(line.to_vec()))
            )
            .unwrap(),
        Outcome::Bootstrapped { .. }
    ));
    assert_eq!(replica.query_get(&query.name).unwrap(), Some(query));
    let request =
        serde_json::json!({"name":"logical_status","params":{"entity":"urn:public:item"}});
    let expected = crate::mcp::named_query::tool_ask(&source, &request).unwrap();
    let before = replica.conn.total_changes();
    let actual = crate::mcp::named_query::tool_ask(&replica, &request).unwrap();
    assert_eq!(actual["rows"], expected["rows"]);
    assert!(!actual["rows"].as_array().unwrap().is_empty(), "{actual}");
    assert_eq!(
        replica.conn.total_changes(),
        before,
        "ask must not allocate persistent terms"
    );
    for table in &manifest.header.catalog {
        let fields = table
            .columns
            .iter()
            .map(|c| catalog::quote(&c.name))
            .collect::<Vec<_>>();
        let keys = table
            .key_indices()
            .iter()
            .map(|&i| fields[i].clone())
            .collect::<Vec<_>>();
        let sql = format!(
            "SELECT {} FROM {} ORDER BY {}",
            fields.join(","),
            catalog::quote(&table.name),
            keys.join(",")
        );
        let read = |s: &Store| {
            s.conn
                .prepare(&sql)
                .unwrap()
                .query_map([], |r| {
                    (0..fields.len())
                        .map(|i| r.get::<_, rusqlite::types::Value>(i))
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
        };
        assert_eq!(read(&source), read(&replica), "{}", table.name);
    }
    let fact_head = source.latest_tx_id().unwrap();
    let since = applier.logical_revision().unwrap();
    let mut changed_query = source.query_get("logical_status").unwrap().unwrap();
    changed_query.template = "SELECT ?state WHERE { <{entity}> <urn:public:status> ?state }".into();
    source
        .query_load(&changed_query, "2026-01-05T00:00:00Z")
        .unwrap();
    assert_eq!(source.latest_tx_id().unwrap(), fact_head);
    let tail = read_revision(&source, since, 100).unwrap();
    let mut broken = tail.clone();
    broken.changes.last_mut().unwrap().table = "unadmitted-table".into();
    let broken_signature = crate::store_replica::sign_revision(&broken, &key).unwrap();
    assert!(
        applier
            .apply_logical_revision(&mut replica, &broken, &broken_signature)
            .is_err()
    );
    assert_eq!(applier.logical_revision(), Some(since));
    assert_eq!(
        replica.query_get("logical_status").unwrap(),
        source.query_get("logical_status").unwrap().map(|mut q| {
            q.template = "SELECT ?value WHERE { <{entity}> <urn:public:status> ?value }".into();
            q
        })
    );
    let tail_signature = crate::store_replica::sign_revision(&tail, &key).unwrap();
    assert!(
        matches!(applier.apply_logical_revision(&mut replica,&tail,&tail_signature).unwrap(),Outcome::Applied{source_txs} if source_txs.is_empty())
    );
    assert_eq!(applier.logical_revision(), Some(tail.through));
    assert!(matches!(
        applier
            .apply_logical_revision(&mut replica, &tail, &tail_signature)
            .unwrap(),
        Outcome::Duplicate
    ));
    assert_eq!(
        replica.query_get("logical_status").unwrap(),
        Some(changed_query)
    );
    let changed_expected = crate::mcp::named_query::tool_ask(&source, &request).unwrap();
    let before = replica.conn.total_changes();
    let changed_actual = crate::mcp::named_query::tool_ask(&replica, &request).unwrap();
    assert_ne!(changed_actual["rows"], expected["rows"]);
    assert_eq!(changed_actual["rows"], changed_expected["rows"]);
    assert_eq!(replica.conn.total_changes(), before);
    let later = source.graph_create("urn:public:later").unwrap();
    source
        .transact_to_graph(
            &[datum("later", Op::Assert, "2023-01-01T00:00:00Z")],
            "2026-01-06T00:00:00Z",
            None,
            None,
            later,
        )
        .unwrap();
    let next = read_revision(&source, tail.through, 100).unwrap();
    let next_signature = crate::store_replica::sign_revision(&next, &key).unwrap();
    applier
        .apply_logical_revision(&mut replica, &next, &next_signature)
        .unwrap();
    assert_eq!(
        replica.registered_graph_id("urn:public:later").unwrap(),
        Some(later)
    );
    assert_eq!(
        replica.current_facts_in_graph(later).unwrap(),
        source.current_facts_in_graph(later).unwrap()
    );
    let mut reopened = Applier::open(&replica).unwrap();
    reopened.require_logical_store(trust).unwrap();
    assert_eq!(
        reopened.progress().unwrap().source_tx,
        source.latest_tx_id().unwrap()
    );
    assert!(matches!(
        reopened
            .apply_frame(&mut replica, "quipu.changes", "{}", 1)
            .unwrap(),
        Outcome::Refused { .. }
    ));
}
