//! A follower never opens a writable connection, migrates, signs or runs writer jobs.
use std::io::Read;
use std::time::{Duration, Instant};

use axum::{extract::State, http::StatusCode, response::IntoResponse};
use parking_lot::Mutex;
use quipu::config::{QuipuConfig, ReplicaConfig, VectorBackend};
use serde::{Deserialize, Serialize};

use super::SharedStore;

pub(crate) fn open_store(path: &str, config: &QuipuConfig) -> quipu::Result<quipu::Store> {
    if let Some(replica) = &config.server.replica {
        let url = url::Url::parse(&replica.primary_url).map_err(|_| {
            quipu::Error::InvalidValue("replica primary_url must be an HTTP URL".into())
        })?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(quipu::Error::InvalidValue(
                "replica primary_url must have no credentials, query or fragment".into(),
            ));
        }
        if !config.attachments.is_empty() || !matches!(config.vector.backend, VectorBackend::Sqlite)
        {
            return Err(quipu::Error::InvalidValue("replica currently requires the SQLite backend and no attachments; refusing to drop configured providers".into()));
        }
        let mut store = quipu::Store::open_read_only(path)?;
        quipu::entity_namespaces::configure(&mut store, config)?;
        store.set_busy_timeout(Duration::from_secs(1))?;
        Ok(store)
    } else {
        quipu::open_with_configured_attachments(path, config)
    }
}

pub(crate) fn apply_search(
    store: &mut quipu::Store,
    config: &quipu::SearchConfig,
) -> quipu::Result<()> {
    store.search_config_mut().clone_from(config);
    if config.keyword && store.lexical_progress()?.is_none() {
        return Err(quipu::Error::InvalidValue(
            "replica keyword index is absent; initialize it on the primary".into(),
        ));
    }
    Ok(())
}

pub(crate) fn write_refusal(config: &ReplicaConfig, path: &str) -> axum::response::Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        axum::Json(serde_json::json!({
            "error": "read-only replica: send writes to the primary",
            "reason": "read_only_replica", "primary": config.primary_url, "endpoint": path,
        })),
    )
        .into_response()
}

#[derive(Debug, Deserialize, Serialize)]
struct Watermark {
    store_id: String,
    watermark_tx: i64,
}

pub(crate) async fn watermark(
    State(store): State<SharedStore>,
) -> Result<axum::Json<serde_json::Value>, super::base::AppError> {
    super::base::blocking(move || {
        let store = store.read();
        Ok(axum::Json(
            serde_json::to_value(Watermark {
                store_id: store.store_id()?,
                watermark_tx: store.latest_tx_id()?,
            })
            .expect("serializing watermark"),
        ))
    })
    .await
}

