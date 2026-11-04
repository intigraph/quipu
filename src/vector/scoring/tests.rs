use super::cosine_bytes;
use crate::vector::{bytes_to_f32_slice, cosine_similarity, f32_slice_to_bytes, squared_norm};

fn same_score(query: &[f32], bytes: &[u8]) {
    let old = cosine_similarity(query, &bytes_to_f32_slice(bytes));
    let new = cosine_bytes(query, bytes, squared_norm(query).sqrt());
    if old.is_nan() {
        assert!(new.is_nan());
    } else {
        assert_eq!(old.to_bits(), new.to_bits());
    }
}

#[test]
fn byte_scoring_keeps_exact_f64_scores_and_legacy_blob_boundaries() {
    let mut state = 17_u64;
    for dimensions in [0, 1, 3, 8, 384, 768, 1536] {
        for _ in 0..20 {
            let mut vector = || {
                (0..dimensions)
                    .map(|_| {
                        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                        ((state >> 32) as i32) as f32 / i32::MAX as f32
                    })
                    .collect::<Vec<_>>()
            };
            let query = vector();
            let values = vector();
            let mut bytes = f32_slice_to_bytes(&values);
            same_score(&query, &bytes);
            // The existing decoder discards an incomplete final coordinate.
            bytes.extend_from_slice(&[1, 2, 3]);
            same_score(&query, &bytes);
            same_score(&[], &bytes);
        }
    }
}

#[test]
fn zero_nonfinite_and_mismatched_inputs_keep_the_old_semantics() {
    for query in [vec![], vec![0.0, -0.0], vec![1.0, 2.0], vec![f32::NAN, 1.0]] {
        for values in [vec![], vec![0.0, -0.0], vec![1.0], vec![f32::INFINITY, 2.0]] {
            same_score(&query, &f32_slice_to_bytes(&values));
        }
    }
}

#[test]
fn a_non_blob_keeps_the_sqlite_column_type_error() {
    use crate::vector::KnowledgeVectorStore;

    let store = crate::Store::open_in_memory().unwrap();
    store
        .conn
        .execute(
            "INSERT INTO vectors VALUES(1,'fixture','not a blob','2026-01-01',NULL)",
            [],
        )
        .unwrap();
    let error = store.vector_search(&[1.0], 1, None).unwrap_err();
    assert!(matches!(error, crate::Error::Sqlite(
        rusqlite::Error::InvalidColumnType(1, ref name, rusqlite::types::Type::Text)
    ) if name == "embedding"));
}
