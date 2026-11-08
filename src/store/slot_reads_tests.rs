use super::*;

const T0: &str = "2026-01-01T00:00:00Z";
const TD: &str = "2026-01-02T00:00:00Z";
const TZ: &str = "2026-01-03T00:00:00Z";

fn limits() -> SlotReadLimits {
    SlotReadLimits {
        max_claims: 16,
        max_bytes: 4096,
    }
}

fn insert(store: &Store, graph: i64, value: &str, tx: i64, end: Option<&str>, closed: Option<i64>) {
    for id in std::iter::once(tx).chain(closed) {
        store
            .conn
            .execute(
                "INSERT OR IGNORE INTO transactions (id,timestamp) VALUES (?1,?2)",
                params![id, T0],
            )
            .unwrap();
    }
    store
        .conn
        .execute(
            "INSERT INTO facts (g,e,a,v,tx,valid_from,valid_to,op,retracted_tx) \
         VALUES (?1,10,20,?2,?3,?4,?5,1,?6)",
            params![
                graph,
                Value::Str(value.into()).to_bytes(),
                tx,
                T0,
                end,
                closed
            ],
        )
        .unwrap();
}

fn read(store: &Store, tx: i64, time: &str, limits: SlotReadLimits) -> Result<Vec<Fact>> {
    store.bounded_slot_facts_at(
        0,
        10,
        20,
        &AsOf {
            tx: Some(tx),
            valid_at: Some(time.into()),
        },
        limits,
    )
}

#[test]
fn later_close_preserves_base_but_current_obeys_authored_boundary() {
    let store = Store::open_in_memory().unwrap();
    insert(&store, 0, "later", 1, Some(TZ), Some(3));
    let base = read(&store, 1, TZ, limits()).unwrap();
    assert_eq!(base.len(), 1);
    assert_eq!(base[0].valid_to, None);
    assert_eq!(read(&store, 3, TD, limits()).unwrap().len(), 1);
    assert!(read(&store, 3, TZ, limits()).unwrap().is_empty());
}

#[test]
fn original_finite_and_empty_received_intervals_are_not_reopened() {
    let store = Store::open_in_memory().unwrap();
    insert(&store, 0, "finite", 1, Some(TZ), None);
    insert(&store, 0, "empty", 2, Some(T0), Some(2));
    assert_eq!(read(&store, 2, TD, limits()).unwrap().len(), 1);
    assert!(read(&store, 2, TZ, limits()).unwrap().is_empty());
    assert!(read(&store, 0, TD, limits()).unwrap().is_empty());
}

#[test]
fn graph_and_slot_are_bound_before_history_budget() {
    let store = Store::open_in_memory().unwrap();
    insert(&store, 0, "wanted", 1, None, None);
    for i in 0..20 {
        insert(&store, 7, &format!("other-{i}"), 1, None, None);
    }
    assert_eq!(
        read(
            &store,
            1,
            TD,
            SlotReadLimits {
                max_claims: 1,
                ..limits()
            }
        )
        .unwrap()
        .len(),
        1
    );
    assert!(
        store
            .bounded_slot_facts_at(
                0,
                11,
                20,
                &AsOf {
                    tx: Some(1),
                    valid_at: Some(TD.into())
                },
                limits()
            )
            .unwrap()
            .is_empty()
    );
}

#[test]
fn overflow_refuses_instead_of_returning_a_partial_or_deduplicated_prefix() {
    let store = Store::open_in_memory().unwrap();
    insert(&store, 0, "first", 1, None, None);
    insert(&store, 0, "future", 2, None, None);
    assert!(
        read(
            &store,
            1,
            TD,
            SlotReadLimits {
                max_claims: 1,
                ..limits()
            }
        )
        .is_err()
    );
    assert!(
        read(
            &store,
            1,
            TD,
            SlotReadLimits {
                max_bytes: 1,
                ..limits()
            }
        )
        .is_err()
    );
    assert!(
        read(
            &store,
            1,
            TD,
            SlotReadLimits {
                max_claims: 0,
                ..limits()
            }
        )
        .is_err()
    );
}

#[test]
fn explicit_cuts_and_supported_timestamp_precision_are_required() {
    let store = Store::open_in_memory().unwrap();
    assert!(
        store
            .bounded_slot_facts_at(
                0,
                10,
                20,
                &AsOf {
                    tx: None,
                    valid_at: Some(TD.into())
                },
                limits()
            )
            .is_err()
    );
    assert!(read(&store, 1, "2026-01-02T00:00:00.100Z", limits()).is_err());
    insert(
        &store,
        0,
        "fraction",
        1,
        Some("2026-01-03T00:00:00.100Z"),
        None,
    );
    assert!(read(&store, 1, TD, limits()).is_err());
}
