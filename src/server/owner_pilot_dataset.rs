//! Source-only startup composition. Main never mounts these routes.
use super::{PreparedOwner, refuse};
use crate::base::AppError;
#[cfg(feature = "ownership-witness-s3")]
use quipu::graph_catalog::Catalog;
use quipu::{
    Store,
    store::graph_owner::{CapturedRead, Owner},
};
use std::{sync::Arc, time::Instant};

#[path = "owner_pilot_read.rs"]
mod read;
pub(crate) use read::ReadConfig;

/// Disabled startup returns the original configured writer untouched.
pub(crate) enum Startup {
    Disabled(Box<Store>),
    Enabled(Box<PreparedDataset>),
}

/// One catalog and the sole configured writer for each participant.
/// Read callbacks are internal, already-authorized dataset evaluations, not an
/// HTTP query language. No implicit union or cross-file write is introduced.
pub(crate) struct PreparedDataset {
    pub owner: Arc<PreparedOwner>,
    legacy: Owner,
    native_files: [std::path::PathBuf; 2],
}

impl PreparedDataset {
    #[cfg(feature = "ownership-witness-s3")]
    pub(crate) fn initialize(
        pilot: &quipu::store::graph_router::IngestPilot,
        catalog: Arc<Catalog>,
        legacy: Store,
        config: &quipu::store::graph_router::EnrolledPilotConfig,
        credentials: impl FnOnce() -> quipu::Result<(String, String)>,
        configure: impl FnOnce(&mut Store) -> quipu::Result<()>,
    ) -> Result<Startup, AppError> {
        if !pilot.enabled {
            return Ok(Startup::Disabled(Box::new(legacy)));
        }
        let pin = catalog
            .legacy_participant()
            .ok_or_else(|| refuse("explicit legacy read participant required"))?;
        // The allocation must include BOTH sets of writer/fence/read handles.
        let count = (catalog.budget().readers_per_owner + 2) as u64;
        if config.allocation.connections < count.saturating_mul(2) {
            return Err(refuse("combined startup memory reservation incomplete"));
        }
        let legacy_policy = pin.policy.clone();
        let native_files = [
            config.root.join(format!("{}.db", pin.file_id)),
            config.root.join(format!(
                "{}.db",
                catalog
                    .routes()
                    .next()
                    .ok_or_else(|| refuse("pilot owner missing"))?
                    .file_id
            )),
        ];
        let owner = PreparedOwner::initialize(
            pilot,
            catalog.clone(),
            &legacy,
            config,
            credentials,
            configure,
        )?
        .ok_or_else(|| refuse("enabled startup returned no owner"))?;
        let legacy = Owner::prepare_with_memory(
            catalog,
            &config.root,
            quipu::schema::ROOT_GRAPH_IRI,
            legacy,
            &legacy_policy,
            config.allocation,
        )?;
        Ok(Startup::Enabled(Box::new(Self {
            owner,
            legacy,
            native_files,
        })))
    }

    /// Capture one strict cut; graph selection is explicit and never a grant.
    /// Unknown/duplicate selections refuse before any callback. Every selected
    /// reader is held in its original SQLite snapshot for the entire callback.
    pub(crate) fn read<T>(
        &self,
        graphs: &[&str],
        deadline: Instant,
        query: impl FnOnce(&[CapturedRead<'_>]) -> quipu::Result<T>,
    ) -> Result<T, AppError> {
        if graphs.is_empty() || graphs.len() > 2 {
            return Err(refuse("one or two explicit read participants required"));
        }
        let routed = self
            .owner
            .router
            .owner(&self.owner.graph)
            .ok_or_else(|| refuse("owner missing"))?;
        let mut selected = Vec::with_capacity(graphs.len());
        for graph in graphs {
            selected.push(if *graph == self.owner.graph {
                routed
            } else if *graph == quipu::schema::ROOT_GRAPH_IRI {
                &self.legacy
            } else {
                return Err(refuse("read graph is outside pinned dataset"));
            });
        }
        Owner::read_strict(&selected, deadline, query).map_err(Into::into)
    }

    /// Uses the same owner Arc as strict reads; normal auth middleware remains
    /// the mounting caller's responsibility. Production does not call this.
    pub(crate) fn routes(&self) -> axum::Router {
        super::http::routes(self.owner.clone())
    }

    /// Explicit authenticated read transport over this same admitted dataset.
    /// The caller retains ordinary outer authentication and request logging.
    /// No profile, registry, grant or participant is discovered from a request.
    pub(crate) fn routes_with_reads(
        self: &Arc<Self>,
        config: ReadConfig,
        admission: quipu::read_authorization::ComposedReadAdmission,
    ) -> Result<axum::Router, AppError> {
        read::routes(self.clone(), config, admission)
    }
}

#[cfg(all(test, feature = "ownership-witness-s3"))]
#[path = "owner_pilot_startup_tests.rs"]
mod tests;
