//! The rollback acceptance pair (event-push design §7): a store written by
//! this code, read by the code that preceded named-graph events.
//!
//! The pre-change reader, resume and pruner are pinned below as the literal
//! SQL they ran, so this test keeps describing what an older binary does
//! even after the current functions change.

use super::*;
use crate::types::Op;
use rusqlite::OptionalExtension;

const BOARD: &str = "https://example.org/graph/board";
const TS: &str = "2026-10-24T00:00:00Z";
const LATER: &str = "2026-12-01T00:00:00Z";

/// `Store::events_after` before named graphs could emit, with no filter.
const PRE_CHANGE_READ: &str = "SELECT \"offset\", type, ts, subject, group_id, tx_id, payload \
     FROM events WHERE \"offset\" > ?1 ORDER BY \"offset\" LIMIT ?2";
/// `Store::consumer_committed` before named graphs could emit.
const PRE_CHANGE_RESUME: &str = "SELECT committed_offset FROM consumers WHERE consumer_id = ?1";
/// `Store::prune_events` before named graphs could emit.
const PRE_CHANGE_PRUNE_SAFE: &str = "SELECT COALESCE(MIN(committed_offset), ?1) FROM consumers";
const PRE_CHANGE_PRUNE: &str = "DELETE FROM events WHERE ts < ?1 AND \"offset\" <= ?2";

fn datum(store: &Store, s: &str) -> Datum {
    Datum {
        entity: store.intern(s).unwrap(),
        attribute: store.intern("urn:p:x").unwrap(),
        value: Value::Ref(store.intern("urn:o:1").unwrap()),
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    }
}

/// ROOT and board writes, with a ROOT consumer and a board consumer each
/// committed partway.
fn written() -> (Store, i64, i64) {
    let mut store = Store::open_in_memory().unwrap();
    store
        .transact(&[datum(&store, "urn:item:r1")], TS, None, None)
        .unwrap();
    store.graph_create_recorded(BOARD, TS, None).unwrap();
    let g = store.committed_graph_id(BOARD).unwrap().unwrap();
    store
        .transact_to_graph(&[datum(&store, "urn:item:b1")], TS, None, None, g)
        .unwrap();
    let root_cursor = store.latest_event_offset().unwrap();
    let board_cursor = store.latest_event_offset_in(Some(BOARD)).unwrap();
    store
        .commit_consumer("root-reader", root_cursor, TS)
        .unwrap();
    store
        .commit_consumer_in("board-reader", Some(BOARD), board_cursor, TS)
        .unwrap();
    // Writes after both cursors: what each reader resumes into.
    store
        .transact(&[datum(&store, "urn:item:r2")], TS, None, None)
        .unwrap();
    store
        .transact_to_graph(&[datum(&store, "urn:item:b2")], TS, None, None, g)
        .unwrap();
    (store, root_cursor, board_cursor)
}

fn pre_change_read(conn: &rusqlite::Connection, table: &str, since: i64) -> Vec<(i64, String)> {
    let sql = PRE_CHANGE_READ.replace("FROM events", &format!("FROM {table}"));
    let mut st = conn.prepare(&sql).unwrap();
    st.query_map(params![since, 10_000], |r| {
        Ok((
            r.get(0)?,
            r.get::<_, Option<String>>(3)?.unwrap_or_default(),
        ))
    })
    .unwrap()
    .collect::<rusqlite::Result<_>>()
    .unwrap()
}

#[test]
fn a_pre_change_reader_sees_exactly_the_root_log() {
    let (store, root_cursor, _) = written();
    let current: Vec<(i64, String)> = store
        .events_after(0, 10_000, None, None)
        .unwrap()
        .into_iter()
        .map(|e| (e.offset, e.subject.unwrap_or_default()))
        .collect();
    let old = pre_change_read(&store.conn, "events", 0);
    assert_eq!(
        old, current,
        "the old reader and the new ROOT reader agree, offsets included"
    );
    assert!(
        old.iter().all(|(_, s)| !s.contains("urn:item:b")),
        "no board event: {old:?}"
    );
    assert!(old.iter().any(|(_, s)| s.contains("urn:item:r2")));
    // The old resume finds the ROOT reader where it committed, and the
    // board reader's cursor is not in its table at all.
    let resume = |id: &str| -> Option<i64> {
        store
            .conn
            .query_row(PRE_CHANGE_RESUME, params![id], |r| r.get(0))
            .optional()
            .unwrap()
    };
    assert_eq!(resume("root-reader"), Some(root_cursor));
    assert_eq!(resume("board-reader"), None);
    let after = pre_change_read(&store.conn, "events", root_cursor);
    assert!(after.iter().all(|(o, _)| *o > root_cursor) && !after.is_empty());
}

#[test]
fn a_pre_change_prune_never_touches_the_board_log_and_the_board_resumes() {
    let (store, _, board_cursor) = written();
    let board_before = store
        .events_after_in(0, 10_000, None, None, Some(BOARD))
        .unwrap();
    let safe: i64 = store
        .conn
        .query_row(PRE_CHANGE_PRUNE_SAFE, params![i64::MAX], |r| r.get(0))
        .unwrap();
    store
        .conn
        .execute(PRE_CHANGE_PRUNE, params![LATER, safe])
        .unwrap();
    let board_after = store
        .events_after_in(0, 10_000, None, None, Some(BOARD))
        .unwrap();
    assert_eq!(
        board_before.iter().map(|e| e.offset).collect::<Vec<_>>(),
        board_after.iter().map(|e| e.offset).collect::<Vec<_>>(),
        "an older pruner leaves every board row"
    );
    // Re-upgraded, the board reader resumes from its own cursor into only
    // its graph's events.
    let resume = store
        .consumer_committed_in("board-reader", Some(BOARD))
        .unwrap();
    assert_eq!(resume, board_cursor);
    let next = store
        .events_after_in(resume, 10_000, None, None, Some(BOARD))
        .unwrap();
    assert!(!next.is_empty());
    assert!(next.iter().all(|e| {
        e.graph.as_deref() == Some(BOARD)
            && e.subject
                .as_deref()
                .is_some_and(|s| s.contains("urn:item:b2"))
    }));
}

/// The control: the rejected design, a `graph` column on `events`, leaks a
/// board event to the same pre-change reader. Without this, the test above
/// could pass on a reader that never saw any named-graph row at all.
#[test]
fn control_a_graph_column_on_events_would_leak_to_the_old_reader() {
    let store = Store::open_in_memory().unwrap();
    store
        .conn
        .execute_batch(
            "CREATE TABLE events_col (\"offset\" INTEGER PRIMARY KEY AUTOINCREMENT, type TEXT NOT NULL, \
             ts TEXT NOT NULL, subject TEXT, group_id TEXT, tx_id INTEGER NOT NULL, \
             payload TEXT NOT NULL DEFAULT '{}', graph TEXT);
             INSERT INTO events_col (type, ts, subject, tx_id, graph) VALUES ('entity.added', 't', 'urn:item:r1', 1, NULL);
             INSERT INTO events_col (type, ts, subject, tx_id, graph) VALUES ('entity.added', 't', 'urn:item:b1', 2, 'https://example.org/graph/board');",
        )
        .unwrap();
    let old = pre_change_read(&store.conn, "events_col", 0);
    assert_eq!(
        old.len(),
        2,
        "the old reader has no graph predicate: {old:?}"
    );
}
