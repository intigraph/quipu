//! Tests for the SQLite-backed knowledge vector store.

use super::*;

#[test]
fn expired_batch_scan_refuses_even_without_rows() {
    let store = Store::open_in_memory().unwrap();
    let query = [1.0_f32; 8];
    let _deadline = crate::time::set_request_deadline(Some(crate::time::Deadline::after_millis(0)));
    let result = store.vector_search_batch(&[&query], 1, None);
    assert!(
        matches!(result, Err(crate::Error::RequestTimeout { .. })),
        "{result:?}"
    );
}

fn make_embedding(seed: f32, dim: usize) -> Vec<f32> {
    (0..dim).map(|i| (seed + i as f32 * 0.1).sin()).collect()
}

#[test]
fn embed_and_search() {
    let store = Store::open_in_memory().unwrap();

    let e1 = store.intern("http://example.org/alice").unwrap();
    let e2 = store.intern("http://example.org/bob").unwrap();
    let e3 = store.intern("http://example.org/carol").unwrap();

    let emb1 = make_embedding(1.0, 8);
    let emb2 = make_embedding(1.1, 8); // similar to emb1
    let emb3 = make_embedding(5.0, 8); // different

    store
        .embed_entity(e1, "Alice the engineer", &emb1, "2026-01-01")
        .unwrap();
    store
        .embed_entity(e2, "Bob the developer", &emb2, "2026-01-01")
        .unwrap();
    store
        .embed_entity(e3, "Carol the manager", &emb3, "2026-01-01")
        .unwrap();

    assert_eq!(store.vector_count().unwrap(), 3);

    // Search with emb1 — Alice should be top match, Bob close second.
    let results = store.vector_search(&emb1, 3, None).unwrap();
    assert_eq!(results.len(), 3);
    assert_eq!(results[0].entity_id, e1); // Alice = exact match
    assert_eq!(results[1].entity_id, e2); // Bob = similar
    assert!(results[0].score > results[1].score);
    assert!(results[1].score > results[2].score);
}

#[test]
fn dimension_mismatch_fails_loud() {
    // hq-7v0: searching with a query whose dim differs from the stored
    // vectors must error, not silently score every candidate 0.0.
    let store = Store::open_in_memory().unwrap();
    let e1 = store.intern("http://example.org/alice").unwrap();
    store
        .embed_entity(e1, "Alice", &make_embedding(1.0, 8), "2026-01-01")
        .unwrap();

    // 4-dim query vs 8-dim stored → loud error mentioning the dimensions.
    let err = store
        .vector_search(&make_embedding(1.0, 4), 3, None)
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("dimension mismatch"), "got: {msg}");

    // Matching dimension still works.
    assert_eq!(
        store
            .vector_search(&make_embedding(1.0, 8), 3, None)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn temporal_vector_search() {
    let store = Store::open_in_memory().unwrap();

    let e1 = store.intern("http://example.org/svc").unwrap();
    let emb_old = make_embedding(1.0, 8);
    let emb_new = make_embedding(2.0, 8);

    // Old embedding, valid until March.
    store
        .embed_entity(e1, "old description", &emb_old, "2026-01-01")
        .unwrap();
    store.close_embedding(e1, "2026-03-01").unwrap();

    // New embedding, current.
    store
        .embed_entity(e1, "new description", &emb_new, "2026-03-01")
        .unwrap();

    // Current search: only new embedding.
    let results = store.vector_search(&emb_old, 10, None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].text, "new description");

    // Time-travel search to February: only old embedding.
    let results = store
        .vector_search(&emb_old, 10, Some("2026-02-01"))
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].text, "old description");

    // Time-travel to April: only new embedding.
    let results = store
        .vector_search(&emb_new, 10, Some("2026-04-01"))
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].text, "new description");
}

#[test]
fn cosine_similarity_self() {
    let v = vec![1.0, 2.0, 3.0];
    let sim = cosine_similarity(&v, &v);
    assert!((sim - 1.0).abs() < 1e-10);
}

#[test]
fn cosine_similarity_orthogonal() {
    let a = vec![1.0, 0.0];
    let b = vec![0.0, 1.0];
    let sim = cosine_similarity(&a, &b);
    assert!(sim.abs() < 1e-10);
}

