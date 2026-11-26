//! Graph-set catalog file (aegis-n0wc.5, Phase 1). Operator-written JSON that
//! names every logical graph of every physical owner by its EXACT IRI. Nothing
//! is expanded from a prefix, pattern, directory listing or request, so an
//! open-ended family (for example weekly message windows) cannot enter a set by
//! accident; it stays on the legacy path until a membership rule is designed.
//! Every field is required and unknown fields refuse. The whole document is
//! validated through [`GraphSetCatalog::new`] and
//! [`GraphSetCatalog::with_legacy_graphs`]; live identity is still re-verified
//! by native owner preparation. Loading grants no writer and routes nothing.
use super::{GraphSet, GraphSetCatalog};
use crate::graph_catalog::{
    Budget, Route,
    catalog_file::{FileBudget, FileRoute, MAX_BYTES},
};
use crate::{Error, Result};
use serde::Deserialize;
use std::path::Path;

/// The only accepted graph-set catalog file format version.
pub const GRAPH_SET_FILE_VERSION: u32 = 1;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileSet {
    owner: FileRoute,
    graphs: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileLegacy {
    participant: FileRoute,
    graphs: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GraphSetFile {
    version: u32,
    revision: u64,
    legacy_store_id: String,
    max_graphs: usize,
    budget: FileBudget,
    sets: Vec<FileSet>,
    legacy: FileLegacy,
}

impl GraphSetCatalog {
    /// Parse one graph-set catalog document. The legacy membership is
    /// mandatory and must name ROOT: strict composed reads select it by name.
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let file: GraphSetFile = serde_json::from_slice(bytes).map_err(|e| {
            Error::PolicyDenied(format!(
                "graph owner admission: malformed graph-set catalog file at line {} column {}",
                e.line(),
                e.column()
            ))
        })?;
        if file.version != GRAPH_SET_FILE_VERSION {
            return Err(crate::graph_catalog::refuse(
                "unsupported graph-set catalog file version",
            ));
        }
        let b = file.budget;
        GraphSetCatalog::new(
            file.revision,
            &file.legacy_store_id,
            file.sets
                .into_iter()
                .map(|s| GraphSet {
                    owner: Route::from(s.owner),
                    graphs: s.graphs,
                })
                .collect(),
            Budget {
                max_owners: b.max_owners,
                queued_per_owner: b.queued_per_owner,
                readers_per_owner: b.readers_per_owner,
                max_queued: b.max_queued,
                max_connections: b.max_connections,
            },
            file.max_graphs,
        )?
        .with_legacy_graphs(file.legacy.participant.into(), file.legacy.graphs)
    }

    /// Load a bounded, regular file that no group or other user can write.
    pub fn load_file(path: &Path) -> Result<Self> {
        Self::from_json(&crate::config::ingest_pilot::read_protected(
            path, MAX_BYTES, false,
        )?)
    }
}

#[cfg(test)]
#[path = "file_tests.rs"]
mod tests;
