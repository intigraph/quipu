//! Explicit native factory; no environment discovery, enrollment or HTTP wiring.
use super::*;
use crate::memory_budget::MemoryAllocation;
use crate::ownership_handoff_model::WitnessHead;
use crate::ownership_handoff_model::witness::s3::{Credentials, Location, S3Objects};
use crate::ownership_handoff_model::witness::{ObjectWitness, Reply};
use crate::store::ownership_admission::{EnrolledStartup, WitnessSource};
use std::path::PathBuf;
use std::time::Duration;

/// Trusted startup configuration, separate from request graph selection.
/// Secrets are supplied lazily by the initializer, never serialized here.
/// This is not a server TOML switch or permission to enroll/migrate a file.
#[derive(Debug, Clone)]
pub struct EnrolledPilotConfig {
    /// Existing canonical directory containing catalog-named owner files.
    pub root: PathBuf,
    /// Explicit protected S1 inventory; never discovered from a graph IRI.
    pub inventory: PathBuf,
    /// Exact witness profile name required by the enrolled inventory entry.
    pub profile: String,
    /// HTTPS origin; redirects are disabled by the witness transport.
    pub endpoint: String,
    /// Explicit witness bucket, region and object prefix.
    pub bucket: String,
    /// Signing region.
    pub region: String,
    /// Prefix for the existing enrolled lineage, not a new genesis.
    pub prefix: String,
    /// Per-request witness timeout, nonzero and at most thirty seconds.
    pub timeout: Duration,
    /// Optional explicitly trusted DER CA certificate. None uses the transport's
    /// ordinary public roots. This never disables certificate or name checks.
    pub witness_ca_der: Option<Vec<u8>>,
    /// Independently selected served policy; must equal the catalog pin.
    pub installed_policy: String,
    /// Reservation from the initializer's aggregate connection budget.
    pub allocation: MemoryAllocation,
}

struct ProfileSource {
    profile: String,
    store_id: String,
    witness: ObjectWitness<S3Objects>,
}

impl WitnessSource for ProfileSource {
    fn head_evidence(
        &self,
        profile: &str,
        store_id: &str,
        anchor: u64,
        _nonce: &[u8; 32],
    ) -> Reply<(WitnessHead, Vec<(String, String)>)> {
        if profile != self.profile || store_id != self.store_id {
            return Reply::Unknown;
        }
        // No cached head, assumed genesis or write to the witness. S1 owns the
        // fresh nonce and compares this live evidence with inventory/journal.
        self.witness.head_evidence(store_id, anchor)
    }
}

impl EnrolledPilotConfig {
    pub(crate) fn location(&self) -> Result<Location> {
        if self.profile.is_empty()
            || self.profile.len() > 128
            || !self
                .profile
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
            || !self.root.is_absolute()
            || !self.inventory.is_absolute()
            || !self.endpoint.starts_with("https://")
            || self.timeout.is_zero()
            || self.timeout > Duration::from_secs(30)
        {
            return Err(refused("invalid enrolled witness profile configuration"));
        }
        Location::new(&self.endpoint, &self.bucket, &self.region, &self.prefix)
            .ok_or_else(|| refused("invalid enrolled witness location"))
    }

    /// Shared S1 open of one exact catalog route's existing enrolled file:
    /// trust anchor, lazy credentials, live witness source, then
    /// `Owner::open_enrolled`. Callers validate `location` and the policy pin
    /// first. No fallback to an ordinary open, enrollment or legacy exists.
    pub(crate) fn open_owner(
        &self,
        location: Location,
        catalog: Arc<Catalog>,
        route: &crate::graph_catalog::Route,
        credentials: impl FnOnce() -> Result<(String, String)>,
        configure: impl FnOnce(&mut Store) -> Result<()>,
    ) -> Result<Owner> {
        let roots = match self.witness_ca_der.as_deref() {
            Some(der) => Some(
                S3Objects::trust_root(der)
                    .ok_or_else(|| refused("invalid witness CA certificate"))?,
            ),
            None => None,
        };
        let (key, secret) = credentials()?;
        let credentials = Credentials::new(key, secret)
            .ok_or_else(|| refused("invalid enrolled witness credentials"))?;
        let source = ProfileSource {
            profile: self.profile.clone(),
            store_id: route.store_id.clone(),
            witness: ObjectWitness::attach(
                route.store_id.clone(),
                S3Objects::with_roots(location, credentials, self.timeout, roots),
            ),
        };
        let startup = EnrolledStartup {
            inventory: &self.inventory,
            witness: &source,
        };
        Owner::open_enrolled(
            catalog,
            &self.root,
            &route.graph,
            &startup,
            &self.installed_policy,
            configure,
            Some(self.allocation),
        )
    }
}

impl IngestPilot {
    /// Build one router from an existing enrolled owner. OFF invokes neither
    /// callback and performs no configuration, filesystem or network read.
    ///
    /// ON verifies the complete legacy fence before credential resolution or
    /// owner startup. S1 then verifies the inventory, marker, journal and live
    /// witness before installing an ownership pin. `configure` must install
    /// the served writer policy; `allocation` must include all owner readers
    /// and fence connections in the initializer's aggregate service budget.
    /// No fallback to ordinary open, enrollment or legacy execution exists.
    pub fn admit_enrolled(
        &self,
        catalog: Arc<Catalog>,
        legacy: &Store,
        config: &EnrolledPilotConfig,
        credentials: impl FnOnce() -> Result<(String, String)>,
        configure: impl FnOnce(&mut Store) -> Result<()>,
    ) -> Result<Option<Router>> {
        let owner_catalog = catalog.clone();
        self.admit(catalog, legacy, || {
            let location = config.location()?;
            let graph = self
                .graph
                .as_deref()
                .ok_or_else(|| refused("pilot graph missing"))?;
            let route = owner_catalog
                .routes()
                .find(|r| r.graph == graph)
                .ok_or_else(|| refused("pilot route missing"))?;
            if config.installed_policy != route.policy {
                return Err(refused("installed policy differs from pilot catalog"));
            }
            config.open_owner(
                location,
                owner_catalog.clone(),
                route,
                credentials,
                configure,
            )
        })
    }
}
