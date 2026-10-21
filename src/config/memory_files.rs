//! Opt-in file-memory RPC scope, not a per-owner read-secrecy policy.
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MemoryFilesConfig {
    pub enabled: bool,
    pub graph: String,
    pub deployment: String,
    pub project: String,
}

impl MemoryFilesConfig {
    pub fn check(&self, owner: &crate::memory_files::Owner) -> crate::Result<()> {
        if !self.enabled
            || [&self.graph, &self.deployment, &self.project]
                .iter()
                .any(|s| s.len() > 2048 || oxrdf::NamedNode::new(s.as_str()).is_err())
        {
            return Err(crate::Error::CannotVerify(
                "file memory is disabled or unconfigured".into(),
            ));
        }
        if owner.deployment != self.deployment || owner.project != self.project {
            return Err(crate::Error::PolicyDenied(
                "file memory deployment/project differs".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scope_is_explicit_and_cross_crew_reads_are_not_secrecy_gated() {
        let mut owner = crate::memory_files::Owner {
            deployment: "urn:example:deployment".into(),
            project: "urn:example:project".into(),
            crew: "urn:example:alpha".into(),
        };
        assert!(MemoryFilesConfig::default().check(&owner).is_err());
        let config = MemoryFilesConfig {
            enabled: true,
            graph: "urn:example:memory".into(),
            deployment: owner.deployment.clone(),
            project: owner.project.clone(),
        };
        assert!(config.check(&owner).is_ok());
        owner.crew = "urn:example:beta".into();
        assert!(config.check(&owner).is_ok());
        owner.project = "urn:example:unrelated".into();
        assert!(config.check(&owner).is_err());
    }
}
