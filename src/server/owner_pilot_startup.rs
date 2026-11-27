//! Production startup for the default-OFF ingest pilot (aegis-4qv).
//!
//! Absent or `enabled = false`: returns the router untouched, reading no file
//! and resolving no credential, so startup is byte-identical. Enabled: the
//! catalog is loaded and validated, then `PreparedDataset::initialize` verifies
//! the legacy fence, resolves credentials, runs S1 against the live witness and
//! admits the owner. ANY refusal exits the process. There is no fallback: a
//! server asked to run the pilot never serves without it.
//!
//! With `[quipu.ingest_pilot.reads]` the same admitted dataset also serves
//! authenticated composed reads at `POST /owner/query` (aegis-4qv.8), sized
//! from the same memory budget. Without it, only `/owner/knot*` is mounted.
use super::dataset::{PreparedDataset, ReadConfig, Startup};
use crate::SharedStore;
use quipu::{
    QuipuConfig,
    config::PilotReadsConfig,
    graph_catalog::Catalog,
    read_authorization::{
        ComposedReadAdmission, ComposedReadReservation, profile::ScopedReadConfig,
    },
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

/// Composed-read admission over the pilot's one memory budget. The legacy
/// participant and the owner each hold a writer, a fence and their readers.
pub(crate) fn read_admission(
    catalog: &Arc<Catalog>,
    memory_mib: u64,
    reads: &PilotReadsConfig,
) -> quipu::Result<ComposedReadAdmission> {
    let bytes = |mib: u64| {
        mib.checked_mul(1 << 20).ok_or_else(|| {
            quipu::Error::InvalidValue("ingest pilot reads: reservation overflows".into())
        })
    };
    let readers = catalog.budget().readers_per_owner as u64;
    ComposedReadAdmission::new(
        catalog.clone(),
        memory_mib,
        readers + 2,
        reads.concurrency,
        ComposedReadReservation {
            runtime_bytes: bytes(reads.runtime_mib)?,
            view_bytes: bytes(reads.view_mib)?,
            query_bytes: bytes(reads.query_mib)?,
            max_response_bytes: usize::try_from(bytes(reads.max_response_mib)?).map_err(|_| {
                quipu::Error::InvalidValue("ingest pilot reads: response bound too large".into())
            })?,
        },
    )
}

/// The explicit, owner-selected read authority; nothing comes from a request.
pub(crate) fn read_config(reads: &PilotReadsConfig) -> ReadConfig {
    ReadConfig {
        policy: ScopedReadConfig {
            policy_path: reads.policy_file.to_string_lossy().into_owned(),
            owner: reads.owner.clone(),
        },
        registry: reads.registry_file.to_string_lossy().into_owned(),
        timeout: Duration::from_secs(reads.timeout_secs),
    }
}

/// The served pilot router: `/owner/knot*`, plus `/owner/query` when reads
/// are configured. A refusal here is a startup refusal, never a fallback.
pub(crate) fn serve(
    dataset: &Arc<PreparedDataset>,
    reads: Option<(&PilotReadsConfig, ComposedReadAdmission)>,
) -> Result<axum::Router, crate::base::AppError> {
    match reads {
        None => Ok(dataset.routes()),
        Some((reads, admission)) => dataset.routes_with_reads(read_config(reads), admission),
    }
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
    let reads = pilot.reads.as_ref().map(|reads| {
        let admission =
            read_admission(&catalog, pilot.memory_mib, reads).unwrap_or_else(|e| fail(e));
        (reads, admission)
    });
    // Legacy participant and owner each hold a writer, a fence and readers;
    // with reads, the same budget also reserves the composed-read scratch.
    let allocation = match &reads {
        Some((_, admission)) => admission.allocation(),
        None => catalog
            .memory_allocation(pilot.memory_mib, readers + 2, 0)
            .unwrap_or_else(|e| fail(e)),
    };
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
    let dataset: &'static Arc<PreparedDataset> = Box::leak(Box::new(Arc::new(*dataset)));
    let routes = serve(dataset, reads).unwrap_or_else(|e| fail(e.message()));
    eprintln!(
        "ingest pilot ENABLED: graph {} at /owner/knot{} (catalog revision {revision})",
        pilot.graph,
        if pilot.reads.is_some() {
            " and /owner/query"
        } else {
            ""
        },
    );
    app.merge(routes.with_state(()))
}
