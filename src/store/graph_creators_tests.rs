use super::*;

const GRAPH: &str = "urn:fixture:durable-creator";
const TIME: &str = "2000-01-01T00:00:00Z";
const CUTOFF: &str = "2001-01-01T00:00:00Z";

#[test]
fn creator_survives_real_pruning_restart_and_repeat_without_retaining_events() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("store.db");
    let original;
    {
        let s = Store::open(path.to_str().unwrap()).unwrap();
        original = s
            .graph_create_recorded(GRAPH, TIME, Some("original"))
            .unwrap();
        assert!(s.prune_events(CUTOFF).unwrap() > 0);
        let events: i64 = s
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            events, 0,
            "retention still deletes eligible delivery history"
        );
        assert_eq!(
            s.graph_metadata(GRAPH).unwrap().unwrap()["creation"],
            original["creation"]
        );
    }
    let s = Store::open(path.to_str().unwrap()).unwrap();
    let repeat = s
        .graph_create_recorded(GRAPH, CUTOFF, Some("later-caller"))
        .unwrap();
    assert_eq!(repeat["created"], false);
    assert_eq!(repeat["creation"], original["creation"]);
    assert_eq!(
        s.graph_metadata(GRAPH).unwrap().unwrap()["creation"],
        original["creation"]
    );
}

#[test]
fn migration_backfills_retained_records_is_idempotent_and_keeps_lost_actors_unknown() {
    let s = Store::open_in_memory().unwrap();
    s.graph_create_recorded(GRAPH, TIME, Some("retained-original"))
        .unwrap();
    let lost = "urn:fixture:already-pruned";
    s.graph_create(lost).unwrap();
    s.conn.execute_batch("DROP TABLE graph_creators").unwrap();
    Store::migrate_graph_creators_v1(&s.conn).unwrap();
    let changes = s.conn.total_changes();
    Store::migrate_graph_creators_v1(&s.conn).unwrap();
    assert_eq!(s.conn.total_changes(), changes);
    s.prune_events(CUTOFF).unwrap();
    assert_eq!(
        s.graph_metadata(GRAPH).unwrap().unwrap()["creation"]["actor"],
        "retained-original"
    );
    assert!(
        s.graph_create_recorded(lost, CUTOFF, Some("not-the-creator"))
            .unwrap()["creation"]["actor"]
            .is_null()
    );
    s.conn.execute("INSERT INTO events(type,ts,subject,tx_id,payload) VALUES('graph.created',?1,?2,0,'{\"actor\":\"impostor\"}')", params![TIME,lost]).unwrap();
    assert!(s.graph_metadata(lost).unwrap().unwrap()["creation"]["actor"].is_null());
}

#[test]
fn malformed_legacy_provenance_rolls_back_the_entire_migration() {
    for payload in [
        "broken-json",
        "[]",
        "{\"actor\":7}",
        "{\"graph\":\"urn:wrong\",\"actor\":\"x\"}",
    ] {
        let s = Store::open_in_memory().unwrap();
        s.graph_create_recorded(GRAPH, TIME, Some("original"))
            .unwrap();
        s.conn.execute_batch("DROP TABLE graph_creators").unwrap();
        s.conn
            .execute(
                "UPDATE events SET payload=?1 WHERE type='graph.created'",
                [payload],
            )
            .unwrap();
        assert!(
            Store::migrate_graph_creators_v1(&s.conn).is_err(),
            "{payload}"
        );
        let exists: bool = s
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='graph_creators')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!exists);
        assert!(s.conn.is_autocommit());
    }
}

#[test]
fn failed_durable_insert_rolls_back_registration_event_and_interning() {
    let s = Store::open_in_memory().unwrap();
    s.conn.execute_batch("CREATE TEMP TRIGGER refuse_durable BEFORE INSERT ON graph_creators BEGIN SELECT RAISE(ABORT,'fixture refuses durable record'); END").unwrap();
    assert!(
        s.graph_create_recorded(GRAPH, TIME, Some("original"))
            .is_err()
    );
    assert_eq!(s.lookup(GRAPH).unwrap(), None);
    let events: i64 = s
        .conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE subject=?1",
            [GRAPH],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(events, 0);
    s.conn.execute_batch("DROP TRIGGER refuse_durable").unwrap();
    assert_eq!(
        s.graph_create_recorded(GRAPH, TIME, Some("original"))
            .unwrap()["created"],
        true
    );
}

