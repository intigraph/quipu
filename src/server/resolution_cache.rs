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

/// `quipu_resolution_label_index_total{outcome}`, `..._warm_total{outcome}` and
/// `..._cleared_total{reason}` for the metrics page (aegis-wjo7): which path
/// each `/resolve` label lookup took, what each off-request warm did, and why
/// the commit hook left the shared cache empty.
pub(crate) fn render(out: &mut String) {
    use quipu::resolution::cache_counters as c;
    use std::fmt::Write as _;
    for (name, help, label, counts) in [
        (
            "quipu_resolution_label_index_total",
            "Resolution label lookups by where they were answered from (aegis-wjo7).",
            "outcome",
            c::lookup_counts(),
        ),
        (
            "quipu_resolution_label_warm_total",
            "Off-request label warms by outcome (aegis-wjo7).",
            "outcome",
            c::warm_counts(),
        ),
        (
            "quipu_resolution_label_cache_cleared_total",
            "Commits after which the shared label cache was left empty, by reason (aegis-wjo7).",
            "reason",
            c::cleared_counts(),
        ),
    ] {
        let _ = writeln!(out, "# HELP {name} {help}");
        let _ = writeln!(out, "# TYPE {name} counter");
        for (value, n) in counts {
            let _ = writeln!(out, "{name}{{{label}=\"{value}\"}} {n}");
        }
    }
}
