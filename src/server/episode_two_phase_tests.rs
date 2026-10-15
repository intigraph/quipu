//! aegis-xi0: `/episode` resolves off the writer.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Resolution that cannot finish until a concurrent write has: it waits
/// for `released`, giving up after five seconds and recording that it did.
struct HeldEmbed {
    released: Arc<AtomicBool>,
    gave_up: Arc<AtomicBool>,
    entered: Arc<AtomicBool>,
}

impl quipu::EmbeddingProvider for HeldEmbed {
    fn embed_text(&self, _: &str) -> quipu::Result<Vec<f32>> {
        self.entered.store(true, Ordering::SeqCst);
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !self.released.load(Ordering::SeqCst) {
            if std::time::Instant::now() > until {
                self.gave_up.store(true, Ordering::SeqCst);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Ok(vec![1.0, 0.0, 0.0])
    }
    fn dimension(&self) -> usize {
        3
    }
}

/// aegis-xi0: a non-strict `/episode` resolves on a reader, so a write can
/// complete while it is still resolving. Before, resolution ran under the
/// single writer: the write could only finish after resolution, so the
/// held embedder would give up instead of being released.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn episode_resolution_does_not_hold_the_writer() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("episode.db");
    let path = db.to_str().unwrap();
    let mut store = quipu::Store::open(path).unwrap();
    store.resolution_config_mut().enabled = true;
    store.embedding_config_mut().auto_embed = false;
    let seed = quipu::Datum {
        entity: store.intern("urn:xi0:concurrent").unwrap(),
        attribute: store
            .intern("http://www.w3.org/2000/01/rdf-schema#label")
            .unwrap(),
        value: quipu::Value::Str("seeded".into()),
        valid_from: "2026-01-01T00:00:00Z".into(),
        valid_to: None,
        op: quipu::Op::Assert,
    };
    store
        .transact(&[seed], "2026-01-01T00:00:00Z", None, None)
        .unwrap();
    let (released, gave_up, entered) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    store.set_embedding_provider(Arc::new(HeldEmbed {
        released: released.clone(),
        gave_up: gave_up.clone(),
        entered: entered.clone(),
    }));
    let readers = super::super::ReadPool::open(path, &store, 1);
    let shared = Arc::new(super::super::StoreHandle::serving(
        store,
        readers,
        path,
        Default::default(),
    ));
    assert!(shared.resolves_off_writer(), "precondition: pooled reader");
    let input = serde_json::json!({"name": "held-batch", "edges": [],
        "nodes": [{"name": "held-document", "type": "Document"}]});

    let episode = tokio::spawn(super::super::tools::episode(
        axum::extract::State(shared.clone()),
        axum::Json(input),
    ));
    while !entered.load(Ordering::SeqCst) {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let _set = super::super::tools::set_predicate(
        axum::extract::State(shared.clone()),
        axum::Json(serde_json::json!({"entity": "urn:xi0:concurrent",
            "predicate": "http://www.w3.org/2000/01/rdf-schema#label", "value": "concurrent"})),
    )
    .await
    .unwrap();
    released.store(true, Ordering::SeqCst);
    let created = episode.await.unwrap().unwrap().0;

    assert_eq!(created["outcome"], "created");
    assert!(
        !gave_up.load(Ordering::SeqCst),
        "the write could not complete while the episode was resolving"
    );
}
