//! `[quipu.ingest_pilot]` (aegis-4qv): explicit startup for one enrolled
//! per-graph owner. Absent (the default) is byte-identical startup: nothing here
//! is read, opened or resolved. Present means the server admits the pilot or
//! exits; there is no fallback to the legacy writer.
//!
//! Secrets never live in TOML. `credentials_file` names a separate owner-only
//! (0600) file read lazily by the startup initializer, after the legacy fence
//! and the catalog have been verified.
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Trusted startup selection of the pilot owner, its catalog and its witness.
/// Every field is required: an incomplete table refuses at config load.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IngestPilotConfig {
    /// Explicit opt-in. `false` keeps the table inert for staging an edit.
    pub enabled: bool,
    /// Exact graph IRI; must be the catalog's only route.
    pub graph: String,
    /// Existing canonical directory holding the catalog-named owner files.
    pub root: PathBuf,
    /// Protected S1 enrollment inventory.
    pub inventory: PathBuf,
    /// Witness profile name required by the inventory entry.
    pub profile: String,
    /// HTTPS witness origin.
    pub endpoint: String,
    /// Witness bucket.
    pub bucket: String,
    /// Witness signing region.
    pub region: String,
    /// Object prefix of the existing enrolled lineage.
    pub prefix: String,
    /// Per-request witness timeout, 1..=30 seconds.
    pub timeout_secs: u64,
    /// Optional DER trust anchor; absent uses ordinary public roots.
    pub witness_ca_file: Option<PathBuf>,
    /// Served owner policy; must equal the catalog's route policy.
    pub installed_policy: String,
    /// Owner-only JSON file holding the witness access key and secret.
    pub credentials_file: PathBuf,
    /// Owner-written JSON catalog (see `graph_catalog::Catalog::load_file`).
    pub catalog_file: PathBuf,
    /// Memory budget (MiB) for the legacy participant plus owner connections.
    pub memory_mib: u64,
    /// Optional `[quipu.ingest_pilot.reads]` (aegis-4qv.8): authenticated
    /// composed reads at `POST /owner/query`. Absent mounts only `/owner/knot*`.
    #[serde(default)]
    pub reads: Option<PilotReadsConfig>,
}

/// Owner-selected read authority for `POST /owner/query`. Every key is
/// required; none is discovered from a request. The policy and registry are
/// separate files from the data files, and both are re-read per request.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PilotReadsConfig {
    /// `OwnerReadPolicy` SQLite file holding the read grants.
    pub policy_file: PathBuf,
    /// Issuer IRI the grants in `policy_file` must carry.
    pub owner: String,
    /// Named-bearer credential registry (JSON) the read identity resolves in.
    pub registry_file: PathBuf,
    /// Per-read deadline, 1..=30 seconds.
    pub timeout_secs: u64,
    /// Concurrent composed reads admitted at once (scratch connections).
    pub concurrency: usize,
    /// Runtime share (MiB) kept outside the read admission.
    pub runtime_mib: u64,
    /// Per-read view scratch reservation (MiB).
    pub view_mib: u64,
    /// Per-read evaluator plus serialized output reservation (MiB).
    pub query_mib: u64,
    /// Hard response body ceiling (MiB); must not exceed `query_mib`.
    pub max_response_mib: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialsFile {
    access_key_id: String,
    secret_access_key: String,
}

/// Read a bounded, regular, owner-only file. Errors name the file, never bytes.
pub(crate) fn read_protected(path: &Path, max: u64, owner_only: bool) -> crate::Result<Vec<u8>> {
    use std::io::Read;
    let refuse =
        |why: &str| crate::Error::PolicyDenied(format!("ingest pilot: {} {why}", path.display()));
    let file = std::fs::File::open(path).map_err(|_| refuse("cannot be opened"))?;
    let meta = file.metadata().map_err(|_| refuse("cannot be inspected"))?;
    if !meta.is_file() {
        return Err(refuse("is not a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let forbidden = if owner_only { 0o077 } else { 0o022 };
        if meta.permissions().mode() & forbidden != 0 {
            return Err(refuse("has unsafe permissions"));
        }
    }
    #[cfg(not(unix))]
    let _ = owner_only;
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| refuse("cannot be read"))?;
    if bytes.len() as u64 > max {
        return Err(refuse("exceeds its size bound"));
    }
    Ok(bytes)
}

impl IngestPilotConfig {
    /// Resolve the witness key pair. Called only by the enabled initializer.
    pub fn read_credentials(&self) -> crate::Result<(String, String)> {
        let bytes = read_protected(&self.credentials_file, 4096, true)?;
        let parsed: CredentialsFile = serde_json::from_slice(&bytes).map_err(|_| {
            crate::Error::PolicyDenied("ingest pilot: malformed credentials file".into())
        })?;
        Ok((parsed.access_key_id, parsed.secret_access_key))
    }

    /// Optional DER trust anchor, bounded like the witness transport's own limit.
    pub fn read_witness_ca(&self) -> crate::Result<Option<Vec<u8>>> {
        self.witness_ca_file
            .as_deref()
            .map(|path| read_protected(path, 64 * 1024, false))
            .transpose()
    }
}

#[cfg(test)]
#[path = "ingest_pilot_tests.rs"]
mod tests;
