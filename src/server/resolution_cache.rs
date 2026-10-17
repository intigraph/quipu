//! Coalesced off-request label rebuilds after unknown committed changes.
use super::SharedStore;
pub(crate) fn start(state: &SharedStore) {
    let mut wake = state.commit_wake.subscribe();
    let weak = std::sync::Arc::downgrade(state);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                changed = wake.changed() => if changed.is_err() { break; },
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {},
            }
            let Some(state) = weak.upgrade() else {
                break;
            };
            // One worker, one scan at a time; watch coalesces bursts. A write
            // during the scan prevents publication; its hint triggers another.
            let result =
                tokio::task::spawn_blocking(move || state.read().prepare_resolution_index()).await;
            match result {
                Ok(Ok(_)) => {}
                error => eprintln!("resolution label warm failed: {error:?}"),
            }
        }
    });
}
