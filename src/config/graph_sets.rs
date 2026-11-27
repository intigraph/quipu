//! `[quipu.graph_sets]` (aegis-n0wc.5): explicit startup selection of graph-set
//! owners described by a graph-set catalog file. Generic: nothing here names an
//! application. Absent (the default) is byte-identical startup: nothing is
//! read, opened or resolved. `enabled = false` keeps the table inert for
//! staging an edit.
//!
//! Enabled startup validates the catalog and refuses any graph or physical
//! file also claimed by the `[quipu.ingest_pilot]` catalog. A build with
//! `ownership-witness-s3` then opens every routed owner through enrolled
//! ownership (S1, [`GraphSetsConfig::admit_enrolled`]); any refusal refuses
//! the whole startup. Other builds refuse with [`STARTUP_REFUSAL`]. A plain
//! writable open would weaken ownership, so there is never such a fallback.
use serde::Deserialize;
use std::collections::BTreeMap;
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
    /// Protected S1 enrollment inventory holding every owner file's record.
    pub inventory: PathBuf,
    /// Witness profile name required by every inventory entry.
    pub profile: String,
    /// HTTPS witness origin.
    pub endpoint: String,
    /// Witness bucket.
    pub bucket: String,
    /// Witness signing region.
    pub region: String,
    /// Per-request witness timeout, 1..=30 seconds.
    pub timeout_secs: u64,
    /// Optional DER trust anchor; absent uses ordinary public roots.
    pub witness_ca_file: Option<PathBuf>,
    /// Owner-only JSON file holding the witness access key and secret.
    pub credentials_file: PathBuf,
    /// Exactly one existing enrolled lineage prefix per routed owner graph.
    pub prefixes: BTreeMap<String, String>,
    /// Optional `[quipu.graph_sets.reads]` (aegis-n0wc.5.2): authenticated
    /// reads of one routed owner's members at `POST /graph-sets/query`, the
    /// same keys and authority as `[quipu.ingest_pilot.reads]`. Absent mounts
    /// nothing; `/update` routing is unchanged either way.
    #[serde(default)]
    pub reads: Option<super::PilotReadsConfig>,
}

#[cfg(all(unix, feature = "ownership-witness-s3"))]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialsFile {
    access_key_id: String,
    secret_access_key: String,
}

/// The one-line startup refusal for an enabled table whose catalog passed
/// preflight in a build that cannot open enrolled graph-set owners (no Unix
/// `ownership-witness-s3`).
pub const STARTUP_REFUSAL: &str =
    "graph_sets enabled but enrolled set-owner open is not available; refusing";

impl GraphSetsConfig {
    /// The reason server startup must refuse, as one line, or None when the
    /// table is disabled (then nothing is read). A preflight refusal names its
    /// own reason; a valid catalog gets [`STARTUP_REFUSAL`]. Never a fallback.
    pub fn startup_refusal(&self, pilot: Option<&IngestPilotConfig>) -> Option<String> {
        if !self.enabled {
            return None;
        }
        let reason = match self.preflight(pilot) {
            Err(e) => e.to_string(),
            Ok(_) => STARTUP_REFUSAL.to_owned(),
        };
        Some(reason.replace(['\n', '\r'], " "))
    }

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

#[cfg(all(unix, feature = "ownership-witness-s3"))]
impl GraphSetsConfig {
    /// Open every routed owner of an already preflighted `catalog` through S1
    /// and admit one router over them. Before any credential is read: the
    /// prefixes must name exactly the routed owners and the legacy fence must
    /// name exactly every routed member. Each owner then opens through
    /// `GraphSetOwner::open_enrolled`; any refusal refuses the whole startup
    /// and drops every owner already opened. Never a plain-open fallback.
    pub fn admit_enrolled(
        &self,
        catalog: std::sync::Arc<GraphSetCatalog>,
        legacy: &crate::Store,
        configure: impl Fn(&mut crate::Store) -> crate::Result<()>,
    ) -> crate::Result<crate::store::graph_owner::GraphSetRouter> {
        use crate::store::graph_owner::{GraphSetOwner, GraphSetRouter};
        use crate::store::graph_router::EnrolledPilotConfig;
        let owners: Vec<&str> = catalog
            .physical()
            .routes()
            .map(|route| route.graph.as_str())
            .collect();
        if self.prefixes.len() != owners.len()
            || owners
                .iter()
                .any(|graph| !self.prefixes.contains_key(*graph))
        {
            return Err(refused(
                "prefixes must name exactly one lineage per routed owner graph",
            ));
        }
        GraphSetRouter::verify_fence(&catalog, legacy)?;
        let readers = catalog.physical().budget().readers_per_owner as u64;
        // With reads, the owners and the read admission share ONE allocation
        // that also reserves the read scratch (as the pilot does).
        let scratch = self
            .reads
            .as_ref()
            .map_or(0, |reads| reads.concurrency as u64);
        let allocation =
            catalog
                .physical()
                .memory_allocation(self.memory_mib, readers + 2, scratch)?;
        let witness_ca_der = self
            .witness_ca_file
            .as_deref()
            .map(|path| super::ingest_pilot::read_protected(path, 64 * 1024, false))
            .transpose()?;
        let credentials = || {
            let bytes = super::ingest_pilot::read_protected(&self.credentials_file, 4096, true)?;
            let parsed: CredentialsFile = serde_json::from_slice(&bytes)
                .map_err(|_| refused("malformed credentials file"))?;
            Ok((parsed.access_key_id, parsed.secret_access_key))
        };
        let mut opened = Vec::with_capacity(owners.len());
        for graph in owners {
            let config = EnrolledPilotConfig {
                root: self.root.clone(),
                inventory: self.inventory.clone(),
                profile: self.profile.clone(),
                endpoint: self.endpoint.clone(),
                bucket: self.bucket.clone(),
                region: self.region.clone(),
                prefix: self.prefixes[graph].clone(),
                timeout: std::time::Duration::from_secs(self.timeout_secs),
                witness_ca_der: witness_ca_der.clone(),
                installed_policy: self.installed_policy.clone(),
                allocation,
            };
            opened.push(GraphSetOwner::open_enrolled(
                catalog.clone(),
                graph,
                &config,
                credentials,
                &configure,
            )?);
        }
        GraphSetRouter::admit(catalog, legacy, opened)
    }
}

fn refused(message: &str) -> crate::Error {
    crate::Error::PolicyDenied(format!("graph sets: {message}"))
}

#[cfg(test)]
#[path = "graph_sets_tests.rs"]
mod tests;