#[test]
fn respaced_attached_source_uses_its_durable_actor_after_events_are_pruned() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("original.db");
    let layer = directory.path().join("layer.db");
    {
        let s = Store::open(original.to_str().unwrap()).unwrap();
        s.graph_create_recorded(GRAPH, TIME, Some("source-original"))
            .unwrap();
        s.prune_events(CUTOFF).unwrap();
    }
    super::super::respace::respace_file(&original, &layer, 7).unwrap();
    let before = std::fs::read(&layer).unwrap();
    let path = directory.path().join("main.db");
    let s = Store::open_with_attachments(
        path.to_str().unwrap(),
        &[super::super::attach::Attachment::read_only(
            "source",
            layer.to_str().unwrap(),
        )],
    )
    .unwrap();
    s.conn.execute("INSERT INTO events(type,ts,subject,tx_id,payload) VALUES('graph.created',?1,?2,0,'{\"actor\":\"local-impostor\"}')",params![TIME,GRAPH]).unwrap();
    let metadata = s.graph_metadata(GRAPH).unwrap().unwrap();
    assert_eq!(metadata["creation"]["actor"], "source-original");
    assert_eq!(metadata["creation"]["timestamp"], TIME);
    assert_eq!(metadata["source"], "source");
    assert_eq!(
        s.graph_creation_provenance(metadata["g"].as_i64().unwrap())
            .unwrap(),
        metadata["creation"]
    );
    let listing = crate::tool_graph_list(&s, &json!({})).unwrap();
    let listed = listing["graphs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["iri"] == GRAPH)
        .unwrap();
    assert_eq!(listed["creation"], metadata["creation"]);
    assert_eq!(std::fs::read(&layer).unwrap(), before);
}

#[test]
fn old_read_only_sources_fall_back_to_retained_events_without_migration() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("legacy.db");
    {
        let s = Store::open(path.to_str().unwrap()).unwrap();
        s.graph_create_recorded(GRAPH, TIME, Some("legacy-original"))
            .unwrap();
        s.conn.execute_batch("DROP TABLE graph_creators").unwrap();
    }
    let before = std::fs::read(&path).unwrap();
    let reader = Store::open_graph_metadata_reader(path.to_str().unwrap(), &[], GRAPH).unwrap();
    assert_eq!(
        reader.graph_metadata(GRAPH).unwrap().unwrap()["creation"]["actor"],
        "legacy-original"
    );
    assert_eq!(reader.conn.total_changes(), 0);
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn incompatible_registry_shape_refuses_instead_of_guessing_a_migration() {
    let s = Store::open_in_memory().unwrap();
    s.conn.execute_batch("DROP TABLE graph_creators; CREATE TABLE graph_creators(g INTEGER PRIMARY KEY, actor TEXT)").unwrap();
    assert!(Store::migrate_graph_creators_v1(&s.conn).is_err());
    assert!(s.conn.is_autocommit());
}

#[test]
fn recreated_legacy_graph_does_not_inherit_an_older_incarnations_actor() {
    let s = Store::open_in_memory().unwrap();
    let old = s
        .graph_create_recorded(GRAPH, TIME, Some("old-incarnation"))
        .unwrap();
    s.conn
        .execute(
            "DELETE FROM graphs WHERE g=?1",
            [old["g"].as_i64().unwrap()],
        )
        .unwrap();
    let g = s.graph_create(GRAPH).unwrap();
    assert!(s.graph_creation_provenance(g).unwrap()["actor"].is_null());
    s.conn.execute_batch("DROP TABLE graph_creators").unwrap();
    Store::migrate_graph_creators_v1(&s.conn).unwrap();
    assert!(s.graph_metadata(GRAPH).unwrap().unwrap()["creation"]["actor"].is_null());
}

#[test]
fn frozen_broad_listing_never_borrows_the_mutable_original_actor() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("main.db");
    let mut s = Store::open(path.to_str().unwrap()).unwrap();
    let g = s
        .graph_create_recorded(GRAPH, TIME, Some("mutable-original"))
        .unwrap()["g"]
        .as_i64()
        .unwrap();
    s.freeze_graph(GRAPH, directory.path().to_str().unwrap(), TIME, None)
        .unwrap();
    let metadata = s.graph_metadata(GRAPH).unwrap().unwrap();
    assert!(
        metadata["creation"]["actor"].is_null(),
        "old pack has no creator evidence"
    );
    assert_eq!(
        s.graph_creation_provenance(g).unwrap(),
        metadata["creation"]
    );
}
