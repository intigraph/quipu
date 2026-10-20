use super::*;
use crate::tool_graph_list;

const GRAPH: &str = "urn:fixture:metadata";
const TIME: &str = "2026-10-20T00:00:00Z";

#[test]
fn exact_positive_missing_interned_and_removed_are_read_only() {
    let s = Store::open_in_memory().unwrap();
    s.graph_create_recorded(GRAPH, TIME, Some("fixture-first"))
        .unwrap();
    s.graph_create("urn:fixture:other").unwrap();
    s.intern("urn:fixture:interned").unwrap();
    let removed = s.graph_create("urn:fixture:removed").unwrap();
    s.conn
        .execute("DELETE FROM graphs WHERE g=?1", [removed])
        .unwrap();
    // Make list/enrichment unusable: point mode must not touch transactions.
    s.conn
        .execute_batch("DROP TABLE transactions; PRAGMA query_only=ON;")
        .unwrap();
    let changes = s.conn.total_changes();
    let result = tool_graph_list(&s, &json!({"graph":GRAPH})).unwrap();
    assert_eq!(result["count"], 1);
    assert_eq!(result["graphs"][0]["iri"], GRAPH);
    assert_eq!(
        result["graphs"][0]["creation"],
        json!({"timestamp":TIME,"actor":"fixture-first"})
    );
    assert!(result["graphs"][0].get("materialization").is_none());
    for iri in [
        "urn:fixture:absent",
        "urn:fixture:interned",
        "urn:fixture:removed",
    ] {
        assert_eq!(
            tool_graph_list(&s, &json!({"graph":iri})).unwrap()["count"],
            0
        );
    }
    assert_eq!(s.lookup("urn:fixture:absent").unwrap(), None);
    assert_eq!(s.conn.total_changes(), changes);
    assert!(
        tool_graph_list(&s, &json!({})).is_err(),
        "list positive control reaches enrichment"
    );
}

#[test]
fn legacy_null_malformed_selectors_and_real_errors() {
    let s = Store::open_in_memory().unwrap();
    s.graph_create(GRAPH).unwrap();
    assert!(s.graph_metadata(GRAPH).unwrap().unwrap()["creation"]["actor"].is_null());
    for input in [
        json!({"graph":null}),
        json!({"graph":3}),
        json!({"graph":""}),
        json!({"graph":"relative"}),
        json!({"graph":GRAPH,"kind":"archive"}),
        json!({"graph":GRAPH,"lifecycle":"frozen"}),
    ] {
        assert!(tool_graph_list(&s, &input).is_err(), "{input}");
    }
    s.conn.execute("INSERT INTO events(type,ts,subject,tx_id,payload) VALUES('graph.created',?1,?2,0,'broken-json')",[TIME,GRAPH]).unwrap();
    assert!(s.graph_metadata(GRAPH).is_err());
    s.conn.execute_batch("DROP TABLE events").unwrap();
    assert!(s.graph_metadata(GRAPH).is_err());
}

#[test]
fn migration_v1_idempotent_rollback_preserves_rows_and_read_only_never_migrates() {
    let s = Store::open_in_memory().unwrap();
    s.graph_create_recorded(GRAPH, TIME, None).unwrap();
    let before = s.graph_metadata(GRAPH).unwrap();
    s.conn
        .execute_batch("DROP INDEX idx_events_type_subject_offset")
        .unwrap();
    assert!(s.graph_metadata(GRAPH).is_err());
    let changes = s.conn.total_changes();
    for _ in 0..2 {
        Store::migrate_graph_metadata_v1(&s.conn).unwrap();
    }
    assert_eq!(s.conn.total_changes(), changes);
    assert_eq!(s.graph_metadata(GRAPH).unwrap(), before);
    s.conn
        .execute_batch("DROP INDEX idx_events_type_subject_offset; PRAGMA query_only=ON")
        .unwrap();
    assert!(Store::migrate_graph_metadata_v1(&s.conn).is_err());
    assert!(s.graph_metadata(GRAPH).is_err());
    assert_eq!(s.conn.total_changes(), changes);
}

#[test]
fn index_shape_collision_is_refused() {
    let s = Store::open_in_memory().unwrap();
    s.conn.execute_batch("DROP INDEX idx_events_type_subject_offset; CREATE INDEX idx_events_type_subject_offset ON events(type,offset)").unwrap();
    assert!(Store::migrate_graph_metadata_v1(&s.conn).is_err());
}

