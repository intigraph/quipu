use super::*;
use std::sync::{Arc, Mutex};

fn capture(enabled: bool) -> (Observation, Arc<Mutex<Vec<Value>>>) {
    let records = Arc::new(Mutex::new(Vec::new()));
    let output = records.clone();
    let observation = Observation::with_sink(
        &json!({"query":"ASK { <urn:private> ?p ?o }"}),
        Some(91),
        enabled,
        Box::new(move |record| output.lock().unwrap().push(record)),
    );
    (observation, records)
}

#[test]
fn admission_logging_is_opt_in_in_config_and_record_shape() {
    assert!(!quipu::config::ServerConfig::default().log_query_admission);
    let omitted: quipu::config::ServerConfig = toml::from_str("").unwrap();
    assert!(!omitted.log_query_admission);
    let enabled: quipu::config::ServerConfig =
        toml::from_str("log_query_admission = true").unwrap();
    assert!(enabled.log_query_admission);
    let (mut observation, records) = capture(false);
    assert!(observation.admission_stamp().is_none());
    observation.dispatched();
    drop(observation);
    let records = records.lock().unwrap();
    assert_eq!(records.len(), 2);
    assert!(
        records
            .iter()
            .all(|record| record.get("admission_ms").is_none())
    );
}

#[test]
fn enabled_logging_preserves_missing_and_granted_admission() {
    let (mut observation, records) = capture(true);
    let stamp = observation.admission_stamp().unwrap();
    stamp.set(Instant::now()).unwrap();
    assert!(
        stamp.set(Instant::now()).is_err(),
        "stamp is single assignment"
    );
    observation.dispatched();
    drop(observation);
    let records = records.lock().unwrap();
    assert_eq!(records.len(), 2);
    assert!(records[0]["admission_ms"].is_null());
    let admission = records[1]["admission_ms"].as_f64().unwrap();
    assert!(admission >= 0.0);
    assert!(admission <= records[1]["dispatch_ms"].as_f64().unwrap());
    assert!(!records[1].to_string().contains("urn:private"));

    let (observation, records) = capture(true);
    drop(observation);
    let records = records.lock().unwrap();
    assert!(records[1]["admission_ms"].is_null());
    assert!(records[1]["dispatch_ms"].is_null());
    assert_eq!(records[1]["worker_outcome"], "not_started");
}
