//! Paired exact paths, using SQLite's real index and transaction maintenance.
use super::*;

type MatchSignature = (i64, u64, String, String, Option<String>);

fn signature(matches: Vec<Vec<VectorMatch>>) -> Vec<Vec<MatchSignature>> {
    matches
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|m| {
                    (
                        m.entity_id,
                        m.score.to_bits(),
                        m.text,
                        m.valid_from,
                        m.valid_to,
                    )
                })
                .collect()
        })
        .collect()
}

fn paired(store: &Store, queries: &[&[f32]], limit: usize, at: Option<&str>) {
    store
        .conn
        .execute_batch("DROP INDEX IF EXISTS idx_vectors_current_payload_v1")
        .unwrap();
    let old = store.vector_search_batch(queries, limit, at);
    store.conn.execute_batch(VECTORS_SQL).unwrap();
    let new = store.vector_search_batch(queries, limit, at);
    match (old, new) {
        (Ok(old), Ok(new)) => assert_eq!(signature(old), signature(new)),
        (Err(old), Err(new)) => assert_eq!(old.to_string(), new.to_string()),
        other => panic!("path disagreement: {other:?}"),
    }
}

#[test]
fn current_cover_preserves_row_order_score_bits_and_metadata() {
    let store = Store::open_in_memory().unwrap();
    // Insertion order differs from the covering index's entity-id order.
    for id in (1..=70).rev() {
        let vector = match id % 4 {
            0 => vec![1.0, 0.0, 0.0],
            1 => vec![0.1, -0.2, 0.3],
            2 => vec![0.0, 0.0, 0.0],
            _ => vec![-0.7, 1e-20, 2.3],
        };
        store
            .embed_entity(id, &"large text".repeat(2000), &vector, "2026-01-01")
            .unwrap();
    }
    let a = [1.0, 0.0, 0.0];
    let b = [0.1, -0.2, 0.3];
    let zero = [0.0; 3];
    for limit in [0, 1, 3, 17, 200] {
        paired(&store, &[&a, &b, &zero], limit, None);
        paired(&store, &[&a, &b], limit, Some("2026-01-02"));
    }
    paired(&store, &[], 3, None);
}

#[test]
fn current_cover_tracks_close_replace_and_rollback() {
    let store = Store::open_in_memory().unwrap();
    store
        .embed_entity(3, "first", &[1.0, 0.0], "2026-01-01")
        .unwrap();
    store
        .embed_entity(2, "second", &[1.0, 0.0], "2026-01-01")
        .unwrap();
    store.close_embedding(3, "2026-02-01").unwrap();
    store
        .embed_entity(3, "replacement", &[0.0, 1.0], "2026-02-01")
        .unwrap();
    let query = [1.0, 0.0];
    paired(&store, &[&query], 4, None);
    paired(&store, &[&query], 4, Some("2026-01-15"));
    let before = signature(store.vector_search_batch(&[&query], 4, None).unwrap());
    store
        .conn
        .execute_batch("SAVEPOINT private_vector_change")
        .unwrap();
    store.close_embedding(2, "2026-03-01").unwrap();
    store
        .embed_entity(9, "transient", &query, "2026-03-01")
        .unwrap();
    store
        .conn
        .execute_batch("ROLLBACK TO private_vector_change; RELEASE private_vector_change")
        .unwrap();
    assert_eq!(
        before,
        signature(store.vector_search_batch(&[&query], 4, None).unwrap())
    );
    paired(&store, &[&query], 4, None);
}

#[test]
fn current_cover_retains_unordered_scores_empty_vectors_and_error_order() {
    let store = Store::open_in_memory().unwrap();
    store.embed_entity(9, "empty", &[], "2026-01-01").unwrap();
    store
        .embed_entity(5, "nan", &[f32::NAN, 1.0], "2026-01-01")
        .unwrap();
    store
        .embed_entity(1, "finite", &[1.0, 0.0], "2026-01-01")
        .unwrap();
    for limit in [0, 1, 2, 10] {
        paired(&store, &[&[1.0, 0.0]], limit, None);
    }
    // The FIRST offending entity/error is preserved despite index ordering.
    paired(&store, &[&[1.0]], 2, None);
}

#[test]
fn analyzed_cover_cannot_reorder_other_current_search_paths() {
    let store = Store::open_in_memory().unwrap();
    // Reversed entity insertion and multiple current rows also stress metadata.
    for (id, since, value) in [
        (9, "2026-02-01", 1.0),
        (9, "2026-01-01", -1.0),
        (2, "2026-01-01", 1.0),
        (1, "2026-01-01", 1.0),
    ] {
        store.embed_entity(id, since, &[value, 0.0], since).unwrap();
    }
    store
        .conn
        .execute_batch("DROP INDEX idx_vectors_current_payload_v1")
        .unwrap();
    let query = [1.0, 0.0];
    let old_single = signature(vec![store.vector_search(&query, 10, None).unwrap()]);
    store.conn.execute_batch(VECTORS_SQL).unwrap();
    store.conn.execute_batch("ANALYZE").unwrap();
    assert_eq!(
        old_single,
        signature(vec![store.vector_search(&query, 10, None).unwrap()])
    );
    assert_eq!(
        old_single,
        signature(store.vector_search_batch(&[&query], 10, None).unwrap())
    );
}
