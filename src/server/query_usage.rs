//! Request-usage annotations attached by the query handler.

use std::time::Instant;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Server-issued request numbers restart with the process; log both identities.
pub(super) fn with_process_id(log: String) -> String {
    let Ok(mut record) = serde_json::from_str::<Value>(&log) else {
        return log;
    };
    if !record.is_object() {
        return log;
    }
    record["server_process_id"] = json!(std::process::id());
    record.to_string()
}

/// Log-only query correlation. Never export these high-cardinality values as
/// metric labels, or describe a digest as anonymization of private query text.
pub(super) struct Observation {
    identity: Value,
    started: Instant,
    dispatched_ms: Option<f64>,
    admission_stamp: Option<super::admission::AdmissionStamp>,
    wait_started: Option<Instant>,
    wait_ms: Option<f64>,
    acquired: Option<Instant>,
    held_ms: Option<f64>,
    statements: Option<quipu::statement_profile::Totals>,
    outcome: &'static str,
    emit: Box<dyn FnMut(Value) + Send>,
}

impl Observation {
    pub(super) fn new(input: &Value, request_id: Option<u64>, log_admission: bool) -> Self {
        Self::with_sink(
            input,
            request_id,
            log_admission,
            Box::new(|record| eprintln!("{record}")),
        )
    }

    fn with_sink(
        input: &Value,
        request_id: Option<u64>,
        log_admission: bool,
        emit: Box<dyn FnMut(Value) + Send>,
    ) -> Self {
        let serialized = input.to_string();
        let query = input.get("query").and_then(Value::as_str);
        let digest = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
        let mut observation = Self {
            identity: json!({
                "server_process_id": std::process::id(),
                "request_id": request_id,
                "query_sha256": query.map(|q| digest(q.as_bytes())),
                "query_bytes": query.map(str::len),
                // Includes graph/temporal/protocol options; same query alone
                // is not proof of the same logical read.
                "input_sha256": digest(serialized.as_bytes()),
                "input_bytes": serialized.len(),
            }),
            started: Instant::now(),
            dispatched_ms: None,
            admission_stamp: log_admission.then(|| std::sync::Arc::new(std::sync::OnceLock::new())),
            wait_started: None,
            wait_ms: None,
            acquired: None,
            held_ms: None,
            statements: None,
            outcome: "not_started",
            emit,
        };
        let record = observation.record("query_observation_start");
        (observation.emit)(record);
        observation
    }

    pub(super) fn capture(
        &self,
        capture: &super::query_capture::Capture,
        input: &Value,
        client: &str,
        is_post: bool,
    ) {
        capture.observe(input, &self.identity, client, is_post);
    }

    pub(super) fn dispatched(&mut self) {
        self.dispatched_ms = Some(self.started.elapsed().as_secs_f64() * 1_000.0);
        self.outcome = "unfinished";
    }

    pub(super) fn admission_stamp(&self) -> Option<super::admission::AdmissionStamp> {
        self.admission_stamp.clone()
    }

    pub(super) fn waiting(&mut self) {
        self.wait_started = Some(Instant::now());
    }

    pub(super) fn acquired(&mut self) {
        self.wait_ms = self
            .wait_started
            .map(|t| t.elapsed().as_secs_f64() * 1_000.0);
        self.acquired = Some(Instant::now());
    }

    /// `SQLite` statement totals for this query, when `log_query_phases` is on.
    pub(super) fn statements(&mut self, totals: Option<quipu::statement_profile::Totals>) {
        self.statements = totals;
    }

    pub(super) fn finished(&mut self, ok: bool) {
        self.held_ms = self.acquired.map(|t| t.elapsed().as_secs_f64() * 1_000.0);
        self.outcome = if ok { "ok" } else { "error" };
    }

    fn record(&self, event: &str) -> Value {
        let mut record = self.identity.clone();
        record["timestamp"] = json!(quipu::time::now_iso());
        record["event"] = json!(event);
        record["dispatch_ms"] = json!(self.dispatched_ms);
        if let Some(stamp) = &self.admission_stamp {
            // Same origin as dispatch_ms. Missing acquisition stays null,
            // including cancellation while still waiting for a permit.
            record["admission_ms"] =
                json!(stamp.get().map(|acquired| {
                    acquired.duration_since(self.started).as_secs_f64() * 1_000.0
                }));
        }
        record["store_wait_ms"] = json!(self.wait_ms);
        record["store_held_ms"] = json!(self.held_ms);
        record["worker_outcome"] = json!(self.outcome);
        if let Some(totals) = &self.statements {
            // Compare with store_held_ms: the remainder is spent outside SQLite.
            record["sqlite_statements"] = json!(totals.statements);
            record["sqlite_rows"] = json!(totals.rows);
            record["sqlite_ms"] = json!(nanos_ms(totals.nanos));
            record["sqlite_top"] = json!(
                totals
                    .top(3)
                    .into_iter()
                    .map(|(sql, count, nanos)| json!({"sql": sql, "count": count, "ms": nanos_ms(nanos)}))
                    .collect::<Vec<_>>()
            );
        }
        record
    }
}

