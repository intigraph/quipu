//! `[quipu.graph_sets]` (aegis-n0wc.5): explicit startup selection of graph-set
//! owners described by a graph-set catalog file. Generic: nothing here names an
//! application. Absent (the default) is byte-identical startup: nothing is
//! read, opened or resolved. `enabled = false` keeps the table inert for
//! staging an edit.
//!
//! Enabled startup validates the catalog and refuses any graph or physical
//! file also claimed by the `[quipu.ingest_pilot]` catalog, then REFUSES to
//! serve: production owners open only through enrolled ownership (S1), and
//! server startup does not yet call the native enrolled graph-set open. A plain
//! writable open would weaken ownership, so there is no such fallback.
use serde::Deserialize;
use std::path::PathBuf;

use super::IngestPilotConfig;
use crate::graph_catalog::{Catalog, GraphSetCatalog};

/// Every field is required: an incomplete table refuses at config load.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphSetsConfig {
    /// Explicit opt-in.
    pub enabled: bool,
    /// Owner-written graph-set catalog (see `GraphSetCatalog::load_file`).
    pub catalog_file: PathBuf,
    /// Existing canonical directory holding the catalog-named owner files.
    pub root: PathBuf,
    /// Served owner policy; must equal each catalog route's policy.
    pub installed_policy: String,
    /// Memory budget (MiB) for the legacy participant plus owner connections.
    pub memory_mib: u64,
}

impl GraphSetsConfig {
    /// Load and validate the catalog, then refuse any claim shared with an
    /// enabled ingest pilot: the same logical graph, the same file or the
    /// same store lineage would give one graph two writers.
    pub fn preflight(&self, pilot: Option<&IngestPilotConfig>) -> crate::Result<GraphSetCatalog> {
        let catalog = GraphSetCatalog::load_file(&self.catalog_file)?;
        if catalog
            .physical()
            .routes()
            .any(|route| route.policy != self.installed_policy)
        {
            return Err(refused("installed policy differs from catalog"));
        }
        if let Some(pilot) = pilot.filter(|p| p.enabled) {
            if catalog
                .owner_graphs(&pilot.graph)
                .is_ok_and(|members| !members.contains(&crate::schema::ROOT_GRAPH_IRI))
            {
                return Err(refused(
                    "the ingest pilot's graph is also a graph-set member",
                ));
            }
            let other = Catalog::load_file(&pilot.catalog_file)?;
            catalog.refuse_shared_claims(&self.root, &other, &pilot.root)?;
        }
        Ok(catalog)
    }
}

fn refused(message: &str) -> crate::Error {
    crate::Error::PolicyDenied(format!("graph sets: {message}"))
}

#[cfg(test)]
#[path = "graph_sets_tests.rs"]
mod tests;
