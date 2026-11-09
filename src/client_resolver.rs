//! Separately signed resolver evidence; the client v1 wire object is unchanged.
//!
//! Signature validity is not validation, registration or permission to commit.
//! The admission writer must independently reconstruct both public artifacts
//! from its reserved snapshot and run the actual validator before accepting.

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::client_changeset::{SignedChangeSet, digest, refuse};
use crate::resolver_contract::{ContentDigest, ProspectiveCandidate, ValidatorManifest};

#[cfg(not(target_arch = "wasm32"))]
use crate::resolver_contract::{ValidationEvidence, ValidationOutcome};

/// Independent from both the v1 client message and the unsigned DTO domains.
pub const DOMAIN: &str = "quipu-client-resolver-artifact-v1";

/// Prospective evidence signed separately from the immutable client proposal.
/// Ordered inputs and decisions are covered through the public candidate encoder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolverArtifact {
    /// Resolver artifact wire version, independent of the client envelope.
    pub version: u32,
    /// Fingerprint of the complete original signed client proposal.
    pub proposal_fingerprint: String,
    /// Supplied prospective rows and ordered source/decision bindings.
    pub candidate: ProspectiveCandidate,
    /// Supplied validator observations; the writer must reconstruct them.
    pub manifest: ValidatorManifest,
}

/// Caller-held private keys never appear in the serialized object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedResolverArtifact {
    /// Evidence sealed by the separate resolver signing domain.
    pub artifact: ResolverArtifact,
    /// Hex Ed25519 signature; no private key material is carried.
    pub signature: String,
}

fn digest_value(value: &str) -> Result<ContentDigest> {
    let bytes = value
        .strip_prefix("sha256:")
        .and_then(|v| hex::decode(v).ok())
        .and_then(|v| <[u8; 32]>::try_from(v).ok())
        .ok_or_else(|| refuse("invalid resolver source digest"))?;
    Ok(ContentDigest(bytes))
}

