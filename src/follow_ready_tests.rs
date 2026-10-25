//! Readiness from a follower's own records; the bootstrap path that writes
//! the catch-up marker is covered with the follower in `follow_tests.rs`.
use super::*;

const SOURCE: &str = "urn:uuid:source";
const EPOCH: &str = "urn:uuid:epoch";

/// A store whose records say it followed SOURCE to tx 7 and caught up.
fn caught_up() -> Store {
    let store = Store::open_in_memory().unwrap();
    store
        .conn
        .execute_batch(&format!(
            "CREATE TABLE replica_progress (id INTEGER PRIMARY KEY CHECK (id = 1), \
             store_id TEXT NOT NULL, log_epoch TEXT NOT NULL, source_tx INTEGER NOT NULL); \
             INSERT INTO replica_progress VALUES (1, '{SOURCE}', '{EPOCH}', 7);"
        ))
        .unwrap();
    mark_caught_up(&store).unwrap();
    store
}

fn reason(readiness: Readiness) -> String {
    match readiness {
        Readiness::NotReady { reason } => reason,
        ready @ Readiness::Ready { .. } => panic!("expected not ready, got {ready:?}"),
    }
}

#[test]
fn a_caught_up_follower_of_the_expected_store_is_ready() {
    let store = caught_up();
    let Readiness::Ready {
        lineage,
        source_tx,
        caught_up_at,
    } = readiness(&store, Some(SOURCE)).unwrap()
    else {
        panic!("positive control must be ready");
    };
    assert_eq!((lineage.as_str(), source_tx), (SOURCE, 7));
    assert!(caught_up_at.ends_with('Z'), "{caught_up_at}");
}

#[test]
fn readiness_fails_closed_without_an_expected_store() {
    assert!(reason(readiness(&caught_up(), None).unwrap()).contains("expected_store_id"));
}

#[test]
fn a_store_the_applier_did_not_build_is_not_ready() {
    let plain = Store::open_in_memory().unwrap();
    assert!(reason(readiness(&plain, Some(SOURCE)).unwrap()).contains("not a follower"));
}

#[test]
fn another_lineage_is_not_ready() {
    let text = reason(readiness(&caught_up(), Some("urn:uuid:other")).unwrap());
    assert!(
        text.contains(SOURCE) && text.contains("urn:uuid:other"),
        "{text}"
    );
}

#[test]
fn a_stopped_follower_is_not_ready() {
    let store = caught_up();
    store
        .conn
        .execute_batch(
            "CREATE TABLE replica_stop (id INTEGER PRIMARY KEY CHECK (id = 1), reason TEXT NOT NULL); \
             INSERT INTO replica_stop VALUES (1, 'revoked');",
        )
        .unwrap();
    assert!(reason(readiness(&store, Some(SOURCE)).unwrap()).contains("revoked"));
}

#[test]
fn an_ended_lineage_is_not_ready_but_another_epoch_ending_is_irrelevant() {
    let store = caught_up();
    store
        .conn
        .execute_batch(
            "CREATE TABLE replica_source_end (store_id TEXT NOT NULL, log_epoch TEXT NOT NULL, \
             reason TEXT NOT NULL, PRIMARY KEY (store_id, log_epoch)); \
             INSERT INTO replica_source_end VALUES ('urn:uuid:source', 'urn:uuid:older', 'x');",
        )
        .unwrap();
    assert!(matches!(
        readiness(&store, Some(SOURCE)).unwrap(),
        Readiness::Ready { .. }
    ));
    store
        .conn
        .execute(
            "INSERT INTO replica_source_end VALUES (?1, ?2, 'the source log epoch changed')",
            [SOURCE, EPOCH],
        )
        .unwrap();
    assert!(reason(readiness(&store, Some(SOURCE)).unwrap()).contains("epoch changed"));
}

#[test]
fn a_catch_up_on_an_older_epoch_does_not_make_the_current_one_ready() {
    let store = caught_up();
    store
        .conn
        .execute(
            "UPDATE replica_progress SET log_epoch = 'urn:uuid:new', source_tx = 9",
            [],
        )
        .unwrap();
    assert!(reason(readiness(&store, Some(SOURCE)).unwrap()).contains("bootstrap"));
    // The first marker stands; a later catch-up does not rewrite it.
    mark_caught_up(&store).unwrap();
    assert!(reason(readiness(&store, Some(SOURCE)).unwrap()).contains("bootstrap"));
}