/// What the follower holds and when it was last confirmed current.
#[derive(Default)]
pub(crate) struct ReplicaStatus {
    observation: Mutex<Option<(Instant, i64)>>,
    /// The last successful lag observation: when a primary last confirmed it.
    confirmed: Mutex<Option<Instant>>,
    /// The follower's own position, read every tick even with no primary.
    position: Mutex<Option<quipu::follow::Position>>,
}
impl ReplicaStatus {
    fn lag(&self) -> Option<i64> {
        self.observation
            .lock()
            .as_ref()
            .filter(|(at, _)| at.elapsed() <= Duration::from_secs(30))
            .map(|(_, lag)| *lag)
    }
    fn confirmed_age_secs(&self) -> Option<u64> {
        self.confirmed.lock().map(|at| at.elapsed().as_secs())
    }
    fn record(&self, position: Option<quipu::follow::Position>, lag: Option<i64>) {
        if lag.is_some() {
            *self.confirmed.lock() = Some(Instant::now());
        }
        *self.observation.lock() = lag.map(|lag| (Instant::now(), lag));
        if position.is_some() {
            *self.position.lock() = position;
        }
    }
    pub(crate) fn metrics(&self) -> String {
        let lag = self.lag();
        let source_tx = self.position.lock().as_ref().map_or(-1, |p| p.source_tx);
        let age = self
            .confirmed_age_secs()
            .map_or(-1, |s| i64::try_from(s).unwrap_or(i64::MAX));
        format!(
            "# HELP quipu_replica_lag_transactions Primary minus follower transaction IDs; -1 means unknown.\n# TYPE quipu_replica_lag_transactions gauge\nquipu_replica_lag_transactions {}\n# HELP quipu_replica_lag_known Whether a fresh primary/follower observation exists.\n# TYPE quipu_replica_lag_known gauge\nquipu_replica_lag_known {}\n# HELP quipu_replica_source_tx The last source transaction the follower holds; -1 means unknown.\n# TYPE quipu_replica_source_tx gauge\nquipu_replica_source_tx {source_tx}\n# HELP quipu_replica_confirmed_age_seconds Seconds since a primary last confirmed the lag; -1 means never since start.\n# TYPE quipu_replica_confirmed_age_seconds gauge\nquipu_replica_confirmed_age_seconds {age}\n",
            lag.unwrap_or(-1),
            u8::from(lag.is_some())
        )
    }
    /// The staleness headers every replica response carries. A value nobody
    /// can establish is `unknown`, never zero.
    fn header_values(&self) -> [(&'static str, String); 4] {
        let unknown = || "unknown".to_string();
        let position = self.position.lock().clone();
        [
            (
                "x-quipu-replica-lag-tx",
                self.lag().map_or_else(unknown, |n| n.to_string()),
            ),
            (
                "x-quipu-replica-source-tx",
                position
                    .as_ref()
                    .map_or_else(unknown, |p| p.source_tx.to_string()),
            ),
            (
                "x-quipu-replica-source-time",
                position.and_then(|p| p.source_time).unwrap_or_else(unknown),
            ),
            (
                "x-quipu-replica-confirmed-age-s",
                self.confirmed_age_secs()
                    .map_or_else(unknown, |s| s.to_string()),
            ),
        ]
    }
}

pub(crate) async fn headers(
    State(store): State<SharedStore>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(req).await;
    if let Some(replica) = &store.replica {
        for (name, value) in replica.header_values() {
            if let Ok(value) = value.parse() {
                response.headers_mut().insert(name, value);
            }
        }
    }
    if let Some(relay) = &store.relay {
        // The relay's own freshness from its upstream: a proven bound, never
        // a guess; UNKNOWN until a fresh upstream proof exists.
        let bound = relay
            .freshness()
            .staleness_bound_ms
            .map_or_else(|| "unknown".to_string(), |ms| ms.to_string());
        response.headers_mut().insert(
            "x-quipu-relay-staleness-ms",
            bound.parse().expect("numeric bound"),
        );
    }
    response
}

/// The follower's position, read on its own connection.
fn local_position(store: &SharedStore) -> Option<quipu::follow::Position> {
    let local = store.writer.try_lock()?;
    quipu::follow::position(&local).ok()
}

fn remote_watermark(primary: &str) -> Option<Watermark> {
    // Never hold a local SQLite connection across the network request.
    let response = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(2))
        .redirects(0)
        .build()
        .get(&format!("{}/watermark", primary.trim_end_matches('/')))
        .set("X-Quipu-Client", "replica-metadata")
        .call()
        .ok()?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(65_537)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 65_536 {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

/// Primary minus follower, against the lineage the follower actually holds:
/// the source's id in an applier-built follower, its own id in a file copy.
fn lag_of(remote: &Watermark, local: &quipu::follow::Position) -> Option<i64> {
    // A lineage mismatch or a primary behind the follower is UNKNOWN, never zero.
    if remote.store_id != local.lineage
        || remote.watermark_tx < local.source_tx
        || local.source_tx < 0
    {
        return None;
    }
    Some(remote.watermark_tx - local.source_tx)
}

fn observe(store: &SharedStore, primary: &str) -> (Option<quipu::follow::Position>, Option<i64>) {
    let remote = remote_watermark(primary);
    let local = local_position(store);
    let lag = remote
        .as_ref()
        .zip(local.as_ref())
        .and_then(|(r, l)| lag_of(r, l));
    (local, lag)
}

pub(crate) fn start_metadata_reader(store: SharedStore, config: ReplicaConfig) {
    tokio::spawn(async move {
        loop {
            let s = store.clone();
            let primary = config.primary_url.clone();
            let (position, lag) = tokio::task::spawn_blocking(move || observe(&s, &primary))
                .await
                .unwrap_or_default();
            if let Some(status) = &store.replica {
                status.record(position, lag);
            }
            tokio::time::sleep(Duration::from_secs(15)).await;
        }
    });
}

#[cfg(test)]
#[path = "replica_tests.rs"]
mod tests;
