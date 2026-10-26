use super::*;
use serde_json::json;

fn legacy_file() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    let old = rusqlite::Connection::open(&path).unwrap();
    old.execute_batch(include_str!("fixtures/pre_scope_defaults.sql"))
        .unwrap();
    drop(old);
    (dir, path)
}

#[test]
fn legacy_store_without_scope_keeps_root_compatibility() {
    let (_dir, path) = legacy_file();
    Store::adopt(path.to_str().unwrap()).unwrap();
    let store = Store::open(path.to_str().unwrap()).unwrap();
    store.graph_create("urn:legacy:named").unwrap();
    // The actual old schema, not a freshly-created candidate store, preserves
    // its former semantic default until an operator explicitly broadens it.
    let scope = GraphScope::parse(&store, &json!({})).unwrap();
    assert_eq!(scope.ids, vec![0]);
}

#[test]
fn full_backup_and_restore_preserve_both_store_defaults() {
    for legacy in [false, true] {
        let (dir, path) = legacy_file();
        if !legacy {
            std::fs::remove_file(&path).unwrap();
        }
        if legacy {
            Store::adopt(path.to_str().unwrap()).unwrap();
        }
        let mut source = Store::open(path.to_str().unwrap()).unwrap();
        crate::rdf::ingest_rdf(
            &mut source,
            &b"<urn:backup:positive> <urn:backup:p> \"retained\" ."[..],
            oxrdfio::RdfFormat::NTriples,
            None,
            "2026-01-01T00:00:00Z",
            None,
            None,
        )
        .unwrap();
        let pack = dir.path().join("full.pendant");
        crate::pack_full::pack_full(
            &source,
            pack.to_str().unwrap(),
            &crate::pack::PackOptions {
                destination: crate::share_scrub::ShareDestination::Internal,
                ..Default::default()
            },
            "2026-01-01T00:00:00Z",
        )
        .unwrap();
        let destination = dir.path().join("restored.db");
        crate::pack_restore::restore(pack.to_str().unwrap(), destination.to_str().unwrap(), false)
            .unwrap();
        let restored = Store::open(destination.to_str().unwrap()).unwrap();
        assert_eq!(
            restored.default_search_graph_scope().unwrap(),
            source.default_search_graph_scope().unwrap()
        );
        assert!(restored.lookup("urn:backup:positive").unwrap().is_some());
    }
}

#[test]
fn same_unversioned_configuration_uses_store_intent_not_file_age() {
    let config: crate::config::SearchConfig =
        toml::from_str("named_graphs=false\nrequest_timeout_ms=10000").unwrap();
    assert!(config.default_graph_scope.is_none());
    let mut fresh = Store::open_in_memory().unwrap();
    fresh.graph_create("urn:fresh:named").unwrap();
    *fresh.search_config_mut() = config.clone();
    assert_eq!(
        GraphScope::parse_keyword(&fresh, &json!({}))
            .unwrap()
            .ids
            .len(),
        2
    );
    assert!(GraphScope::parse(&fresh, &json!({})).is_err());
    let (_dir, path) = legacy_file();
    Store::adopt(path.to_str().unwrap()).unwrap();
    let mut old = Store::open(path.to_str().unwrap()).unwrap();
    old.graph_create("urn:old:named").unwrap();
    *old.search_config_mut() = config;
    let scope = GraphScope::parse(&old, &json!({})).unwrap();
    assert_eq!(scope.ids, vec![0]);
    assert_eq!(
        scope.coverage(&old, &json!({}))["default_source"],
        "store_legacy_compatibility"
    );
}

#[test]
fn explicit_scope_wins_but_never_enables_unprepared_named_vectors() {
    let (_dir, path) = legacy_file();
    Store::adopt(path.to_str().unwrap()).unwrap();
    let mut old = Store::open(path.to_str().unwrap()).unwrap();
    old.graph_create("urn:old:named").unwrap();
    old.search_config_mut().default_graph_scope = Some("all".into());
    assert_eq!(
        GraphScope::parse_keyword(&old, &json!({}))
            .unwrap()
            .ids
            .len(),
        2
    );
    assert!(GraphScope::parse(&old, &json!({})).is_err());
    old.search_config_mut().named_graphs = true;
    let broad = GraphScope::parse(&old, &json!({})).unwrap();
    assert_eq!(broad.ids.len(), 2);
    assert_eq!(
        broad.coverage(&old, &json!({}))["default_source"],
        "configuration"
    );
    let mut fresh = Store::open_in_memory().unwrap();
    fresh.graph_create("urn:fresh:named").unwrap();
    fresh.search_config_mut().default_graph_scope = Some("root".into());
    assert_eq!(GraphScope::parse(&fresh, &json!({})).unwrap().ids, vec![0]);
}

