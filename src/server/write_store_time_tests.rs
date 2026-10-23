//! Writes record writer-mutex store time under their route template (aegis-wlf0).
use std::sync::Arc;

use axum::{Router, routing::post};
use serde_json::json;

const CLIENT: &str = "wlf0-write-time";

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
