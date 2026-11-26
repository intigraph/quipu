//! Production catalog loader (aegis-4qv). The catalog is an operator-written
//! JSON file; nothing is discovered from a graph IRI, a directory listing or a
//! request. Every field is required and unknown fields refuse, so a typo cannot
//! silently drop a pin. The file is validated whole through [`Catalog::new`] and
//! [`Catalog::with_legacy_participant`]; live identity (store ID, log epoch,
//! policy, file) is then re-verified against the actual files by native owner
//! admission, which refuses on any mismatch.
use super::{Budget, Catalog, Route};
use crate::{Error, Result};
use serde::Deserialize;
use std::path::Path;

/// The only accepted catalog file format version.
pub const CATALOG_FILE_VERSION: u32 = 1;
pub(super) const MAX_BYTES: u64 = 64 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileRoute {
    pub(super) graph: String,
    pub(super) file_id: String,
    pub(super) store_id: String,
    pub(super) log_epoch: String,
    pub(super) generation: u64,
    pub(super) policy: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileBudget {
    pub(super) max_owners: usize,
    pub(super) queued_per_owner: usize,
    pub(super) readers_per_owner: usize,
    pub(super) max_queued: usize,
    pub(super) max_connections: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogFile {
    version: u32,
    revision: u64,
    legacy_store_id: String,
    budget: FileBudget,
    routes: Vec<FileRoute>,
    legacy_participant: FileRoute,
}

impl From<FileRoute> for Route {
    fn from(r: FileRoute) -> Self {
        Route {
            graph: r.graph,
            file_id: r.file_id,
            store_id: r.store_id,
            log_epoch: r.log_epoch,
            generation: r.generation,
            policy: r.policy,
        }
    }
}

impl Catalog {
    /// Parse one catalog document. The legacy participant is mandatory: the
    /// pilot's strict reads require it, so a catalog without one refuses here.
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let file: CatalogFile = serde_json::from_slice(bytes).map_err(|e| {
            Error::PolicyDenied(format!(
                "graph owner admission: malformed catalog file at line {} column {}",
                e.line(),
                e.column()
            ))
        })?;
        if file.version != CATALOG_FILE_VERSION {
            return Err(super::refuse("unsupported catalog file version"));
        }
        let b = file.budget;
        Catalog::new(
            file.revision,
            &file.legacy_store_id,
            file.routes.into_iter().map(Route::from).collect(),
            Budget {
                max_owners: b.max_owners,
                queued_per_owner: b.queued_per_owner,
                readers_per_owner: b.readers_per_owner,
                max_queued: b.max_queued,
                max_connections: b.max_connections,
            },
        )?
        .with_legacy_participant(file.legacy_participant.into())
    }

    /// Load a bounded, regular file that no group or other user can write.
    pub fn load_file(path: &Path) -> Result<Self> {
        Self::from_json(&crate::config::ingest_pilot::read_protected(
            path, MAX_BYTES, false,
        )?)
    }
}

#[cfg(test)]
#[path = "catalog_file_tests.rs"]
mod tests;
