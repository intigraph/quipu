//! `[quipu.graph_sets]` startup (aegis-n0wc.5).
//!
//! Absent or `enabled = false`: returns at once, reading no file, so startup is
//! byte-identical. Enabled: the graph-set catalog is loaded and validated and
//! any claim shared with an enabled `[quipu.ingest_pilot]` refuses; then the
//! process exits. Graph-set owners open only through enrolled ownership
//! (`GraphSetOwner::open_enrolled`, native), and server startup does not call
//! it yet. There is no plain-open fallback: a server asked to run graph-set
//! owners never serves without them.

fn fail(why: impl std::fmt::Display) -> ! {
    eprintln!("error: graph-set startup refused: {why}");
    eprintln!(
        "refusing to serve without the explicitly enabled graph sets; \
         set [quipu.graph_sets] enabled = false to start without them"
    );
    std::process::exit(1)
}

/// Run before any route is mounted. Returns only when the table is inert.
pub(crate) fn check(config: &quipu::QuipuConfig) {
    let Some(sets) = config.graph_sets.as_ref().filter(|s| s.enabled) else {
        return;
    };
    if let Err(e) = sets.preflight(config.ingest_pilot.as_ref()) {
        fail(e);
    }
    fail("server startup does not yet open enrolled graph-set owners (no plain-open fallback)")
}
