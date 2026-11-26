//! `[quipu.graph_sets]` startup (aegis-n0wc.5).
//!
//! Absent or `enabled = false`: returns at once, reading no file, so startup is
//! byte-identical. Enabled in a Unix `ownership-witness-s3` build: the catalog
//! is preflighted (including claims shared with `[quipu.ingest_pilot]`), then
//! `GraphSetsConfig::admit_enrolled` verifies the legacy fence and opens every
//! routed owner through S1. The admitted router lives for the process; this
//! slice mounts no HTTP route. Any refusal exits with one line. Other builds
//! exit with `STARTUP_REFUSAL`. There is never a plain-open fallback.
use crate::SharedStore;

fn fail(why: impl std::fmt::Display) -> ! {
    eprintln!("error: graph_sets startup refused: {why}");
    std::process::exit(1)
}

/// Run before any route is mounted. Returns only when the table is inert or
/// every routed owner was admitted.
pub(crate) fn start(config: &quipu::QuipuConfig, legacy: &SharedStore) {
    let Some(sets) = config.graph_sets.as_ref().filter(|s| s.enabled) else {
        return;
    };
    #[cfg(all(unix, feature = "ownership-witness-s3"))]
    {
        let catalog = sets
            .preflight(config.ingest_pilot.as_ref())
            .unwrap_or_else(|e| fail(e));
        let revision = catalog.physical().revision();
        let owners = catalog.physical().routes().count();
        let writer = legacy.writer.lock();
        let router = sets
            .admit_enrolled(std::sync::Arc::new(catalog), &writer, |store| {
                configure(store, config);
                Ok(())
            })
            .unwrap_or_else(|e| fail(e));
        drop(writer);
        // Process-lifetime, like the served StoreHandle: the owners' leases
        // must outlive every request.
        let _router: &'static _ = Box::leak(Box::new(router));
        eprintln!(
            "graph sets ENABLED: {owners} enrolled owner(s) admitted at catalog revision {revision}; no HTTP route"
        );
    }
    #[cfg(not(all(unix, feature = "ownership-witness-s3")))]
    {
        let _ = legacy;
        if let Some(reason) = sets.startup_refusal(config.ingest_pilot.as_ref()) {
            fail(reason);
        }
    }
}

/// The owners' served writer policy: the same identity, search, label, import,
/// SHACL and governance policy the legacy writer was configured with.
#[cfg(all(unix, feature = "ownership-witness-s3"))]
fn configure(store: &mut quipu::Store, config: &quipu::QuipuConfig) {
    crate::base::apply_identity_policies(store, config);
    crate::base::apply_search(store, &config.search);
    store.labels_config_mut().clone_from(&config.label_floors);
    store
        .import_policy_config_mut()
        .clone_from(&config.import_policy);
    store.shacl_config_mut().clone_from(&config.shacl);
    crate::base::apply_governance(store, config);
}
