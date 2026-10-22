//! aegis-oa5: `/import` resolves off the writer.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use quipu::share::{SHARE_PAYLOAD_MAX_BYTES, ShareOptions, ShareScope};

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

fn share_request() -> quipu::share_import::ShareImportRequest {
    let ts = "2026-10-22T00:00:00Z";
    let mut producer = quipu::Store::open_in_memory().unwrap();
    let catalogue = producer.overlay_create("urn:test:catalogue", 0).unwrap();
    quipu::rdf::ingest_rdf_to_graph(
        &mut producer,
        include_bytes!("../../tests/fixtures/share-catalogue.ttl").as_slice(),
        oxrdfio::RdfFormat::Turtle,
        None,
        ts,
        None,
        None,
        catalogue,
    )
    .unwrap();
    let graph = producer.graph_create("urn:oa5:repository").unwrap();
    quipu::rdf::ingest_rdf_to_graph(
        &mut producer,
        b"<urn:oa5:foreign> <http://www.w3.org/2000/01/rdf-schema#label> \"change-event-00196ae7\" ."
            .as_slice(),
        oxrdfio::RdfFormat::NTriples,
        None,
        ts,
        None,
        None,
        graph,
    )
    .unwrap();
    let payload = quipu::share::share_payload(
        &producer,
        &ShareOptions {
            scope: ShareScope::Graph("urn:oa5:repository".into()),
            no_shapes: true,
            ..Default::default()
        },
        SHARE_PAYLOAD_MAX_BYTES,
    )
    .unwrap();
    serde_json::from_value(serde_json::json!({
        "manifest": payload.manifest, "export_ntriples": payload.files["export.nt"],
        "shapes_turtle": payload.files["shapes.ttl"], "source": "aegis-oa5 two-phase"
    }))
    .unwrap()
}

/// aegis-oa5: an `/import` resolves its labels on a reader, so a write can
/// complete while it is still resolving. Before, resolution ran under the
/// single writer: one 25k-triple share held production writes for over 30
/// minutes, and here the held embedder would give up instead of being released.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn import_resolution_does_not_hold_the_writer() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("import.db");
    let path = db.to_str().unwrap();
    let mut store = quipu::Store::open(path).unwrap();
    store.embedding_config_mut().auto_embed = false;
    let seed = quipu::Datum {
        entity: store.intern("urn:oa5:concurrent").unwrap(),
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

    let import = tokio::spawn(super::super::publication::import_share(
        axum::extract::State(shared.clone()),
        None,
        axum::Json(share_request()),
    ));
    while !entered.load(Ordering::SeqCst) {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let _set = super::super::tools::set_predicate(
        axum::extract::State(shared.clone()),
        axum::Json(serde_json::json!({"entity": "urn:oa5:concurrent",
            "predicate": "http://www.w3.org/2000/01/rdf-schema#label", "value": "concurrent"})),
    )
    .await
    .unwrap();
    released.store(true, Ordering::SeqCst);
    let imported = import.await.unwrap().unwrap().0;

    assert!(
        matches!(imported["outcome"].as_str(), Some("staged" | "quarantined")),
        "{imported}"
    );
    assert_eq!(
        imported["resolution"]["unmatched"],
        serde_json::json!(["urn:oa5:foreign"])
    );
    assert!(
        !gave_up.load(Ordering::SeqCst),
        "the write could not complete while the import was resolving"
    );
}
