//! The oracle is SQL over current facts, never the cached index builder.
use super::{LabelIndex, cache};
use crate::{
    namespace,
    store::{Datum, Store},
    types::{Op, Value},
};
use proptest::prelude::*;
use std::collections::HashSet;

fn oracle(store: &Store) -> HashSet<(i64, String)> {
    let labels = store
        .lookup_all(&format!("{}label", namespace::RDFS))
        .unwrap();
    if labels.is_empty() {
        return HashSet::new();
    }
    let (entity, join) = if store.attachments.is_empty() {
        ("f.e", String::new())
    } else {
        (
            "COALESCE(t.canonical_id,f.e)",
            " LEFT JOIN term_alias t ON t.alias_id=f.e".into(),
        )
    };
    let sql = format!(
        "SELECT {entity},f.v FROM (SELECT e,a,v,op,valid_to FROM {}) f{join} WHERE f.a IN ({}) AND f.op=1 AND f.valid_to IS NULL",
        store.facts_source(),
        super::id_list(&labels)
    );
    let mut stmt = store.conn.prepare(&sql).unwrap();
    stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))
        .unwrap()
        .filter_map(|r| {
            let (e, v) = r.unwrap();
            match Value::from_bytes(&v).unwrap() {
                Value::Str(s) => Some((e, s.to_lowercase())),
                _ => None,
            }
        })
        .collect()
}
fn entries(store: &Store) -> HashSet<(i64, String)> {
    LabelIndex::build(store)
        .unwrap()
        .entries
        .iter()
        .map(|e| (e.entity_id, e.lower.clone()))
        .collect()
}
fn write(store: &mut Store, n: u8, label: &str, op: Op) {
    let e = store.intern(&format!("urn:test:{n}")).unwrap();
    let a = store.intern(&format!("{}label", namespace::RDFS)).unwrap();
    store
        .transact(
            &[Datum {
                entity: e,
                attribute: a,
                value: Value::Str(label.into()),
                valid_from: "2026-01-01".into(),
                valid_to: None,
                op,
            }],
            "2026-01-01",
            None,
            Some("cache-test"),
        )
        .unwrap();
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn randomized_writes_match_independent_sql(ops in prop::collection::vec((0u8..7,0u8..8,"[a-zA-ZÀ-ÿΩΣ]{0,20}"),1..48)) {
        let mut store=Store::open_in_memory().unwrap();
        for n in 0..8 { write(&mut store,n,"Seed",Op::Assert); }
        store.prepare_resolution_index().unwrap();
        for (kind,entity,label) in ops {
            match kind {
                0|1=>write(&mut store,entity,&label,Op::Assert),
                2=>write(&mut store,entity,&label,Op::Retract),
                3=>{write(&mut store,entity,"Pre-set seed",Op::Assert);crate::tool_set(&mut store,&serde_json::json!({"entity":format!("urn:test:{entity}"),"predicate":format!("{}label",namespace::RDFS),"value":{"str":label}})).unwrap();},
                4=>{crate::tool_knot(&mut store,&serde_json::json!({"turtle":format!("<urn:test:{entity}> <{}sameAs> <urn:test:alias{entity}> .",namespace::OWL)})).unwrap();},
                5=>{
                    let previous=store.resolution_cache.lock().take();
                    write(&mut store,entity,&label,Op::Assert);
                    *store.resolution_cache.lock()=previous;
                    cache::maintain(&store,None,store.latest_tx_id().unwrap());
                    prop_assert!(store.resolution_cache.lock().is_none());
                },
                _=>{
                    let plan=crate::tool_retract_source(&mut store,&serde_json::json!({"source":"cache-test","repair":"private-cache-test"})).unwrap();
                    crate::tool_retract_source(&mut store,&serde_json::json!({"source":"cache-test","repair":"private-cache-test","apply":true,"expect":plan["planned"]})).unwrap();
                },
            }
            prop_assert_eq!(entries(&store),oracle(&store));
            store.prepare_resolution_index().unwrap();
            prop_assert_eq!(entries(&store),oracle(&store));
        }
    }
}
#[test]
fn pooled_reader_sees_writes_and_fallback_retains_no_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    let mut writer = Store::open(path.to_str().unwrap()).unwrap();
    write(&mut writer, 1, "Alice", Op::Assert);
    writer.prepare_resolution_index().unwrap();
    let mut reader = Store::open_read_only(path.to_str().unwrap()).unwrap();
    reader.adopt_read_config_from(&writer);
    let first = LabelIndex::build(&reader).unwrap();
    assert!(std::sync::Arc::ptr_eq(
        &first,
        &LabelIndex::build(&writer).unwrap()
    ));
    write(&mut writer, 2, "Bob", Op::Assert);
    assert_eq!(entries(&reader), oracle(&reader));
    assert_eq!(first.entries.len(), 1); // readers retain their immutable generation
    writer.resolution_config_mut().cache_labels = false;
    writer.prepare_resolution_index().unwrap();
    assert!(writer.resolution_cache.lock().is_none());
    assert_eq!(entries(&writer), oracle(&writer));
}
#[test]
fn unknown_commit_and_outer_rollback_never_publish_uncommitted_labels() {
    let mut store = Store::open_in_memory().unwrap();
    write(&mut store, 1, "Alice", Op::Assert);
    store.prepare_resolution_index().unwrap();
    store.conn.execute_batch("SAVEPOINT outer").unwrap();
    write(&mut store, 2, "Uncommitted", Op::Assert);
    assert_eq!(entries(&store), oracle(&store));
    store
        .conn
        .execute_batch("ROLLBACK TO outer; RELEASE outer")
        .unwrap();
    assert_eq!(entries(&store), oracle(&store));
    assert_eq!(entries(&store).len(), 1);
    cache::maintain(&store, None, store.latest_tx_id().unwrap());
    assert_eq!(entries(&store), oracle(&store));
}

