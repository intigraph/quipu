//! `[quipu.graph_sets]` startup (aegis-n0wc.5).
//!
//! Absent or `enabled = false`: returns at once, reading no file, so startup is
//! byte-identical. Enabled in a Unix `ownership-witness-s3` build: the catalog
//! is preflighted (including claims shared with `[quipu.ingest_pilot]`), then
//! `GraphSetsConfig::admit_enrolled` verifies the legacy fence and opens every
//! routed owner through S1. The admitted router lives for the process and
//! routes `/update` for its members (aegis-n0wc.5 H1, `update_route`). With
//! `[quipu.graph_sets.reads]` the same router also serves authenticated reads
//! at `POST /graph-sets/query` (aegis-n0wc.5.2, `read`), sized from the same
//! memory budget; without it nothing read-side is mounted. Any refusal exits
//! with one line. Other builds exit with `STARTUP_REFUSAL`. There is never a
//! plain-open fallback.
use crate::SharedStore;

#[cfg(all(unix, feature = "ownership-witness-s3"))]
#[path = "graph_set_read.rs"]
mod read;

fn fail(why: impl std::fmt::Display) -> ! {
    eprintln!("error: graph_sets startup refused: {why}");
    std::process::exit(1)
}

/// Run before any route is mounted. Returns only when the table is inert or
/// every routed owner was admitted, with the `/graph-sets/query` routes when
/// reads are configured. The caller merges them BEFORE `protected_app`, so
/// they keep the ordinary auth layer.
pub(crate) fn start(config: &quipu::QuipuConfig, legacy: &SharedStore) -> Option<axum::Router> {
    let sets = config.graph_sets.as_ref().filter(|s| s.enabled)?;
    #[cfg(all(unix, feature = "ownership-witness-s3"))]
    {
        let catalog = sets
            .preflight(config.ingest_pilot.as_ref())
            .unwrap_or_else(|e| fail(e));
        let revision = catalog.physical().revision();
        let owners = catalog.physical().routes().count();
        let catalog = std::sync::Arc::new(catalog);
        // Sized before any owner opens: the owners take this admission's
        // allocation, so reads and owners share one split of memory_mib.
        let reads = sets.reads.as_ref().map(|reads| {
            let admission =
                read_admission(&catalog, sets.memory_mib, reads).unwrap_or_else(|e| fail(e));
            (reads, admission)
        });
        let shared = reads.as_ref().map(|(_, admission)| admission.allocation());
        let writer = legacy.writer.lock();
        let router = sets
            .admit_enrolled(catalog.clone(), &writer, shared, |store| {
                configure(store, config);
                Ok(())
            })
            .unwrap_or_else(|e| fail(e));
        drop(writer);
        // Process-lifetime, like the served StoreHandle: the owners' leases
        // must outlive every request.
        let router: &'static _ = Box::leak(Box::new(router));
        crate::update::update_route::install(router).unwrap_or_else(|e| fail(e));
        let routes = reads.map(|(reads, admission)| {
            let config = crate::knot_durable::owner::startup::read_config(reads);
            read::routes(router, data_files(&catalog, &sets.root), config, admission)
                .unwrap_or_else(|e| fail(e.message()))
        });
        eprintln!(
            "graph sets ENABLED: {owners} enrolled owner(s) admitted at catalog revision {revision}; /update routes their members{}",
            if routes.is_some() {
                "; /graph-sets/query reads them"
            } else {
                "; reads NOT mounted"
            },
        );
        routes
    }
    #[cfg(not(all(unix, feature = "ownership-witness-s3")))]
    {
        let _ = legacy;
        if let Some(reason) = sets.startup_refusal(config.ingest_pilot.as_ref()) {
            fail(reason);
        }
        None
    }
}

/// Composed-read admission over the graph sets' one memory budget: the
/// legacy participant and each owner hold a writer, a fence and readers.
#[cfg(all(unix, feature = "ownership-witness-s3"))]
fn read_admission(
    catalog: &std::sync::Arc<quipu::graph_catalog::GraphSetCatalog>,
    memory_mib: u64,
    reads: &quipu::config::PilotReadsConfig,
) -> quipu::Result<quipu::read_authorization::ComposedReadAdmission> {
    let bytes = |mib: u64| {
        mib.checked_mul(1 << 20).ok_or_else(|| {
            quipu::Error::InvalidValue("graph-set reads: reservation overflows".into())
        })
    };
    let readers = catalog.physical().budget().readers_per_owner as u64;
    quipu::read_authorization::ComposedReadAdmission::for_graph_sets(
        catalog.clone(),
        memory_mib,
        readers + 2,
        reads.concurrency,
        quipu::read_authorization::ComposedReadReservation {
            runtime_bytes: bytes(reads.runtime_mib)?,
            view_bytes: bytes(reads.view_mib)?,
            query_bytes: bytes(reads.query_mib)?,
            max_response_bytes: usize::try_from(bytes(reads.max_response_mib)?).map_err(|_| {
                quipu::Error::InvalidValue("graph-set reads: response bound too large".into())
            })?,
        },
    )
}

/// Every data file the catalog names under `root`, legacy participant
/// included: the read policy may share none of their inodes.
#[cfg(all(unix, feature = "ownership-witness-s3"))]
fn data_files(
    catalog: &quipu::graph_catalog::GraphSetCatalog,
    root: &std::path::Path,
) -> Vec<std::path::PathBuf> {
    let physical = catalog.physical();
    physical
        .routes()
        .map(|route| route.file_id.clone())
        .chain(physical.legacy_participant().map(|p| p.file_id.clone()))
        .map(|file_id| root.join(format!("{file_id}.db")))
        .collect()
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