/// Evaluate with `SQLite` statement totals armed when `enabled`, and record
/// them on `observation` whatever `evaluate` returns, including an outer error
/// such as an unready substring index. The span guard disarms the thread on
/// every exit, unwind included, so a reused blocking worker never keeps
/// counting after the query (harding, #702).
pub(super) fn profiled<T>(
    observation: &mut Observation,
    enabled: bool,
    evaluate: impl FnOnce() -> T,
) -> T {
    let span = enabled.then(quipu::statement_profile::Span::arm);
    let out = evaluate();
    if let Some(span) = span {
        observation.statements(span.finish());
    }
    out
}

#[allow(clippy::cast_precision_loss)]
fn nanos_ms(nanos: u64) -> f64 {
    nanos as f64 / 1_000_000.0
}

impl Drop for Observation {
    fn drop(&mut self) {
        // This object moves into the blocking closure. Its completion records
        // work ending, even if the HTTP future already logged a 499/408. An
        // unstarted closure instead retains null timings and not_started.
        if self.held_ms.is_none() {
            self.held_ms = self.acquired.map(|t| t.elapsed().as_secs_f64() * 1_000.0);
        }
        let record = self.record("query_observation_end");
        (self.emit)(record);
    }
}

pub(crate) fn client(headers: &axum::http::HeaderMap) -> String {
    quipu::metrics::normalize_client(
        headers.get("x-quipu-client").and_then(|v| v.to_str().ok()),
        headers
            .get(axum::http::header::USER_AGENT)
            .and_then(|v| v.to_str().ok()),
    )
}

pub(crate) fn query_text(input: &serde_json::Value) -> String {
    input
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("<no query field>")
        .chars()
        .take(300)
        .collect()
}

pub(crate) fn annotate(
    mut response: axum::response::Response,
    query_shape: quipu::request_usage::QueryShape,
    result_size: usize,
) -> axum::response::Response {
    response
        .extensions_mut()
        .insert(quipu::request_usage::RequestUsage {
            query_shape,
            result_size: Some(result_size),
        });
    response
}

