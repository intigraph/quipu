//! Explicit serving profile. Merely compiling this type enables nothing.

use super::{OwnerReadPolicy, Result, denied};
use serde::{Deserialize, Serialize};

/// Separate owner-controlled policy authority. Never a tenant data/import file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedReadConfig {
    pub policy_path: String,
    pub owner: String,
}

impl ScopedReadConfig {
    /// Serving opens existing authority read-only; it never initializes grants.
    pub fn open_policy(&self) -> Result<OwnerReadPolicy> {
        OwnerReadPolicy::open_existing(&self.policy_path, &self.owner)
    }

    /// First source profile has a dedicated read-only listener. Replication,
    /// followers and process workers require their separate R3 qualification.
    pub fn validate(
        &self,
        config: &crate::config::ServerConfig,
        data: &std::path::Path,
    ) -> Result<()> {
        let policy = std::fs::canonicalize(&self.policy_path).map_err(|_| denied())?;
        let data = std::fs::canonicalize(data).map_err(|_| denied())?;
        if !physically_distinct(&policy, &data)?
            || !config.read_only
            || config.replica.is_some()
            || config.relay.is_some()
            || config.read_processes != 0
            || config.session_keys.required
            || config.crew_credentials_file.is_none()
        {
            return Err(denied());
        }
        self.open_policy()?;
        let path = config.crew_credentials_file.as_deref().ok_or_else(denied)?;
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|_| denied())?
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| denied())?;
        crate::crew_credentials::CredentialRegistry::parse(&bytes, "quipu")
            .map_err(|_| denied())?;
        Ok(())
    }
}

#[cfg(unix)]
fn physically_distinct(policy: &std::path::Path, data: &std::path::Path) -> Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let p = std::fs::metadata(policy).map_err(|_| denied())?;
    let d = std::fs::metadata(data).map_err(|_| denied())?;
    Ok(p.dev() != d.dev() || p.ino() != d.ino())
}

#[cfg(not(unix))]
fn physically_distinct(_: &std::path::Path, _: &std::path::Path) -> Result<bool> {
    Err(denied())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::read_authorization::{ReadGrant, ReadOperation};

    #[test]
    fn explicit_profile_refuses_missing_malformed_shared_or_incompatible_authority() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data.db");
        let _source = crate::Store::open(data.to_str().unwrap()).unwrap();
        let policy_path = dir.path().join("policy.db");
        let mut policy = OwnerReadPolicy::open(policy_path.to_str().unwrap(), "urn:owner").unwrap();
        policy
            .apply(
                &ReadGrant {
                    version: 1,
                    id: "urn:grant".into(),
                    principal: "urn:p".into(),
                    graphs: ["urn:a".into()].into(),
                    operations: [ReadOperation::Query].into(),
                    valid_from: 1,
                    valid_until: 100,
                    max_offline_seconds: 300,
                    issuer: "urn:owner".into(),
                    revision: 1,
                    revoked: false,
                },
                "private profile fixture",
            )
            .unwrap();
        let registry = dir.path().join("registry.json");
        std::fs::write(&registry, r#"{"version":1,"credentials":[]}"#).unwrap();
        let profile = ScopedReadConfig {
            policy_path: policy_path.to_string_lossy().into(),
            owner: "urn:owner".into(),
        };
        let config = crate::config::ServerConfig {
            read_only: true,
            crew_credentials_file: Some(registry.to_string_lossy().into()),
            ..Default::default()
        };
        assert!(profile.validate(&config, &data).is_ok());
        let mut incompatible = config.clone();
        incompatible.read_only = false;
        assert!(profile.validate(&incompatible, &data).is_err());
        incompatible = config.clone();
        incompatible.read_processes = 1;
        assert!(profile.validate(&incompatible, &data).is_err());
        incompatible = config.clone();
        incompatible.session_keys.required = true;
        assert!(profile.validate(&incompatible, &data).is_err());
        let mut bad = profile.clone();
        bad.owner = "urn:tenant".into();
        assert!(bad.validate(&config, &data).is_err());
        bad = profile.clone();
        bad.policy_path = dir.path().join("absent.db").to_string_lossy().into();
        assert!(bad.validate(&config, &data).is_err());
        assert!(!dir.path().join("absent.db").exists());
        let alias = dir.path().join("data-hardlink.db");
        std::fs::hard_link(&data, &alias).unwrap();
        bad = profile.clone();
        bad.policy_path = alias.to_string_lossy().into();
        assert!(bad.validate(&config, &data).is_err());
        std::fs::write(&registry, b"malformed registry").unwrap();
        assert!(profile.validate(&config, &data).is_err());
    }
}