#[test]
fn attached_identity_uses_source_creation_never_local_event_and_refuses_missing_index() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("original.db");
    let layer = dir.path().join("layer.db");
    {
        let s = Store::open(original.to_str().unwrap()).unwrap();
        s.graph_create_recorded(GRAPH, TIME, Some("source-creator"))
            .unwrap();
    }
    crate::store::respace::respace_file(&original, &layer, 7).unwrap();
    let main = dir.path().join("main.db");
    let s = Store::open_with_attachments(
        main.to_str().unwrap(),
        &[crate::store::attach::Attachment::read_only(
            "fixture",
            layer.to_str().unwrap(),
        )],
    )
    .unwrap();
    s.conn.execute("INSERT INTO events(type,ts,subject,tx_id,payload) VALUES('graph.created',?1,?2,0,'{\"actor\":\"mutable-impostor\"}')",[TIME,GRAPH]).unwrap();
    let before = std::fs::read(&layer).unwrap();
    let metadata = s.graph_metadata(GRAPH).unwrap().unwrap();
    assert_eq!(metadata["source"], "fixture");
    assert_eq!(metadata["creation"]["actor"], "source-creator");
    assert_eq!(metadata["creation"]["timestamp"], TIME);
    assert_eq!(std::fs::read(&layer).unwrap(), before);
    let reader = Store::open_graph_metadata_reader(
        main.to_str().unwrap(),
        &[crate::store::attach::Attachment::read_only(
            "fixture",
            layer.to_str().unwrap(),
        )],
    )
    .unwrap();
    assert_eq!(
        reader.graph_metadata(GRAPH).unwrap(),
        Some(metadata.clone())
    );
    assert_eq!(reader.conn.total_changes(), 0);
    assert!(reader.conn.execute("DELETE FROM main.events", []).is_err());
    drop(reader);
    assert!(s.conn.execute("DELETE FROM fixture.events", []).is_err());
    drop(s);
    let conn = Connection::open(&layer).unwrap();
    conn.execute_batch("DROP INDEX idx_events_type_subject_offset")
        .unwrap();
    drop(conn);
    let s = Store::open_with_attachments(
        main.to_str().unwrap(),
        &[crate::store::attach::Attachment::read_only(
            "fixture",
            layer.to_str().unwrap(),
        )],
    )
    .unwrap();
    assert!(
        s.graph_metadata(GRAPH).is_err(),
        "never scan an old immutable source or fall back to local actor"
    );
}

