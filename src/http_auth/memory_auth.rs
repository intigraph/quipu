//! Memory credentials have no ordinary read/write or replication authority.
use super::BearerPolicy;
use crate::crew_credentials::{CrewPrincipal, MemoryScope};

impl BearerPolicy {
    /// Includes expired matches, which the caller must refuse rather than
    /// treating as an unknown token and falling through to an open endpoint.
    #[must_use]
    pub fn memory_credential(
        &self,
        header: Option<&str>,
    ) -> Option<(CrewPrincipal, MemoryScope, u64)> {
        self.named.memory_credential(header)
    }
}

#[cfg(test)]
#[path = "memory_auth_tests.rs"]
mod tests;
