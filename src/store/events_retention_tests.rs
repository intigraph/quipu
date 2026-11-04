use crate::Store;
use crate::episode::{Episode, ingest_episode};
use crate::namespace::DEFAULT_BASE_NS;

const CUTOFF: &str = "2026-07-01T00:00:00Z";

fn ingest(store: &mut Store, name: &str, ts: &str) {
    let episode: Episode = serde_json::from_value(serde_json::json!({
        "name": name,
        "nodes": [{"name": format!("{name}-entity"), "type": "T"}],
    }))
    .unwrap();
    ingest_episode(store, &episode, ts, DEFAULT_BASE_NS).unwrap();
}

fn ts_count(store: &Store, ts: &str) -> i64 {
    store
        .conn
        .query_row("SELECT COUNT(*) FROM events WHERE ts = ?1", [ts], |r| {
            r.get(0)
        })
        .unwrap()
}

#[test]
fn an_offset_timestamp_inside_the_window_is_kept() {
    // 2026-06-30T23:00:00-05:00 is 2026-07-01T04:00:00Z, after the cutoff.
    // As text it sorts before "2026-07-01T00:00:00Z", so a text comparison
    // deletes it.
    let mut store = Store::open_in_memory().unwrap();
    let inside = "2026-06-30T23:00:00-05:00";
    let outside = "2026-06-30T17:00:00-05:00"; // 22:00Z, before the cutoff
    ingest(&mut store, "inside", inside);
    ingest(&mut store, "outside", outside);
    assert!(ts_count(&store, inside) > 0 && ts_count(&store, outside) > 0);
    store.prune_events(CUTOFF).unwrap();
    assert!(
        ts_count(&store, inside) > 0,
        "in-window offset row was deleted"
    );
    assert_eq!(ts_count(&store, outside), 0);
}

#[test]
fn an_unparseable_timestamp_is_kept() {
    let mut store = Store::open_in_memory().unwrap();
    ingest(&mut store, "reactive", "reactive");
    ingest(&mut store, "old", "2026-01-01T00:00:00Z");
    store.prune_events(CUTOFF).unwrap();
    assert!(ts_count(&store, "reactive") > 0);
    assert_eq!(ts_count(&store, "2026-01-01T00:00:00Z"), 0);
}

#[test]
fn a_batch_deletes_at_most_its_limit_lowest_offsets_first() {
    let mut store = Store::open_in_memory().unwrap();
    for day in 1..=5 {
        ingest(
            &mut store,
            &format!("old-{day}"),
            &format!("2026-01-0{day}T00:00:00Z"),
        );
    }
    ingest(&mut store, "new", "2026-08-01T00:00:00Z");
    let total: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
        .unwrap();
    let first_old = ts_count(&store, "2026-01-01T00:00:00Z");
    assert!(first_old > 0);
    let n = store.prune_events_batch(CUTOFF, 1).unwrap();
    assert_eq!(n, 1);
    assert_eq!(ts_count(&store, "2026-01-01T00:00:00Z"), first_old - 1);
    let rest = store.prune_events(CUTOFF).unwrap();
    let kept = ts_count(&store, "2026-08-01T00:00:00Z");
    assert_eq!(1 + rest as i64 + kept, total);
    assert_eq!(store.prune_events_batch(CUTOFF, 100).unwrap(), 0);
}

#[test]
fn a_zero_limit_deletes_nothing() {
    let mut store = Store::open_in_memory().unwrap();
    ingest(&mut store, "old", "2026-01-01T00:00:00Z");
    assert_eq!(store.prune_events_batch(CUTOFF, 0).unwrap(), 0);
    assert!(ts_count(&store, "2026-01-01T00:00:00Z") > 0);
}

#[test]
fn the_oldest_retained_event_moves_with_the_prune() {
    let mut store = Store::open_in_memory().unwrap();
    assert_eq!(store.oldest_retained_event_ts().unwrap(), None);
    ingest(&mut store, "old", "2026-01-01T00:00:00Z");
    ingest(&mut store, "new", "2026-08-01T00:00:00Z");
    assert_eq!(
        store.oldest_retained_event_ts().unwrap().as_deref(),
        Some("2026-01-01T00:00:00Z")
    );
    store.prune_events(CUTOFF).unwrap();
    assert_eq!(
        store.oldest_retained_event_ts().unwrap().as_deref(),
        Some("2026-08-01T00:00:00Z")
    );
}