#[test]
fn frozen_metadata_anchors_pack_identity_without_original_store_actor_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.db");
    let mut s = Store::open(main.to_str().unwrap()).unwrap();
    s.graph_create_recorded(GRAPH, TIME, Some("original-mutable-store"))
        .unwrap();
    s.freeze_graph(GRAPH, dir.path().to_str().unwrap(), TIME, None)
        .unwrap();
    let metadata = s.graph_metadata(GRAPH).unwrap().unwrap();
    assert!(metadata["source"].is_string());
    assert_eq!(metadata["lifecycle"], "frozen");
    // The existing pack format does not export graph.created events. Its
    // missing actor is NULL, not a licence to reuse a main-store declaration.
    assert!(metadata["creation"]["actor"].is_null());
    let alias = metadata["source"].as_str().unwrap();
    let source_time: String = s
        .conn
        .query_row(
            &format!("SELECT created_at FROM {alias}.graphs WHERE g=?1"),
            [metadata["g"].as_i64().unwrap()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(metadata["creation"]["timestamp"], source_time);
    drop(s);
    let s = Store::open(main.to_str().unwrap()).unwrap();
    assert_eq!(
        s.graph_metadata(GRAPH).unwrap().unwrap()["creation"],
        metadata["creation"]
    );
}

#[test]
fn representative_private_index_build_and_drop_preserve_all_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private.db");
    let mut s = Store::open(path.to_str().unwrap()).unwrap();
    let e = s.intern("urn:fixture:entity").unwrap();
    let a = s.intern("urn:fixture:attribute").unwrap();
    s.transact(
        &[crate::store::Datum {
            entity: e,
            attribute: a,
            value: crate::types::Value::Str("retained fact".into()),
            valid_from: TIME.into(),
            valid_to: None,
            op: crate::types::Op::Assert,
        }],
        TIME,
        None,
        None,
    )
    .unwrap();
    s.graph_create_recorded(GRAPH, TIME, Some("fixture"))
        .unwrap();
    s.conn
        .execute_batch("DROP INDEX idx_events_type_subject_offset")
        .unwrap();
    let tx = s.conn.unchecked_transaction().unwrap();
    for i in 0..100_000 {
        tx.execute(
            "INSERT INTO events(type,ts,subject,tx_id,payload) VALUES(?1,?2,?3,0,?4)",
            rusqlite::params![
                if i % 3 == 0 {
                    "graph.created"
                } else {
                    "fixture.other"
                },
                TIME,
                format!("urn:fixture:{i}"),
                "{\"actor\":\"fixture\"}"
            ],
        )
        .unwrap();
    }
    for i in 0..10_000 {
        tx.execute("INSERT INTO frozen_packs(graph_iri,alias,path,space,content_hash,frozen_at,thawed_at) VALUES(?1,?2,'fixture-only',?3,'fixture-hash',?4,?4)",
            rusqlite::params![format!("urn:fixture:old:{i}"),format!("fixture_{i}"),i+1,TIME]).unwrap();
    }
    tx.commit().unwrap();
    // Full ordered event content, not only a row count.
    let events = || {
        let mut q = s
            .conn
            .prepare(
                "SELECT offset,type,ts,subject,payload,group_id,tx_id FROM events ORDER BY offset",
            )
            .unwrap();
        q.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, i64>(6)?,
            ))
        })
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap()
    };
    let facts_snapshot = || -> String {
        s.conn.query_row("SELECT json_group_array(json_object('e',e,'a',a,'v',hex(v),'g',g,'tx',tx,'from',valid_from,'to',valid_to,'op',op)) FROM facts",[],|r|r.get(0)).unwrap()
    };
    let facts_before = facts_snapshot();
    let frozen_snapshot = || -> String {
        s.conn.query_row("SELECT json_group_array(json_object('id',id,'graph',graph_iri,'alias',alias,'path',path,'space',space,'hash',content_hash,'at',frozen_at,'thaw',thawed_at)) FROM frozen_packs",[],|r|r.get(0)).unwrap()
    };
    let frozen_before = frozen_snapshot();
    s.conn
        .execute_batch("DROP INDEX idx_frozen_packs_graph_thawed")
        .unwrap();
    let before = events();
    let start = std::time::Instant::now();
    Store::migrate_graph_metadata_v1(&s.conn).unwrap();
    let elapsed = start.elapsed();
    let pages: i64 = s
        .conn
        .query_row("SELECT count(*) FROM dbstat WHERE name=?1", [INDEX], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(events(), before);
    s.conn
        .execute_batch(
            "DROP INDEX idx_events_type_subject_offset; DROP INDEX idx_frozen_packs_graph_thawed",
        )
        .unwrap();
    assert_eq!(events(), before);
    let facts: i64 = s
        .conn
        .query_row("SELECT count(*) FROM facts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(facts, 1);
    assert_eq!(facts_snapshot(), facts_before);
    assert_eq!(frozen_snapshot(), frozen_before);
    eprintln!(
        "graph_metadata private migration 100001 mixed events +10000 historical freeze rows elapsed={elapsed:?} creator_index_pages={pages}; full facts/events/frozen rows unchanged after build/drop; production build cost UNKNOWN"
    );
}

#[test]
fn native_creator_query_cost_is_bounded_with_positive_and_absence_controls() {
    for count in [100, 10_000] {
        let s = Store::open_in_memory().unwrap();
        let tx = s.conn.unchecked_transaction().unwrap();
        for i in 0..count {
            tx.execute(
                "INSERT INTO events(type,ts,subject,tx_id,payload) VALUES('graph.created',?1,?2,0,?3)",
                rusqlite::params![
                    TIME,
                    if i == 0 {
                        GRAPH.to_string()
                    } else {
                        format!("urn:fixture:{i}")
                    },
                    "{\"actor\":\"first\"}"
                ],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        let sql = "SELECT payload FROM events WHERE type='graph.created' AND subject=?1 ORDER BY offset DESC LIMIT 1";
        s.conn
            .execute_batch("DROP INDEX idx_events_type_subject_offset")
            .unwrap();
        for phase in ["before", "after"] {
            if phase == "after" {
                Store::migrate_graph_metadata_v1(&s.conn).unwrap();
            }
            for graph in [GRAPH, "urn:fixture:absent"] {
                let mut stmt = s.conn.prepare(sql).unwrap();
                let payload: Option<String> =
                    stmt.query_row([graph], |r| r.get(0)).optional().unwrap();
                assert_eq!(payload.is_some(), graph == GRAPH);
                let steps = stmt.get_status(rusqlite::StatementStatus::VmStep);
                let plan: String = s
                    .conn
                    .query_row(&format!("EXPLAIN QUERY PLAN {sql}"), [graph], |r| r.get(3))
                    .unwrap();
                eprintln!(
                    "graph_metadata N={count} phase={phase} positive={} VMsteps={steps} plan={plan}",
                    graph == GRAPH
                );
                if phase == "after" {
                    assert!(steps < 100);
                    assert!(plan.contains(INDEX));
                } else {
                    assert!(steps > count);
                }
            }
        }
        assert_eq!(creator(&s.conn, "main", GRAPH).unwrap(), json!("first"));
    }
}
