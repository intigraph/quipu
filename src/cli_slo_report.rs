//! Public operational statistics. This path never opens the knowledge store.
#[cfg(any(feature = "remote", test))]
use serde_json::{Value, json};
#[cfg(any(feature = "remote", test))]
use std::collections::BTreeMap;

pub fn run(args: &[String]) {
    if let Err(error) = execute(args) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(feature = "remote"))]
fn execute(_: &[String]) -> Result<(), String> {
    Err("public SLO reports require the remote feature".into())
}

#[cfg(feature = "remote")]
fn execute(args: &[String]) -> Result<(), String> {
    use base64::Engine;
    use std::io::Read;
    let option = |name: &str| args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone());
    if args.iter().any(|a| {
        matches!(
            a.as_str(),
            "--db" | "--type" | "--predicate" | "--hubs" | "--surprises" | "--questions"
        )
    }) {
        return Err("public SLO reports cannot be combined with graph report options".into());
    }
    let url = option("--prometheus")
        .or_else(|| std::env::var("QUIPU_REPORT_PROMETHEUS_URL").ok())
        .ok_or("set --prometheus or QUIPU_REPORT_PROMETHEUS_URL")?;
    // Never accept credentials in a URL, redirect them, or echo private URLs in errors.
    if !(url.starts_with("https://") || url.starts_with("http://")) || url.contains(['@', '?', '#'])
    {
        return Err(
            "Prometheus URL requires http(s), without credentials, query or fragment".into(),
        );
    }
    let days: u64 = option("--days")
        .unwrap_or_else(|| "30".into())
        .parse()
        .map_err(|_| "invalid days")?;
    if !(1..=90).contains(&days) {
        return Err("days must be between 1 and 90".into());
    }
    let end = option("--end")
        .map(|v| quipu::time::epoch_of_rfc3339(&v).ok_or("invalid UTC end timestamp"))
        .transpose()?
        .unwrap_or((quipu::time::epoch_secs() / 86400 * 86400) as i64);
    let unsigned_end = u64::try_from(end).map_err(|_| "end precedes the Unix epoch")?;
    if end < (days * 86400) as i64 || end % 86400 != 0 || unsigned_end > quipu::time::epoch_secs() {
        return Err("end must be a past UTC midnight with enough preceding days".into());
    }
    let start = end - ((days - 1) * 86400) as i64;
    let read_secret = |name: &str| -> Result<Option<String>, String> {
        let Some(path) = std::env::var_os(name) else {
            return Ok(None);
        };
        let file =
            std::fs::File::open(path).map_err(|_| "cannot read Prometheus credential file")?;
        if !file
            .metadata()
            .map_err(|_| "cannot inspect credential file")?
            .is_file()
        {
            return Err("credential file must be regular".into());
        }
        let mut secret = String::new();
        file.take(16385)
            .read_to_string(&mut secret)
            .map_err(|_| "cannot read credential file")?;
        if secret.len() > 16384 || secret.trim().is_empty() {
            return Err("invalid credential file".into());
        }
        Ok(Some(secret.trim().into()))
    };
    let bearer = read_secret("PROMETHEUS_BEARER_TOKEN_FILE")?;
    let password = read_secret("PROMETHEUS_PASSWORD_FILE")?;
    if bearer.is_some() && password.is_some() {
        return Err("choose one Prometheus credential method".into());
    }
    let auth = if let Some(token) = bearer {
        Some(format!("Bearer {token}"))
    } else if let Some(password) = password {
        let user = std::env::var("PROMETHEUS_USERNAME")
            .map_err(|_| "PROMETHEUS_USERNAME is required with a password file")?;
        if user.contains(':') {
            return Err("invalid Prometheus username".into());
        }
        Some(format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"))
        ))
    } else {
        None
    };
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(std::time::Duration::from_secs(30))
        .build();
    let mut inputs = BTreeMap::new();
    for (name, query) in queries() {
        let endpoint = format!("{}/api/v1/query_range", url.trim_end_matches('/'));
        let mut request = agent
            .get(&endpoint)
            .query("query", &query)
            .query("start", &start.to_string())
            .query("end", &end.to_string())
            .query("step", "86400")
            .query("timeout", "25s");
        if let Some(auth) = &auth {
            request = request.set("Authorization", auth);
        }
        let response = request
            .call()
            .map_err(|_| "Prometheus request failed; no public report emitted")?;
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Prometheus response read failed")?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err("Prometheus response exceeds report cap".into());
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| "invalid Prometheus JSON")?;
        inputs.insert(name, value);
    }
    let report = render(&inputs, start, end)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|_| "report serialization failed")?
    );
    Ok(())
}