#[test]
fn reopening_preserves_both_default_and_identity_without_changing_facts() {
    for legacy in [false, true] {
        let (dir, path) = if legacy {
            legacy_file()
        } else {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("new.db");
            (dir, path)
        };
        if legacy {
            Store::adopt(path.to_str().unwrap()).unwrap();
        }
        let (id, facts) = {
            let store = Store::open(path.to_str().unwrap()).unwrap();
            store.graph_create("urn:reopen:named").unwrap();
            let id: String = store
                .conn
                .query_row("SELECT store_id FROM store_identity WHERE id=1", [], |r| {
                    r.get(0)
                })
                .unwrap();
            let facts: i64 = store
                .conn
                .query_row("SELECT count(*) FROM facts", [], |r| r.get(0))
                .unwrap();
            (id, facts)
        };
        let store = Store::open(path.to_str().unwrap()).unwrap();
        let actual_id: String = store
            .conn
            .query_row("SELECT store_id FROM store_identity WHERE id=1", [], |r| {
                r.get(0)
            })
            .unwrap();
        let actual_facts: i64 = store
            .conn
            .query_row("SELECT count(*) FROM facts", [], |r| r.get(0))
            .unwrap();
        assert_eq!((actual_id, actual_facts), (id, facts));
        assert_eq!(
            GraphScope::parse_keyword(&store, &json!({}))
                .unwrap()
                .ids
                .len(),
            if legacy { 1 } else { 2 }
        );
        drop(store);
        drop(dir);
    }
}

#[test]
fn readonly_and_immutable_legacy_opens_do_not_migrate_or_change_bytes() {
    let (dir, path) = legacy_file();
    let before = std::fs::read(&path).unwrap();
    for immutable in [false, true] {
        let store = if immutable {
            Store::open_immutable(path.to_str().unwrap()).unwrap()
        } else {
            Store::open_read_only(path.to_str().unwrap()).unwrap()
        };
        let scope = GraphScope::parse(&store, &json!({})).unwrap();
        assert_eq!(scope.ids, vec![0]);
        assert_eq!(
            scope.coverage(&store, &json!({}))["default_source"],
            "legacy_unmarked_store"
        );
        let exists: bool = store
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='search_default_policy')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!exists);
        drop(store);
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn immutable_modern_store_retains_all_without_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("modern.db");
    {
        let store = Store::open(path.to_str().unwrap()).unwrap();
        store.graph_create("urn:modern:named").unwrap();
        store.checkpoint_wal_truncate().unwrap();
    }
    let before = std::fs::read(&path).unwrap();
    let count = std::fs::read_dir(dir.path()).unwrap().count();
    let store = Store::open_immutable(path.to_str().unwrap()).unwrap();
    let scope = GraphScope::parse_keyword(&store, &json!({})).unwrap();
    assert_eq!(scope.ids.len(), 2);
    assert_eq!(
        scope.coverage(&store, &json!({}))["default_source"],
        "store_default"
    );
    drop(store);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), count);
}

#[test]
fn missing_or_invalid_policy_refuses_instead_of_rechoosing_a_default() {
    for corrupt in [
        "DELETE FROM search_default_policy",
        "PRAGMA ignore_check_constraints=ON; UPDATE search_default_policy SET policy='private-sentinel'",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("corrupt.db");
        {
            let store = Store::open(path.to_str().unwrap()).unwrap();
            store.conn.execute_batch(corrupt).unwrap();
        }
        let reader = Store::open_read_only(path.to_str().unwrap()).unwrap();
        let error = GraphScope::parse_keyword(&reader, &json!({}))
            .err()
            .unwrap()
            .to_string();
        assert!(!error.contains("private-sentinel"));
        drop(reader);
        assert!(Store::open(path.to_str().unwrap()).is_err());
    }
}
