//! `[quipu.graph_sets]` startup (aegis-n0wc.5).
//!
//! Absent or `enabled = false`: returns at once, reading no file, so startup is
//! byte-identical. Enabled: `GraphSetsConfig::startup_refusal` validates the
//! catalog and any claim shared with an enabled `[quipu.ingest_pilot]`, and
//! the process exits with that one-line reason. Graph-set owners open only
//! through enrolled ownership (`GraphSetOwner::open_enrolled`, native), and
//! server startup does not call it yet. There is no plain-open fallback.

/// Run before any route is mounted. Returns only when the table is inert.
pub(crate) fn check(config: &quipu::QuipuConfig) {
    let Some(sets) = config.graph_sets.as_ref() else {
        return;
    };
    if let Some(reason) = sets.startup_refusal(config.ingest_pilot.as_ref()) {
        eprintln!("error: {reason}");
        std::process::exit(1);
    }
}
