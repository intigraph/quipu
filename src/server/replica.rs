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

#[derive(Default)]
pub(crate) struct ReplicaStatus {
    observation: Mutex<Option<(Instant, i64)>>,
}
impl ReplicaStatus {
    fn lag(&self) -> Option<i64> {
        self.observation
            .lock()
            .as_ref()
            .filter(|(at, _)| at.elapsed() <= Duration::from_secs(30))
            .map(|(_, lag)| *lag)
    }
    pub(crate) fn metrics(&self) -> String {
        let lag = self.lag();
        format!(
            "# HELP quipu_replica_lag_transactions Primary minus follower transaction IDs; -1 means unknown.\n# TYPE quipu_replica_lag_transactions gauge\nquipu_replica_lag_transactions {}\n# HELP quipu_replica_lag_known Whether a fresh primary/follower observation exists.\n# TYPE quipu_replica_lag_known gauge\nquipu_replica_lag_known {}\n",
            lag.unwrap_or(-1),
            u8::from(lag.is_some())
        )
    }
}

pub(crate) async fn headers(
    State(store): State<SharedStore>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(req).await;
    if let Some(replica) = &store.replica {
        let lag = replica
            .lag()
            .map_or_else(|| "unknown".to_string(), |n| n.to_string());
        response
            .headers_mut()
            .insert("x-quipu-replica-lag-tx", lag.parse().expect("numeric lag"));
    }
    response
}

fn observe(store: &SharedStore, primary: &str) -> Option<i64> {
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
    let remote: Watermark = serde_json::from_slice(&bytes).ok()?;
    let local = store.writer.try_lock()?;
    let tx = local.latest_tx_id().ok()?;
    // A lineage mismatch or a primary behind the follower is UNKNOWN, never zero.
    if remote.store_id != local.store_id().ok()? || remote.watermark_tx < tx || tx < 0 {
        return None;
    }
    Some(remote.watermark_tx - tx)
}

pub(crate) fn start_metadata_reader(store: SharedStore, config: ReplicaConfig) {
    tokio::spawn(async move {
        loop {
            let s = store.clone();
            let primary = config.primary_url.clone();
            let result = tokio::task::spawn_blocking(move || observe(&s, &primary))
                .await
                .ok()
                .flatten();
            if let Some(status) = &store.replica {
                *status.observation.lock() = result.map(|lag| (Instant::now(), lag));
            }
            tokio::time::sleep(Duration::from_secs(15)).await;
        }
    });
}

#[cfg(test)]
#[path = "replica_tests.rs"]
mod tests;