fn composed_store() -> (Store, tempfile::TempDir, std::path::PathBuf) {
    use crate::store::attach::Attachment;
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.db");
    let src = dir.path().join("src.db");
    let layer = dir.path().join("layer.db");
    {
        let mut s = Store::open(main.to_str().unwrap()).unwrap();
        write(&mut s, 1, "Local", Op::Assert);
    }
    {
        let mut s = Store::open(src.to_str().unwrap()).unwrap();
        let e = s.intern("urn:test:1").unwrap();
        let a = s.intern(&format!("{}label", namespace::RDFS)).unwrap();
        let g = s.intern("urn:layer:graph").unwrap();
        s.conn
            .execute(
                "INSERT INTO graphs(g,class,created_at) VALUES(?1,'committed','2026-01-01')",
                [g],
            )
            .unwrap();
        s.transact_to_graph(
            &[Datum {
                entity: e,
                attribute: a,
                value: Value::Str("Packed".into()),
                valid_from: "2026-01-01".into(),
                valid_to: None,
                op: Op::Assert,
            }],
            "2026-01-01",
            None,
            None,
            g,
        )
        .unwrap();
    }
    crate::store::respace::respace_file(&src, &layer, 31).unwrap();
    let store = Store::open_with_attachments(
        main.to_str().unwrap(),
        &[Attachment::read_only("layer", layer.to_str().unwrap())],
    )
    .unwrap();
    (store, dir, layer)
}

#[test]
fn maintaining_a_local_alias_preserves_unchanged_labels_in_attached_graphs() {
    let (mut store, _dir, _layer) = composed_store();
    store.prepare_resolution_index().unwrap();
    assert_eq!(entries(&store).len(), 2);
    write(&mut store, 1, "Local", Op::Retract);
    assert_eq!(entries(&store), oracle(&store));
    assert_eq!(entries(&store).len(), 1);
    assert!(entries(&store).iter().any(|(_, label)| label == "packed"));
}

#[test]
fn known_delta_remains_resident_and_external_writer_cannot_hide_new_labels() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    let mut store = Store::open(path.to_str().unwrap()).unwrap();
    write(&mut store, 1, "Alice", Op::Assert);
    store.prepare_resolution_index().unwrap();
    write(&mut store, 2, "Bob", Op::Assert);
    assert_eq!(
        cache::resident_generation(&store),
        Some(store.latest_tx_id().unwrap())
    );
    assert_eq!(entries(&store), oracle(&store));
    let mut external = Store::open(path.to_str().unwrap()).unwrap();
    write(&mut external, 3, "New outside this handle", Op::Assert);
    assert_ne!(
        cache::resident_generation(&store),
        Some(store.latest_tx_id().unwrap())
    );
    assert_eq!(entries(&store), oracle(&store));
}

#[test]
fn configured_fallback_round_trips_and_reaches_pooled_readers() {
    let cfg: crate::config::QuipuConfig =
        toml::from_str("[resolution]\ncache_labels=false\n").unwrap();
    assert!(!cfg.resolution.cache_labels);
    let mut writer = Store::open_in_memory().unwrap();
    *writer.resolution_config_mut() = cfg.resolution;
    let mut reader = Store::open_in_memory().unwrap();
    reader.adopt_read_config_from(&writer);
    assert!(!reader.resolution_config().cache_labels);
    assert!(!reader.prepare_resolution_index().unwrap());
}

#[test]
fn externally_changed_read_only_provider_never_reuses_cached_labels() {
    let (store, _dir, layer) = composed_store();
    store.prepare_resolution_index().unwrap();
    assert!(entries(&store).iter().any(|(_, label)| label == "packed"));
    // mode=ro constrains this handle, not other writers of the mounted file.
    // This private fixture intentionally changes it without touching MAIN.
    let provider = rusqlite::Connection::open(layer).unwrap();
    provider
        .execute(
            "UPDATE facts SET v=?1 WHERE a IN (SELECT id FROM terms WHERE iri=?2) AND op=1",
            rusqlite::params![
                Value::Str("Provider changed".into()).to_bytes(),
                format!("{}label", namespace::RDFS)
            ],
        )
        .unwrap();
    assert_eq!(entries(&store), oracle(&store));
    assert!(
        entries(&store)
            .iter()
            .any(|(_, label)| label == "provider changed")
    );
    assert!(
        store.resolution_cache.lock().is_none(),
        "uncertified attached lifetimes must use exact scans"
    );
}
