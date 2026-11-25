//! Production startup for the default-OFF ingest pilot (aegis-4qv).
//!
//! Absent or `enabled = false`: returns the router untouched, reading no file
//! and resolving no credential, so startup is byte-identical. Enabled: the
//! catalog is loaded and validated, then `PreparedDataset::initialize` verifies
//! the legacy fence, resolves credentials, runs S1 against the live witness and
//! admits the owner. ANY refusal exits the process. There is no fallback: a
//! server asked to run the pilot never serves without it.
use super::dataset::{PreparedDataset, Startup};
use crate::SharedStore;
use quipu::{
    QuipuConfig,
    graph_catalog::Catalog,
    store::{
        graph_owner::LegacyWriterFence,
        graph_router::{EnrolledPilotConfig, IngestPilot},
    },
};
use std::{sync::Arc, time::Duration};

fn fail(why: impl std::fmt::Display) -> ! {
    eprintln!("error: ingest pilot startup refused: {why}");
    eprintln!(
        "refusing to serve without the explicitly enabled pilot; \
         set [quipu.ingest_pilot] enabled = false to start without it"
    );
    std::process::exit(1)
}

/// The owner's served writer policy: the same identity, search, label, import,
/// SHACL and governance policy the legacy writer was configured with above.
fn configure(store: &mut quipu::Store, config: &QuipuConfig) {
    crate::base::apply_identity_policies(store, config);
    crate::base::apply_search(store, &config.search);
    store.labels_config_mut().clone_from(&config.label_floors);
    store
        .import_policy_config_mut()
        .clone_from(&config.import_policy);
    store.shacl_config_mut().clone_from(&config.shacl);
    crate::base::apply_governance(store, config);
}

/// Merge the admitted owner's `/owner/knot*` routes. The caller must pass the
/// router BEFORE `protected_app`, so these routes keep the ordinary auth layer.
pub(crate) fn mount(
    app: axum::Router<SharedStore>,
    config: &QuipuConfig,
    legacy: &SharedStore,
) -> axum::Router<SharedStore> {
    let Some(pilot) = config.ingest_pilot.as_ref().filter(|p| p.enabled) else {
        return app;
    };
    // The catalog is validated before any credential or witness is touched.
    let catalog = Arc::new(Catalog::load_file(&pilot.catalog_file).unwrap_or_else(|e| fail(e)));
    let readers = catalog.budget().readers_per_owner as u64;
    // Legacy participant and owner each hold a writer, a fence and readers.
    let allocation = catalog
        .memory_allocation(pilot.memory_mib, readers + 2, 0)
        .unwrap_or_else(|e| fail(e));
    let enrolled = EnrolledPilotConfig {
        root: pilot.root.clone(),
        inventory: pilot.inventory.clone(),
        profile: pilot.profile.clone(),
        endpoint: pilot.endpoint.clone(),
        bucket: pilot.bucket.clone(),
        region: pilot.region.clone(),
        prefix: pilot.prefix.clone(),
        timeout: Duration::from_secs(pilot.timeout_secs),
        witness_ca_der: pilot.read_witness_ca().unwrap_or_else(|e| fail(e)),
        installed_policy: pilot.installed_policy.clone(),
        allocation,
    };
    let admission = IngestPilot {
        enabled: true,
        graph: Some(pilot.graph.clone()),
    };
    let revision = catalog.revision();
    let fence: Arc<dyn LegacyWriterFence> = legacy.clone();
    let startup = PreparedDataset::initialize(
        &admission,
        catalog,
        fence,
        &enrolled,
        || pilot.read_credentials(),
        |store| {
            configure(store, config);
            Ok(())
        },
    )
    .unwrap_or_else(|e| fail(e.message()));
    let Startup::Enabled(dataset) = startup else {
        fail("the enabled pilot was not admitted")
    };
    // Process-lifetime: the borrowed legacy participant and the owner must
    // outlive every request, exactly like the served StoreHandle.
    let dataset: &'static PreparedDataset = Box::leak(dataset);
    eprintln!(
        "ingest pilot ENABLED: graph {} at /owner/knot (catalog revision {revision})",
        pilot.graph
    );
    app.merge(dataset.routes().with_state(()))
}
