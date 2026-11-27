//! `quipu pilot enroll` entry point (aegis-4qv.2): resolves the pilot witness
//! configuration into the S3 client and calls the enrollment library. The
//! CLI is a thin wrapper over [`enroll_pilot_owner`].
//!
//! Credentials come only from the owner-only `credentials_file` the config
//! names (group/other-readable is refused); they never reach argv, the
//! environment or the returned report.
use std::path::Path;
use std::time::Duration;

use super::ownership_enroll::{
    EnrollRequest, EnrollWitness, enroll_owner, plan_enrollment, refused,
};
use crate::config::IngestPilotConfig;
use crate::error::Result;
use crate::ownership_handoff_model::AuthorityUnit;
use crate::ownership_handoff_model::witness::s3::{Credentials, Location, S3Objects};

/// One enrollment invocation. `execute == false` is the dry run: read-only
/// preflight plus the planned authority, with no local or remote write.
#[derive(Debug, Clone)]
pub struct PilotEnroll<'a> {
    /// Existing native Quipu database to enroll.
    pub db: &'a Path,
    /// Journal path to create (or resume) for this lineage.
    pub journal: &'a Path,
    /// Authority unit; only `full-store` exists.
    pub unit: &'a str,
    /// Existing principal that owns this placement.
    pub principal: &'a str,
    /// Perform the enrollment; otherwise plan only.
    pub execute: bool,
}

/// Enroll (or plan enrolling) one owner against the pilot witness. Returns a
/// JSON report: `{"mode":"dry-run","plan":..}` or `{"mode":"executed","receipt":..}`.
pub fn enroll_pilot_owner(
    config: &IngestPilotConfig,
    request: &PilotEnroll<'_>,
) -> Result<serde_json::Value> {
    let unit = match request.unit {
        "full-store" | "FULL_STORE" => AuthorityUnit::FullStore,
        _ => return Err(refused("unknown authority unit; expected full-store")),
    };
    if config.timeout_secs == 0 || config.timeout_secs > 30 {
        return Err(refused("witness timeout must be 1..=30 seconds"));
    }
    // Same rule as enrolled startup; refused before credentials, the witness
    // or any local file is touched, in dry run and execute alike.
    if !super::graph_router::admissible_witness_profile(&config.profile, &config.endpoint) {
        return Err(refused(
            "witness endpoint must be https:// and the profile a 1..=128 byte \
             [A-Za-z0-9_.-] name; enrolled startup refuses anything else",
        ));
    }
    let location = Location::new(
        &config.endpoint,
        &config.bucket,
        &config.region,
        &config.prefix,
    )
    .ok_or_else(|| refused("invalid witness location"))?;
    let (key, secret) = config.read_credentials()?;
    let credentials =
        Credentials::new(key, secret).ok_or_else(|| refused("invalid witness credentials"))?;
    let roots = match config.read_witness_ca()? {
        Some(der) => {
            Some(S3Objects::trust_root(&der).ok_or_else(|| refused("invalid witness CA"))?)
        }
        None => None,
    };
    let backend = S3Objects::with_roots(
        location,
        credentials,
        Duration::from_secs(config.timeout_secs),
        roots,
    );
    let req = EnrollRequest {
        db_path: request.db,
        journal_path: request.journal,
        unit,
        profile: &config.profile,
        principal: request.principal,
        policy: &config.installed_policy,
        witness: EnrollWitness {
            profile: &config.profile,
            objects: &backend,
        },
    };
    if request.execute {
        let receipt = enroll_owner(&req)?;
        Ok(serde_json::json!({"mode": "executed", "receipt": receipt}))
    } else {
        let plan = plan_enrollment(&req)?;
        Ok(serde_json::json!({"mode": "dry-run", "plan": plan}))
    }
}
