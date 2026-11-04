//! Writer jobs for the primary. Replica startup never calls this module.
use super::{SharedStore, graph_metrics, wal_maintenance};
pub(crate) fn start(push_store_outer: SharedStore, retention_days: u32) {
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

    // Event-log retention (quipu-9z9, aegis-jio5.5). `[quipu.events]
    // retention_days`, default 30; `0` keeps every event and spawns nothing.
    if retention_days > 0 {
        spawn_event_retention(push_store_outer, retention_days);
    }
}

/// Rows deleted per batch; the store lock is released between batches.
const EVENT_PRUNE_BATCH: usize = 2_000;
/// Rows deleted per hourly pass at most, so a large backlog drains over
/// several passes instead of one long run of writer-held batches.
const EVENT_PRUNE_MAX_PER_PASS: usize = 50_000;

/// Hourly (first tick immediate) bounded prune. The policy is measured in
/// days, and `prune_events_batch` never touches an offset a registered
/// consumer has not committed past, so the cadence cannot break replay.
fn spawn_event_retention(store: SharedStore, days: u32) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            tick.tick().await;
            let store = store.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let cutoff = quipu::time::iso_days_ago(u64::from(days));
                let mut pruned = 0usize;
                while pruned < EVENT_PRUNE_MAX_PER_PASS {
                    let limit = EVENT_PRUNE_BATCH.min(EVENT_PRUNE_MAX_PER_PASS - pruned);
                    // The guard drops at the end of each statement, so other
                    // writers run between batches.
                    match store.lock().prune_events_batch(&cutoff, limit) {
                        Ok(n) => {
                            pruned += n;
                            if n < limit {
                                break;
                            }
                        }
                        Err(e) => {
                            eprintln!("events: retention prune failed: {e}");
                            break;
                        }
                    }
                }
                if pruned > 0 {
                    eprintln!(
                        "events: retention pruned {pruned} event(s) older than {cutoff} ({days}d)"
                    );
                }
            })
            .await;
        }
    });
}
