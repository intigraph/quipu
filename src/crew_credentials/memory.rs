//! Restricted identity lookup is separate from ordinary named-writer lookup.
use super::{CredentialRegistry, CrewPrincipal, MemoryScope};
use sha2::{Digest, Sha256};

pub(super) fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

impl CredentialRegistry {
    /// Recognize even an expired memory credential so it cannot fall through to
    /// an open read or another bearer policy. Expiry is checked at authorization.
    #[must_use]
    pub fn memory_credential(
        &self,
        header: Option<&str>,
    ) -> Option<(CrewPrincipal, MemoryScope, u64)> {
        let token = crate::http_auth::parse_bearer(header?)?;
        if token.len() > 4096 {
            return None;
        }
        let digest = format!("{:x}", Sha256::digest(token.as_bytes()));
        let mut matched = None;
        for credential in &self.credentials {
            let difference = digest
                .bytes()
                .zip(credential.token_sha256.bytes())
                .fold(0u8, |diff, (a, b)| diff | (a ^ b));
            if difference == 0
                && let (Some(scope), Some(expiry)) =
                    (&credential.memory_scope, credential.expires_at_epoch_secs)
            {
                matched = Some((
                    CrewPrincipal {
                        iri: credential.principal.clone(),
                        credential_id: credential.credential_id.clone(),
                    },
                    scope.clone(),
                    expiry,
                ));
            }
        }
        matched
    }

    #[must_use]
    pub fn contains_memory_credentials(&self) -> bool {
        self.credentials
            .iter()
            .any(|entry| entry.memory_scope.is_some())
    }
}
