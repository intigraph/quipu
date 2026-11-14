//! Explicit recipient setup, never producer-authorized shape installation.
use crate::{Error, Result, Store};
use serde::{Deserialize, Serialize};

/// A released profile reference is its exact local shapes registry name.
/// Digest is SHA256 of the exact reviewed Turtle bytes, not RDF equivalence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequiredProfile {
    /// Exact shapes registry name, including the producer release/ref.
    pub reference: String,
    /// Lowercase SHA256 of the reviewed Turtle file bytes, with sha256: prefix.
    pub digest: String,
}

impl RequiredProfile {
    pub(crate) fn validate(&self) -> Result<()> {
        let valid_ref = !self.reference.is_empty()
            && self.reference.len() <= 1024
            && self
                .reference
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || ":/._@-".contains(c))
            && !self.reference.starts_with('-');
        let valid_digest = self.digest.strip_prefix("sha256:").is_some_and(|hex| {
            hex.len() == 64
                && hex
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        });
        if !valid_ref || !valid_digest {
            return Err(Error::InvalidValue("required_profile needs an exact registry reference and lowercase sha256:<64 hex> digest".into()));
        }
        Ok(())
    }

    pub(crate) fn require_present(&self, store: &Store) -> Result<()> {
        self.validate()?;
        let current = store.list_shapes()?;
        let matches: Vec<_> = current
            .iter()
            .filter(|(name, _, _)| name == &self.reference)
            .collect();
        if matches.len() != 1 || super::sha256(matches[0].1.as_bytes()) != self.digest {
            return Err(Error::InvalidValue(format!(
                "required recipient profile {} at {} is absent, different or ambiguous; no import was staged. Setup: verify the released profile file has digest {}, then run `quipu shapes load {} <verified-profile.ttl> --db <recipient-db>` through the recipient's governed shapes path; retry load only after setup. Incoming shapes are not installed automatically.",
                self.reference, self.digest, self.digest, self.reference
            )));
        }
        Ok(())
    }
}
