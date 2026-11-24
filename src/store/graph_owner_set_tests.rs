use super::*;
use crate::{
    graph_catalog::{Budget, GraphSet},
    types::{Op, Value as TermValue},
};
use std::{sync::mpsc, time::Duration};

const DATA: &str = "urn:application";
const PROV: &str = "urn:provenance";

fn fixture(root: &Path, fail_second: bool) -> (GraphSetOwner, GraphSetBatch) {
    let path = root.join("application.db");
    {
        let store = Store::open(path.to_str().unwrap()).unwrap();
        store.graph_create(DATA).unwrap();
        store.graph_create(PROV).unwrap();
    }
    Store::adopt(path.to_str().unwrap()).unwrap();
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let route = Route {
        graph: DATA.into(),
        file_id: "application".into(),
        store_id: store.store_id().unwrap(),
        log_epoch: store.log_epoch().unwrap(),
        generation: 3,
        policy: "policy-v1".into(),
    };
    let datum = Datum {
        entity: store.intern("urn:item").unwrap(),
        attribute: store.intern("urn:value").unwrap(),
        value: TermValue::Str("present".into()),
        valid_from: "2026-01-01T00:00:00Z".into(),
        valid_to: None,
        op: Op::Assert,
    };
    if fail_second {
        let first = store.committed_graph_id(DATA).unwrap().unwrap();
        let second = store.committed_graph_id(PROV).unwrap().unwrap();
        // The error text positively proves that the first physical fact was
        // installed before the second graph failed inside the same savepoint.
        store
            .conn
            .execute_batch(&format!(
                "CREATE TRIGGER fail_second BEFORE INSERT ON facts WHEN NEW.g={second} BEGIN
             SELECT CASE WHEN EXISTS(SELECT 1 FROM facts WHERE g={first} AND op=1)
             THEN RAISE(ABORT,'second-member-after-first')
             ELSE RAISE(ABORT,'first-member-missing') END; END;"
            ))
            .unwrap();
    }
    let batch = GraphSetBatch {
        store_id: route.store_id.clone(),
        log_epoch: route.log_epoch.clone(),
        graphs: vec![
            (DATA.into(), vec![datum.clone()]),
            (PROV.into(), vec![datum]),
        ],
        timestamp: "2026-01-01T00:00:00Z".into(),
        actor: Some("fixture".into()),
        source: Some("fixture".into()),
    };
    let catalog = Arc::new(
        GraphSetCatalog::new(
            7,
            "legacy",
            vec![GraphSet {
                owner: route,
                graphs: vec![DATA.into(), PROV.into()],
            }],
            Budget {
                max_owners: 1,
                queued_per_owner: 1,
                readers_per_owner: 1,
                max_queued: 1,
                max_connections: 3,
            },
            2,
        )
        .unwrap(),
    );
    let allocation = catalog.physical().memory_allocation(320, 1, 1).unwrap();
    let owner = GraphSetOwner::prepare(catalog, root, DATA, store, "policy-v1", allocation);
    (owner.unwrap(), batch)
}

fn requests() -> Vec<WriteRequest<'static>> {
    [DATA, PROV]
        .into_iter()
        .map(|graph| WriteRequest {
            graph: Some(graph),
            catalog_revision: 7,
            owner_generation: Some(3),
            policy: "policy-v1",
            signed: false,
            cross_graph_context: false,
        })
        .collect()
}

fn counts(owner: &GraphSetOwner) -> Vec<i64> {
    owner
        .owner
        .read(|store| {
            [DATA, PROV]
                .iter()
                .map(|g| {
                    let graph = store.committed_graph_id(g)?.unwrap();
                    Ok(store.conn.query_row(
                        "SELECT count(*) FROM facts WHERE g=?1 AND op=1",
                        [graph],
                        |r| r.get(0),
                    )?)
                })
                .collect()
        })
        .unwrap()
}

#[test]
fn real_atomic_batch_returns_ordered_heads_only_after_both_graphs_commit() {
    let temp = tempfile::tempdir().unwrap();
    let (owner, batch) = fixture(temp.path(), false);
    assert_eq!(counts(&owner), [0, 0]);
    let heads = owner.transact(&requests(), &batch).unwrap();
    assert_eq!(
        heads.iter().map(|(g, _)| g.as_str()).collect::<Vec<_>>(),
        [DATA, PROV]
    );
    assert!(heads[0].1 < heads[1].1);
    assert_eq!(counts(&owner), [1, 1]);
}

