//! Writes record writer-mutex store time under their route template (aegis-wlf0).
use std::sync::Arc;

use axum::{Router, routing::post};
use quipu::KnowledgeVectorStore;
use serde_json::json;

const CLIENT: &str = "wlf0-write-time";

struct DeferredFixtureProvider {
    fail: bool,
}

impl quipu::embedding::EmbeddingProvider for DeferredFixtureProvider {
    fn embed_text(&self, _text: &str) -> quipu::Result<Vec<f32>> {
        if self.fail {
            Err(quipu::Error::InvalidValue(
                "fixture inference failure".into(),
            ))
        } else {
            Ok(vec![1.0, 0.0, 0.0])
        }
    }

    fn dimension(&self) -> usize {
        3
    }
}

#[test]
fn deferred_embedding_observes_each_taken_writer_window() {
    for fail in [false, true] {
        let mut store = quipu::Store::open_in_memory().unwrap();
        store.shacl_config_mut().validate_on_write = false;
        store.embedding_config_mut().auto_embed = true;
        store.set_embedding_provider(Arc::new(DeferredFixtureProvider { fail }));
        store.set_defer_auto_embed(true);
        let entity = store.intern("urn:test:deferred-observer").unwrap();
        let label = store
            .intern("http://www.w3.org/2000/01/rdf-schema#label")
            .unwrap();
        store
            .transact(
                &[quipu::Datum {
                    entity,
                    attribute: label,
                    value: quipu::Value::Str("nonempty embedding work".into()),
                    valid_from: "2026-01-01T00:00:00Z".into(),
                    valid_to: None,
                    op: quipu::Op::Assert,
                }],
                "2026-01-01T00:00:00Z",
                None,
                None,
            )
            .unwrap();
        let work = store.take_deferred_embed().unwrap();
        assert!(!work.is_empty(), "the fixture must exercise deferred locks");
        let state = Arc::new(crate::StoreHandle::writer_only(store));
        let mut vector_counts = Vec::new();
        let result = super::finish_deferred_embed_observing(
            &state,
            &work,
            |store, wait, acquired, ok, profile| {
                assert!(ok);
                vector_counts.push(store.vector_count().unwrap());
                super::observe_write(
                    store,
                    "bpl-deferred-observer",
                    "/import/promote",
                    wait,
                    acquired,
                    ok,
                    profile,
                );
            },
        );
        if fail {
            assert!(result.is_err());
            assert_eq!(vector_counts, vec![0]);
            assert_eq!(state.lock().vector_count().unwrap(), 0);
        } else {
            result.unwrap();
            assert_eq!(vector_counts, vec![0, 1]);
            assert_eq!(state.lock().vector_count().unwrap(), 1);
        }
    }
}

fn rendered_line(metric: &str, endpoint: &str) -> Option<f64> {
    let key = format!("{metric}{{client=\"{CLIENT}\",endpoint=\"{endpoint}\"}} ");
    quipu::metrics::metrics()
        .render(0, 0, 0, None)
        .lines()
        .find_map(|line| line.strip_prefix(&key).map(|v| v.parse().unwrap()))
}

#[tokio::test]
async fn routed_writes_record_store_time_by_route_template() {
    // Before aegis-wlf0 only /query and the feeds recorded store time, so the
    // write path was invisible to these counters by construction: a /set that
    // held the writer for a second reported nothing at all.
    let state = Arc::new(crate::StoreHandle::writer_only(
        quipu::Store::open_in_memory().unwrap(),
    ));
    state.lock().intern("urn:test:wlf0").unwrap();
    let app = Router::new()
        .route("/set", post(crate::tools::set_predicate))
        .route("/update", post(crate::update::update_post))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let statuses = tokio::task::spawn_blocking(move || {
        let set = ureq::post(&format!("http://{address}/set"))
            .set("x-quipu-client", CLIENT)
            .set("content-type", "application/json")
            .send_string(
                &json!({"entity": "urn:test:wlf0", "predicate": "urn:test:label", "value": "v"})
                    .to_string(),
            )
            .unwrap()
            .status();
        let update = ureq::post(&format!("http://{address}/update"))
            .set("x-quipu-client", CLIENT)
            .set("content-type", "application/sparql-update")
            .send_string("INSERT DATA { <urn:test:wlf0> <urn:test:note> \"n\" }")
            .unwrap()
            .status();
        (set, update)
    })
    .await
    .unwrap();
    server.abort();
    assert_eq!(statuses, (200, 200));

    for endpoint in ["/set", "/update"] {
        // The template, never "unmatched": the label set stays bounded only
        // if the route the request matched is what gets recorded.
        assert!(
            rendered_line("quipu_store_wait_seconds_total", endpoint).is_some(),
            "missing wait series for {endpoint}"
        );
        let held = rendered_line("quipu_store_held_seconds_total", endpoint)
            .unwrap_or_else(|| panic!("missing held series for {endpoint}"));
        assert!(held > 0.0, "{endpoint} held {held}");
    }
    // Writer holds stay out of the per-client histogram, which has no endpoint
    // label and describes read-pool holds.
    let rendered = quipu::metrics::metrics().render(0, 0, 0, None);
    let count = format!("quipu_store_held_seconds_count{{client=\"{CLIENT}\"}}");
    assert!(
        !rendered.lines().any(|line| line.starts_with(&count)),
        "writer time leaked into the held histogram"
    );
}

#[tokio::test]
async fn promotion_records_primary_writer_time_on_success_and_refusal() {
    // A request timer includes admission and lock-free work; these counters
    // must be emitted by the actual promotion lock scope, including errors.
    let share = format!("sha256:{}", "a".repeat(64));
    let mut store = quipu::Store::open_in_memory().unwrap();
    store.embedding_config_mut().auto_embed = false;
    let graph = store
        .graph_create(&format!("urn:quipu:import:staging:{}", &share[7..]))
        .unwrap();
    quipu::rdf::ingest_rdf_to_graph(
        &mut store,
        b"<urn:bpl:entity> <urn:bpl:value> \"promoted\" .".as_slice(),
        oxrdfio::RdfFormat::NTriples,
        None,
        "2026-11-05T00:00:00Z",
        None,
        None,
        graph,
    )
    .unwrap();
    let state = Arc::new(crate::StoreHandle::writer_only(store));
    let endpoint = "/import/promote";
    let before = rendered_line("quipu_store_held_seconds_total", endpoint).unwrap_or(0.0);
    for (share_id, succeeds) in [(share, true), (format!("sha256:{}", "b".repeat(64)), false)] {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-quipu-client", CLIENT.parse().unwrap());
        let request = serde_json::from_value(json!({"share_id": share_id})).unwrap();
        let previous = rendered_line("quipu_store_held_seconds_total", endpoint).unwrap_or(0.0);
        let result = crate::publication::promote_import(
            axum::extract::State(state.clone()),
            headers,
            None,
            axum::Json(request),
        )
        .await;
        assert_eq!(result.is_ok(), succeeds);
        assert_eq!(state.lock().current_facts().unwrap().len(), 1);
        let held = rendered_line("quipu_store_held_seconds_total", endpoint)
            .expect("promotion must expose actual writer hold, including refusal");
        assert!(held > previous && held > before);
        assert!(rendered_line("quipu_store_wait_seconds_total", endpoint).is_some());
    }
}