#[cfg(any(feature = "remote", test))]
fn queries() -> Vec<(&'static str, String)> {
    let mut queries = vec![
        ("scrape_up", "avg(avg_over_time(up{job=\"quipu\"}[1d]))".into()),
        ("scrape_samples", "sum(count_over_time(up{job=\"quipu\"}[1d]))".into()),
        ("buckets", "sum by (endpoint, le) (increase(quipu_http_request_duration_seconds_bucket{job=\"quipu\"}[1d]))".into()),
        ("clients", "sum by (client) (increase(quipu_http_client_requests_total{job=\"quipu\",endpoint!=\"/metrics\"}[1d]))".into()),
    ];
    for (name, job, function) in [
        ("query_up", "blackbox-quipu-store", "avg(avg_over_time"),
        (
            "query_samples",
            "blackbox-quipu-store",
            "sum(count_over_time",
        ),
        ("search_up", "blackbox-quipu-search", "avg(avg_over_time"),
        (
            "search_samples",
            "blackbox-quipu-search",
            "sum(count_over_time",
        ),
    ] {
        queries.push((
            name,
            format!("{function}(probe_success{{job=\"{job}\"}}[1d]))"),
        ));
    }
    queries
}

#[cfg(any(feature = "remote", test))]
fn opaque(prefix: &str) -> Result<String, String> {
    use ring::rand::SecureRandom;
    let mut bytes = [0; 16];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "public label randomness unavailable")?;
    Ok(format!(
        "{prefix}-{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ))
}

#[cfg(any(feature = "remote", test))]
fn render(inputs: &BTreeMap<&str, Value>, start: i64, end: i64) -> Result<Value, String> {
    let mut rows = BTreeMap::new();
    if start < 86400 || end < start || end - start > 89 * 86400 {
        return Err("invalid report date range".into());
    }
    for timestamp in (start..=end).step_by(86400) {
        let window_end = u64::try_from(timestamp).map_err(|_| "invalid report date")?;
        let window_start = window_end.checked_sub(86400).ok_or("invalid report date")?;
        rows.insert(timestamp, json!({"date": &quipu::time::format_iso(window_start)[..10],
            "window_end":quipu::time::format_iso(window_end),
            "scrape_up":null,"scrape_samples":null,"query_up":null,"query_samples":null,
            "search_up":null,"search_samples":null,"latency_seconds":{},"completed_requests_by_client":{}}));
    }
    let mut client_names = BTreeMap::new();
    let mut endpoint_names = BTreeMap::new();
    let mut buckets: BTreeMap<(i64, String), Vec<(f64, f64)>> = BTreeMap::new();
    for (name, _) in queries() {
        let response = inputs.get(name).ok_or("missing report input")?;
        if response["status"] != "success"
            || response["data"]["resultType"] != "matrix"
            || response
                .get("warnings")
                .and_then(Value::as_array)
                .is_some_and(|w| !w.is_empty())
        {
            return Err("incomplete or unsuccessful Prometheus result".into());
        }
        let results = response["data"]["result"]
            .as_array()
            .ok_or("invalid Prometheus matrix")?;
        if results.len() > 4096 {
            return Err("report series cap exceeded".into());
        }
        let mut seen = std::collections::BTreeSet::new();
        for series in results {
            let raw = if name == "clients" {
                series["metric"]["client"].as_str()
            } else if name == "buckets" {
                series["metric"]["endpoint"].as_str()
            } else {
                Some("")
            }
            .ok_or("missing report dimension")?;
            let label = if name == "clients" {
                if !client_names.contains_key(raw) {
                    client_names.insert(raw.to_string(), opaque("client")?);
                }
                client_names[raw].clone()
            } else if name == "buckets" {
                if matches!(
                    raw,
                    "/query" | "/search" | "/episode" | "/ask" | "/stats" | "/health" | "/metrics"
                ) {
                    raw.to_string()
                } else {
                    if !endpoint_names.contains_key(raw) {
                        endpoint_names.insert(raw.to_string(), opaque("endpoint")?);
                    }
                    endpoint_names[raw].clone()
                }
            } else {
                String::new()
            };
            let bound = if name == "buckets" {
                Some(
                    series["metric"]["le"]
                        .as_str()
                        .ok_or("missing histogram bound")?
                        .parse::<f64>()
                        .map_err(|_| "invalid histogram bound")?,
                )
            } else {
                None
            };
            let points = series["values"].as_array().ok_or("invalid matrix points")?;
            if points.len() > 90 {
                return Err("report point cap exceeded".into());
            }
            for point in points {
                let timestamp = point[0].as_f64().ok_or("invalid report timestamp")?;
                if !timestamp.is_finite() || timestamp.fract() != 0.0 {
                    return Err("invalid report timestamp".into());
                }
                let timestamp = timestamp as i64;
                let row = rows.get_mut(&timestamp).ok_or("unexpected report day")?;
                if !seen.insert((timestamp, raw.to_string(), bound.map(f64::to_bits))) {
                    return Err("duplicate report sample".into());
                }
                let number: f64 = point[1]
                    .as_str()
                    .ok_or("invalid report value")?
                    .parse()
                    .map_err(|_| "invalid report number")?;
                if number < 0.0 {
                    return Err("negative SLO observation".into());
                }
                if let Some(bound) = bound {
                    if bound.is_nan() || bound < 0.0 {
                        return Err("invalid histogram bound".into());
                    }
                    if number.is_finite() {
                        buckets
                            .entry((timestamp, label.clone()))
                            .or_default()
                            .push((bound, number));
                    }
                } else if name == "clients" {
                    row["completed_requests_by_client"][&label] =
                        json!(number.is_finite().then_some(number));
                } else {
                    row[name] = json!(number.is_finite().then_some(number));
                }
            }
        }
    }
    for ((timestamp, endpoint), mut values) in buckets {
        values.sort_by(|a, b| a.0.total_cmp(&b.0));
        rows.get_mut(&timestamp).unwrap()["latency_seconds"][endpoint] = json!({
            "p50": quantile(&values,0.5),"p95":quantile(&values,0.95),"p99":quantile(&values,0.99)});
    }
    Ok(json!({"schema":"quipu-public-slo/v1","public":true,
        "semantics":"Complete UTC days. Ratios cover observed samples only; missing samples are not uptime. Counts and histogram quantiles are reset-aware estimates; null means unknown or no events. Client and nonstandard endpoint labels are random per report; their maps are discarded. No graph data is read.",
        "scrape_expected_samples_per_day":1440,"probe_expected_samples_per_day":2880,
        "days":rows.into_values().collect::<Vec<_>>()}))
}

#[cfg(any(feature = "remote", test))]
fn quantile(buckets: &[(f64, f64)], q: f64) -> Option<f64> {
    let (last, count) = *buckets.last()?;
    if last != f64::INFINITY
        || count <= 0.0
        || buckets.len() < 2
        || buckets
            .windows(2)
            .any(|p| p[1].1 < p[0].1 || p[1].0 == p[0].0)
    {
        return None;
    }
    let rank = q * count;
    let mut lower = (0.0, 0.0);
    for &(upper, cumulative) in buckets {
        if cumulative >= rank {
            if upper == f64::INFINITY {
                return Some(lower.0);
            }
            let span = cumulative - lower.1;
            return (span > 0.0).then_some(lower.0 + (upper - lower.0) * (rank - lower.1) / span);
        }
        lower = (upper, cumulative);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn histogram_estimates_and_unknown_controls() {
        let buckets = [(0.1, 100.0), (f64::INFINITY, 100.0)];
        assert_eq!(quantile(&buckets, 0.95), Some(0.095));
        assert_eq!(quantile(&[(0.1, 0.0), (f64::INFINITY, 0.0)], 0.95), None);
        assert_eq!(quantile(&[(0.1, 100.0)], 0.95), None);
        assert_eq!(quantile(&[(0.1, 100.0), (f64::INFINITY, 90.0)], 0.95), None);
    }
    #[test]
    fn public_projection_drops_private_values_and_keeps_gaps() {
        let mut inputs = BTreeMap::new();
        for (name, _) in queries() {
            inputs.insert(
                name,
                json!({"status":"success","data":{"resultType":"matrix","result":[]}}),
            );
        }
        inputs.insert("clients",json!({"status":"success","data":{"resultType":"matrix","result":[{"metric":{"client":"private-host.internal","instance":"secret-host","task":"secret-task"},"values":[[172800,"10"]]}]}}));
        let report = render(&inputs, 172800, 259200).unwrap();
        let text = report.to_string();
        for private in ["private-host", "secret-host", "secret-task"] {
            assert!(!text.contains(private));
        }
        assert_eq!(report["days"].as_array().unwrap().len(), 2);
        assert!(report["days"][0]["scrape_up"].is_null());
        assert_ne!(report, render(&inputs, 172800, 259200).unwrap());
        inputs.get_mut("clients").unwrap()["warnings"] = json!(["partial"]);
        assert!(render(&inputs, 172800, 259200).is_err());
    }
}