#[test]
fn late_second_member_sql_failure_rolls_back_first_fact_and_transaction_heads() {
    let temp = tempfile::tempdir().unwrap();
    let (owner, batch) = fixture(temp.path(), true);
    let before = owner.owner.read(Store::latest_tx_id).unwrap();
    let error = owner.transact(&requests(), &batch).unwrap_err().to_string();
    assert!(
        error.contains("second-member-after-first"),
        "wrong failure boundary: {error}"
    );
    assert_eq!(counts(&owner), [0, 0]);
    assert_eq!(owner.owner.read(Store::latest_tx_id).unwrap(), before);
    // Failure releases mailbox and ownership scope; it cannot poison recovery.
    owner
        .owner
        .writer
        .lock()
        .conn
        .execute_batch("DROP TRIGGER fail_second")
        .unwrap();
    owner.transact(&requests(), &batch).unwrap();
    assert_eq!(counts(&owner), [1, 1]);
}

#[test]
fn held_root_sqlite_writer_does_not_delay_the_graph_set_commit_or_point_read() {
    let temp = tempfile::tempdir().unwrap();
    let root = Store::open(temp.path().join("root.db").to_str().unwrap()).unwrap();
    root.conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    let (owner, batch) = fixture(temp.path(), false);
    let owner = Arc::new(owner);
    let (tx, rx) = mpsc::channel();
    let child = {
        let owner = owner.clone();
        std::thread::spawn(move || {
            let result = owner.transact(&requests(), &batch);
            tx.send((result, counts(&owner))).unwrap();
        })
    };
    let result = rx.recv_timeout(Duration::from_secs(3));
    // Release only AFTER observing independent completion (also on failure so
    // a broken implementation cannot leave the fixture worker stuck forever).
    root.conn.execute_batch("ROLLBACK").unwrap();
    child.join().unwrap();
    let (heads, counts) = result.expect("graph set waited on unrelated ROOT writer");
    assert!(heads.is_ok());
    assert_eq!(counts, [1, 1]);
}

#[test]
fn wrong_lineage_epoch_member_or_request_pin_refuses_without_partial_data() {
    let temp = tempfile::tempdir().unwrap();
    let (owner, mut batch) = fixture(temp.path(), false);
    batch.log_epoch.push_str("-foreign");
    assert!(owner.transact(&requests(), &batch).is_err());
    batch.log_epoch = owner.owner.route.log_epoch.clone();
    batch.store_id.push_str("-foreign");
    assert!(owner.transact(&requests(), &batch).is_err());
    batch.store_id = owner.owner.route.store_id.clone();
    let mut stale = requests();
    stale[1].owner_generation = Some(2);
    assert!(owner.transact(&stale, &batch).is_err());
    let mut signed = requests();
    signed[1].signed = true;
    assert!(owner.transact(&signed, &batch).is_err());
    batch.graphs[1].0 = "urn:unselected".into();
    assert!(owner.transact(&requests(), &batch).is_err());
    assert_eq!(counts(&owner), [0, 0]);
}

#[test]
fn one_mailbox_is_shared_by_all_members_and_never_borrows_capacity() {
    let temp = tempfile::tempdir().unwrap();
    let (owner, batch) = fixture(temp.path(), false);
    let one = owner.owner.admit().unwrap();
    let two = owner.owner.admit().unwrap();
    let error = owner.transact(&requests(), &batch).unwrap_err().to_string();
    assert!(error.contains("mailbox full"));
    assert_eq!(counts(&owner), [0, 0]);
    drop((one, two));
    owner.transact(&requests(), &batch).unwrap();
    assert_eq!(counts(&owner), [1, 1]);
}

#[test]
fn automatic_embeddings_refuse_before_facts_instead_of_losing_deferred_work() {
    struct Provider;
    impl crate::EmbeddingProvider for Provider {
        fn embed_text(&self, _: &str) -> Result<Vec<f32>> {
            panic!("unqualified embedding provider executed")
        }
        fn dimension(&self) -> usize {
            1
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let (owner, batch) = fixture(temp.path(), false);
    {
        let mut store = owner.owner.writer.lock();
        store.embedding_config.auto_embed = true;
        store.embedding_provider = Some(Arc::new(Provider));
        assert!(GraphSetOwner::check_hooks(&store).is_err());
    }
    let error = owner.transact(&requests(), &batch).unwrap_err().to_string();
    assert!(error.contains("embedding completion"));
    assert_eq!(counts(&owner), [0, 0]);
    assert!(owner.owner.writer.lock().pending_embed.is_none());
}

#[cfg(feature = "reactive-reasoner")]
#[test]
fn reactive_observers_refuse_before_facts_instead_of_observing_uncommitted_members() {
    struct Observer;
    impl super::super::super::TransactObserver for Observer {
        fn after_commit(&self, _: &mut Store, _: &super::super::super::Delta) -> Result<()> {
            panic!("observer ran before qualified batch completion")
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let (owner, batch) = fixture(temp.path(), false);
    owner.owner.writer.lock().observers.push(Arc::new(Observer));
    let error = owner.transact(&requests(), &batch).unwrap_err().to_string();
    assert!(error.contains("reactive completion"));
    assert_eq!(counts(&owner), [0, 0]);
}
