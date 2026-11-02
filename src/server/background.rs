//! Writer jobs for the primary. Replica startup never calls this module.
use super::{SharedStore, graph_metrics, wal_maintenance};
pub(crate) fn start(
    push_store_outer: SharedStore,
    retention_days: Option<u32>,
    vectors: &quipu::EmbeddingConfig,
) {
    // Event push P2: the delivery worker. A 2s tick over deliver_tick with the
    // real poster; each tick runs under spawn_blocking (SQLite + ureq are
    // sync). Cursor semantics make every tick idempotent, so the loop needs no
    // state of its own and a missed tick delays, never loses.
    {
        let push_store = push_store_outer.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let store = push_store.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let store = store.lock();
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
                    let mut post = |url: &str, body: &serde_json::Value| -> bool {
                        matches!(
                            ureq::post(url)
                                .set("Content-Type", "application/json")
                                .timeout(std::time::Duration::from_secs(10))
                                .send_string(&body.to_string()),
                            Ok(r) if (200..300).contains(&r.status())
                        )
                    };
                    match quipu::store::push::deliver_tick(&store, now, &mut post) {
                        Ok(outcomes) => {
                            for (sub, o) in outcomes {
                                if let quipu::store::push::Delivery::Failed = o {
                                    eprintln!(
                                        "push: delivery to '{sub}' failed; will retry (cursor held)"
                                    );
                                }
                            }
                        }
                        Err(e) => eprintln!("push: tick error: {e}"),
                    }
                })
                .await;
            }
        });
    }

    wal_maintenance::spawn_periodic_checkpoint(push_store_outer.clone());
    graph_metrics::spawn_refresh(&push_store_outer);

    // Event-log retention (quipu-9z9). Opt-in via `[quipu.events]
    // retention_days`; unset keeps today's keep-forever behaviour and spawns
    // nothing. An hourly cadence (first tick immediate) is plenty: the policy
    // is measured in days, and `prune_events` never touches an offset a
    // registered consumer has not committed past, so an aggressive cadence
    // could not break replay anyway — it would only burn cycles.
    spawn_vector_retention(&push_store_outer, vectors);

    if let Some(days) = retention_days {
        let prune_store = push_store_outer;
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(3600));
            loop {
                tick.tick().await;
                let store = prune_store.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let cutoff = quipu::time::iso_days_ago(u64::from(days));
                    let store = store.lock();
                    match store.prune_events(&cutoff) {
                        Ok(0) => {}
                        Ok(n) => eprintln!(
                            "events: retention pruned {n} event(s) older than {cutoff} ({days}d)"
                        ),
                        Err(e) => eprintln!("events: retention prune failed: {e}"),
                    }
                })
                .await;
            }
        });
    }
}

/// Rows deleted under one lock hold. Small enough that a writer waiting on
/// the store lock is held up for one short delete, not the whole pass.
const VECTOR_PRUNE_BATCH: usize = 2_000;

/// Superseded-vector retention (aegis-jio5.3). On by default
/// (`[quipu.embedding] superseded_retention_days`, 30); `0` spawns nothing.
/// Hourly, first tick immediate. Each pass deletes at most
/// `superseded_prune_max_per_pass` rows in batches, releasing the store lock
/// between batches, so a store upgraded with a large backlog drains it over
/// several hours rather than in one long delete under the writer lock.
fn spawn_vector_retention(store: &SharedStore, cfg: &quipu::EmbeddingConfig) {
    let days = cfg.superseded_retention_days;
    let max_per_pass = cfg.superseded_prune_max_per_pass;
    if days == 0 || max_per_pass == 0 {
        return;
    }
    let prune_store = store.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            tick.tick().await;
            let mut pass = 0usize;
            while pass < max_per_pass {
                let store = prune_store.clone();
                let batch = VECTOR_PRUNE_BATCH.min(max_per_pass - pass);
                let res = tokio::task::spawn_blocking(move || {
                    let now = quipu::time::now_iso();
                    store.lock().prune_superseded_vectors(days, &now, batch)
                })
                .await;
                match res {
                    Ok(Ok(n)) => {
                        pass += n;
                        if n < batch {
                            break;
                        }
                    }
                    Ok(Err(e)) => {
                        eprintln!("vectors: retention prune failed: {e}");
                        break;
                    }
                    Err(e) => {
                        eprintln!("vectors: retention prune task failed: {e}");
                        break;
                    }
                }
                tokio::task::yield_now().await;
            }
            if pass > 0 {
                eprintln!(
                    "vectors: retention pruned {pass} superseded vector(s) older than {days}d"
                );
            }
        }
    });
}