#[test]
fn embedding_round_trip() {
    let original: Vec<f32> = vec![1.0, -2.5, 3.25, 0.0, f32::MAX, f32::MIN];
    let bytes = f32_slice_to_bytes(&original);
    let decoded = bytes_to_f32_slice(&bytes);
    assert_eq!(original, decoded);
}

#[test]
fn limit_results() {
    let store = Store::open_in_memory().unwrap();

    for i in 0..20 {
        let eid = store.intern(&format!("http://example.org/e{i}")).unwrap();
        let emb = make_embedding(i as f32, 8);
        store
            .embed_entity(eid, &format!("entity {i}"), &emb, "2026-01-01")
            .unwrap();
    }

    let query = make_embedding(0.0, 8);
    let results = store.vector_search(&query, 5, None).unwrap();
    assert_eq!(results.len(), 5);
}

#[test]
fn vector_store_trait_object() {
    let store = Store::open_in_memory().unwrap();
    let vs: &dyn KnowledgeVectorStore = store.vector_store();

    let eid = store.intern("http://example.org/test").unwrap();
    let emb = make_embedding(1.0, 8);

    vs.embed_entity(eid, "test entity", &emb, "2026-01-01")
        .unwrap();
    assert_eq!(vs.vector_count().unwrap(), 1);

    let results = vs.vector_search(&emb, 10, None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].entity_id, eid);
}

/// `vector_search_batch` on the built-in SQLite backend must return, for every
/// query, exactly what `vector_search` returns for that query alone: same
/// entities, same scores, same order, ties included (aegis-nmuwsi).
#[test]
fn batch_search_equals_one_search_per_query_including_ties() {
    let store = Store::open_in_memory().unwrap();
    let dims = 4;
    // Deterministic vectors with deliberate exact duplicates, so ties exist.
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        #[allow(clippy::cast_precision_loss)]
        let x = (seed % 2001) as f32 / 1000.0 - 1.0;
        x
    };
    let mut stored: Vec<Vec<f32>> = Vec::new();
    for id in 1..=300_i64 {
        let v: Vec<f32> = if id % 7 == 0 {
            stored[usize::try_from(id / 2).unwrap()].clone() // exact duplicate -> tie
        } else {
            (0..dims).map(|_| next()).collect()
        };
        store
            .embed_entity(id, &format!("entity {id}"), &v, "2026-01-01T00:00:00Z")
            .unwrap();
        stored.push(v);
    }
    let queries: Vec<Vec<f32>> = vec![
        stored[0].clone(),
        stored[13].clone(),
        (0..dims).map(|_| next()).collect(),
        vec![1.0, 0.0, 0.0, 0.0],
    ];
    let refs: Vec<&[f32]> = queries.iter().map(Vec::as_slice).collect();
    for limit in [0, 1, 3, 15, 299, 1000] {
        let batch = store.vector_search_batch(&refs, limit, None).unwrap();
        assert_eq!(batch.len(), queries.len());
        for (q, got) in refs.iter().zip(&batch) {
            let want = store.vector_search(q, limit, None).unwrap();
            let got: Vec<(i64, f64, &str)> = got
                .iter()
                .map(|m| (m.entity_id, m.score, m.text.as_str()))
                .collect();
            let want: Vec<(i64, f64, &str)> = want
                .iter()
                .map(|m| (m.entity_id, m.score, m.text.as_str()))
                .collect();
            assert_eq!(got, want, "limit {limit}");
        }
    }
    assert!(store.vector_search_batch(&[], 5, None).unwrap().is_empty());
}

/// The batch scan's precomputed-norm cosine must be bit-identical to
/// `cosine_similarity`, or batch and single searches could rank differently.
#[test]
fn cosine_with_norms_is_bit_identical_to_cosine_similarity() {
    let vs: Vec<Vec<f32>> = vec![
        vec![0.1, -0.2, 0.3, 0.4],
        vec![1e-20, 3.5, -7.25, 0.0],
        vec![0.0, 0.0, 0.0, 0.0],
        vec![-1.0, 1.0, -1.0, 1.0],
        make_embedding(2.5, 4),
    ];
    for a in &vs {
        for b in &vs {
            let want = cosine_similarity(a, b);
            let got = cosine_with_norms(a, b, squared_norm(a), squared_norm(b));
            assert_eq!(got.to_bits(), want.to_bits(), "{a:?} vs {b:?}");
        }
    }
}