fn frame(out: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
    let length = u64::try_from(bytes.len()).map_err(|_| refuse("resolver frame too large"))?;
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

impl ResolverArtifact {
    /// Sign with the proposal's own key, without loading or enrolling any key.
    /// This seals supplied evidence; it does not establish its truth or authority.
    pub fn sign(
        self,
        proposal: &SignedChangeSet,
        key: &ring::signature::Ed25519KeyPair,
    ) -> Result<SignedResolverArtifact> {
        proposal.verify(&quipu_signing::public_key_hex(key))?;
        let signature = quipu_signing::sign_hex(key, &self.message(proposal)?);
        Ok(SignedResolverArtifact {
            artifact: self,
            signature,
        })
    }

    /// Bind caller-provided prospective artifacts, without asserting conformance.
    /// The master must compare these with its own actual snapshot and validator.
    pub fn new(
        proposal: &SignedChangeSet,
        candidate: ProspectiveCandidate,
        manifest: ValidatorManifest,
    ) -> Result<Self> {
        let artifact = Self {
            version: 1,
            proposal_fingerprint: proposal.fingerprint()?,
            candidate,
            manifest,
        };
        artifact.check_proposal(proposal)?;
        Ok(artifact)
    }

    /// Check structural bindings only; neither this nor a signature admits data.
    pub fn check_proposal(&self, proposal: &SignedChangeSet) -> Result<()> {
        proposal.changeset()?;
        let envelope = &proposal.envelope;
        let source = &self.candidate.source;
        if self.version != 1
            || self.proposal_fingerprint != proposal.fingerprint()?
            || source.store_id != envelope.store_id
            || source.log_epoch != envelope.log_epoch
            || source.graph != envelope.graph
            || source.base_tx != u64::try_from(envelope.base_tx).unwrap_or(0)
            || source.base_slice_hash != digest_value(&envelope.base_slice_hash)?
            // The retained v1 proposal supports a complete single graph only.
            // An empty or partial resolver scope cannot be treated as that graph.
            || source.scope.is_some()
            || self.candidate.validator != self.manifest.identity()?
            || digest(&self.manifest.combined_shapes) != envelope.shapes_hash
        {
            return Err(refuse("resolver artifact disagrees with client proposal"));
        }
        self.candidate.canonical_bytes()?;
        Ok(())
    }

    /// Domain, explicit version and three length-framed byte strings:
    /// v1 proposal fingerprint, public candidate bytes, public manifest bytes.
    /// This includes every ordered decision/input and every validator identity.
    pub fn message(&self, proposal: &SignedChangeSet) -> Result<Vec<u8>> {
        self.check_proposal(proposal)?;
        let mut out = DOMAIN.as_bytes().to_vec();
        out.push(0);
        out.extend_from_slice(&self.version.to_be_bytes());
        frame(&mut out, self.proposal_fingerprint.as_bytes())?;
        frame(&mut out, &self.candidate.canonical_bytes()?)?;
        frame(&mut out, &self.manifest.canonical_bytes()?)?;
        Ok(out)
    }

    /// Digest of this domain-bound message, distinct from the v1 fingerprint.
    pub fn fingerprint(&self, proposal: &SignedChangeSet) -> Result<String> {
        Ok(digest(&self.message(proposal)?))
    }
}

impl SignedResolverArtifact {
    /// Verify both independently signed messages with the same supplied key.
    /// A transport must still establish that key's *current* master authority.
    pub fn verify(&self, proposal: &SignedChangeSet, public_key: &str) -> Result<()> {
        proposal.verify(public_key)?;
        if !quipu_signing::verify_hex(
            public_key,
            &self.artifact.message(proposal)?,
            &self.signature,
        ) {
            return Err(refuse("invalid resolver artifact signature"));
        }
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl SignedResolverArtifact {
    /// Compare signed evidence with an independently reconstructed candidate and
    /// exactly one execution captured by `Store::observe_validator`.
    ///
    /// This binds evidence only. The caller owns reconstruction, reservation,
    /// current key authority and commit/receipt handling. Even Conforms is the
    /// incremental gate result, not full-graph conformance or a committed write.
    /// Only ROOT can currently be bound: named graph observations expose no IRI.
    pub fn bind_observation(
        &self,
        proposal: &SignedChangeSet,
        public_key: &str,
        reconstructed: &ProspectiveCandidate,
        observations: &[crate::store::ValidatorObservation],
    ) -> Result<ValidationEvidence> {
        self.verify(proposal, public_key)?;
        let [observation] = observations else {
            return Err(refuse(
                "resolver evidence requires exactly one actual gate execution",
            ));
        };
        // The public observation exposes a local graph ID, not its IRI.
        // ROOT is the sole identity provable without a caller-supplied mapping.
        // Do not rebind equal RDF from a named graph (or ROOT) by re-signing it.
        if proposal.envelope.graph != "ROOT" || observation.graph() != crate::schema::ROOT_GRAPH {
            return Err(refuse(
                "resolver observation binding supports actual ROOT only",
            ));
        }
        let manifest = observation.manifest()?;
        if manifest.implementation.options.get("store_id") != Some(&proposal.envelope.store_id) {
            return Err(refuse("resolver source differs from actual observed store"));
        }
        if reconstructed.canonical_bytes()? != self.artifact.candidate.canonical_bytes()?
            || manifest.canonical_bytes()? != self.artifact.manifest.canonical_bytes()?
        {
            return Err(refuse(
                "signed resolver evidence differs from actual writer evidence",
            ));
        }
        // Reuse the RDF parser, not the incremental context selector. The
        // signed full-graph candidate must describe the actual staged RDF.
        let staged = std::str::from_utf8(&manifest.staged_graph)
            .map_err(|_| refuse("actual staged graph is not UTF-8"))?;
        let candidate = reconstructed
            .rows
            .iter()
            .map(|row| format!("{} .\n", row.triple))
            .collect::<String>();
        if crate::share_merge::parse_graph(staged, "observed staged graph")?
            != crate::share_merge::parse_graph(&candidate, "resolver candidate rows")?
        {
            return Err(refuse(
                "resolver candidate rows differ from actual staged graph",
            ));
        }
        let outcome = match observation.validator_conforms() {
            Some(false) => ValidationOutcome::Violates,
            Some(true) if observation.gate_error().is_none() => ValidationOutcome::Conforms,
            _ => ValidationOutcome::Unknown,
        };
        reconstructed.bind_validation(manifest, outcome)
    }
}

#[cfg(test)]
mod tests;