pub(crate) fn result_size(result: &quipu::QueryResult) -> usize {
    match result {
        quipu::QueryResult::Select { rows, .. } => rows.len(),
        quipu::QueryResult::Graph(triples) => triples.len(),
        quipu::QueryResult::Ask(_) => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn http_query_observation_demo_has_a_real_result_positive_control() {
        http_query_demo(false).await;
    }

    #[tokio::test]
    async fn http_query_admission_enabled_demo_has_a_real_result_positive_control() {
        http_query_demo(true).await;
    }

    async fn http_query_demo(log_admission: bool) {
        use tower::ServiceExt;

        let mut store = quipu::Store::open_in_memory().unwrap();
        quipu::tool_knot(
            &mut store,
            &json!({"turtle":"<urn:demo:s> <urn:demo:p> <urn:demo:o> ."}),
        )
        .unwrap();
        let mut handle = crate::StoreHandle::writer_only(store);
        handle.log_query_admission = log_admission;
        let shared: crate::SharedStore = Arc::new(handle);
        let app = axum::Router::new()
            .route("/query", axum::routing::post(crate::query))
            .with_state(shared)
            .layer(axum::middleware::from_fn(
                crate::request_middleware::log_request,
            ));
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/query")
                    .header("content-type", "application/json")
                    .header("x-quipu-client", "demo")
                    .body(axum::body::Body::from(
                        json!({"query":"SELECT ?s WHERE { ?s <urn:demo:p> <urn:demo:o> }"})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["count"], 1);
        eprintln!("PRIVATE_HTTP_CONTROL status=200 count=1 admission_logging={log_admission}");
    }

    fn capture(input: &Value, id: Option<u64>) -> (Observation, Arc<Mutex<Vec<Value>>>) {
        let records = Arc::new(Mutex::new(Vec::new()));
        let sink = records.clone();
        let observation = Observation::with_sink(
            input,
            id,
            false,
            Box::new(move |record| sink.lock().unwrap().push(record)),
        );
        (observation, records)
    }

    #[test]
    fn exact_query_and_input_digests_do_not_stop_at_the_legacy_prefix() {
        let prefix = " ".repeat(300);
        let query = format!("{prefix}ASK {{ <urn:a> <urn:b> <urn:c> }}");
        let (a, ar) = capture(&json!({"query":query,"graph":"urn:one"}), Some(7));
        let (b, br) = capture(
            &json!({"query":format!("{query} # changed"),"graph":"urn:one"}),
            Some(8),
        );
        let (c, cr) = capture(&json!({"query":query,"graph":"urn:two"}), Some(9));
        let (same, sr) = capture(&json!({"query":query,"graph":"urn:one"}), Some(10));
        drop((a, b, c, same));
        let a = ar.lock().unwrap();
        let b = br.lock().unwrap();
        let c = cr.lock().unwrap();
        let same = sr.lock().unwrap();
        assert_ne!(a[0]["query_sha256"], b[0]["query_sha256"]);
        assert_eq!(a[0]["query_sha256"], c[0]["query_sha256"]);
        assert_ne!(a[0]["input_sha256"], c[0]["input_sha256"]);
        assert_eq!(a[0]["input_sha256"], same[0]["input_sha256"]);
        assert_eq!(a[0]["query_sha256"].as_str().unwrap().len(), 64);
    }

    #[test]
    fn lifecycle_identity_timings_and_body_omission_have_positive_controls() {
        let (mut observation, records) = capture(
            &json!({"query":"ASK { <urn:private-marker> ?p ?o }","graph":"urn:secret-marker"}),
            Some(42),
        );
        observation.dispatched();
        observation.waiting();
        observation.acquired();
        observation.finished(false);
        drop(observation);
        let records = records.lock().unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["event"], "query_observation_start");
        assert_eq!(records[1]["event"], "query_observation_end");
        for record in records.iter() {
            assert_eq!(record["request_id"], 42);
            assert_eq!(record["server_process_id"], std::process::id());
            let text = record.to_string();
            assert!(!text.contains("private-marker"));
            assert!(!text.contains("secret-marker"));
        }
        for key in ["dispatch_ms", "store_wait_ms", "store_held_ms"] {
            assert!(records[0][key].is_null());
            assert!(records[1][key].as_f64().unwrap() >= 0.0);
        }
        assert_eq!(records[1]["worker_outcome"], "error");
        let outer: Value = serde_json::from_str(&with_process_id(
            json!({"request_id":42,"event":"request_complete"}).to_string(),
        ))
        .unwrap();
        assert_eq!(outer["server_process_id"], records[1]["server_process_id"]);
        assert_eq!(outer["request_id"], records[1]["request_id"]);
    }

    #[test]
    fn unstarted_and_early_exit_work_keep_unknown_timings_explicit() {
        let (observation, records) = capture(&json!({}), None);
        drop(observation);
        let records = records.lock().unwrap();
        assert!(records[1]["request_id"].is_null());
        assert!(records[1]["query_sha256"].is_null());
        assert_eq!(records[1]["worker_outcome"], "not_started");
        assert!(records[1]["store_wait_ms"].is_null());
        assert!(records[1]["store_held_ms"].is_null());
        let (mut observation, records) = capture(&json!({"query":"ASK {}"}), Some(1));
        observation.dispatched();
        observation.waiting();
        observation.acquired();
        drop(observation);
        assert_eq!(records.lock().unwrap()[1]["worker_outcome"], "unfinished");
    }

    #[tokio::test]
    async fn blocking_worker_observation_survives_abandoned_http_future() {
        let (observation, records) = capture(&json!({"query":"ASK {}"}), Some(19));
        let entered = Arc::new(tokio::sync::Notify::new());
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let worker_entered = entered.clone();
        let future = tokio::spawn(async move {
            tokio::task::spawn_blocking(move || {
                let mut observation = observation;
                observation.dispatched();
                observation.waiting();
                observation.acquired();
                worker_entered.notify_one();
                release_rx
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .unwrap();
                observation.finished(true);
                drop(observation);
                let _ = done_tx.send(());
            })
            .await
            .unwrap();
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
            .await
            .unwrap();
        future.abort();
        assert!(future.await.unwrap_err().is_cancelled());
        release_tx.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), done_rx)
            .await
            .unwrap()
            .unwrap();
        let records = records.lock().unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[1]["worker_outcome"], "ok");
    }

    /// aegis-xacr: statement totals reach the end record, and the statement
    /// texts they name carry no query constants (the log keeps digests only).
    #[test]
    fn statement_totals_reach_the_end_record_without_query_constants() {
        let mut store = quipu::Store::open_in_memory().unwrap();
        quipu::tool_knot(
            &mut store,
            &json!({"turtle":"<urn:private-marker:s> <urn:private-marker:p> \"secret-literal\" ."}),
        )
        .unwrap();
        store.enable_statement_profile();
        let query = "SELECT ?o WHERE { <urn:private-marker:s> <urn:private-marker:p> ?o \
                     FILTER(?o = \"secret-literal\") }";
        let (mut observation, records) = capture(&json!({"query":query}), Some(5));
        quipu::statement_profile::begin();
        let result = quipu::tool_query(&store, &json!({"query":query})).unwrap();
        observation.statements(quipu::statement_profile::take());
        observation.finished(true);
        drop(observation);
        assert_eq!(result["count"], 1, "positive control: the query answered");
        let records = records.lock().unwrap();
        assert!(records[0].get("sqlite_statements").is_none());
        let end = &records[1];
        assert!(end["sqlite_statements"].as_u64().unwrap() > 0);
        assert!(end["sqlite_rows"].as_u64().unwrap() > 0);
        assert!(end["sqlite_ms"].as_f64().unwrap() >= 0.0);
        let top = end["sqlite_top"].as_array().unwrap();
        assert!(!top.is_empty() && top.len() <= 3);
        let text = end.to_string();
        assert!(!text.contains("private-marker"), "{text}");
        assert!(!text.contains("secret-literal"), "{text}");
    }

    fn profiled_store() -> quipu::Store {
        let mut store = quipu::Store::open_in_memory().unwrap();
        quipu::tool_knot(
            &mut store,
            &json!({"turtle":"<urn:demo:s> <urn:demo:p> <urn:demo:o> ."}),
        )
        .unwrap();
        store.enable_statement_profile();
        store
    }

    /// harding #702: a substring outer error (no prepared index) exits the
    /// endpoint closure through `?`. The thread must be disarmed, the totals
    /// recorded, and an unrelated later operation on the thread not counted.
    #[test]
    fn a_substring_outer_error_records_totals_and_disarms_the_thread() {
        let store = profiled_store();
        let (mut observation, records) = capture(&json!({"query":"ASK {}"}), Some(7));
        let mut evaluated = false;
        let outer = profiled(&mut observation, true, || {
            store.substring_query(|| {
                evaluated = true;
            })
        });
        assert!(
            outer.is_err(),
            "positive control: the index is not prepared"
        );
        assert!(!evaluated, "the outer error came before evaluation");
        assert!(!quipu::statement_profile::armed());
        observation.finished(false);
        drop(observation);
        let end = records.lock().unwrap()[1].clone();
        assert!(end["sqlite_statements"].as_u64().unwrap() > 0, "{end}");
        quipu::tool_query(&store, &json!({"query":"SELECT ?s WHERE { ?s ?p ?o }"})).unwrap();
        assert!(quipu::statement_profile::take().is_none());
    }

    #[test]
    fn an_unwinding_evaluation_disarms_the_thread() {
        let store = profiled_store();
        let (mut observation, _records) = capture(&json!({"query":"ASK {}"}), Some(8));
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            profiled(&mut observation, true, || {
                quipu::tool_query(&store, &json!({"query":"ASK { ?s ?p ?o }"})).unwrap();
                panic!("evaluation failed");
            })
        }));
        assert!(unwound.is_err());
        assert!(!quipu::statement_profile::armed());
        quipu::tool_query(&store, &json!({"query":"SELECT ?s WHERE { ?s ?p ?o }"})).unwrap();
        assert!(quipu::statement_profile::take().is_none());
    }

    #[test]
    fn profiling_off_arms_nothing_and_records_nothing() {
        let store = profiled_store();
        let (mut observation, records) = capture(&json!({"query":"ASK {}"}), Some(9));
        profiled(&mut observation, false, || {
            assert!(!quipu::statement_profile::armed());
            quipu::tool_query(&store, &json!({"query":"ASK { ?s ?p ?o }"})).unwrap();
        });
        observation.finished(true);
        drop(observation);
        assert!(
            records.lock().unwrap()[1]
                .get("sqlite_statements")
                .is_none()
        );
    }

    #[test]
    fn statement_totals_are_absent_unless_recorded() {
        let (mut observation, records) = capture(&json!({"query":"ASK {}"}), Some(6));
        observation.finished(true);
        drop(observation);
        assert!(
            records.lock().unwrap()[1]
                .get("sqlite_statements")
                .is_none()
        );
    }
}

#[cfg(test)]
#[path = "query_usage/admission_tests.rs"]
mod admission_tests;
