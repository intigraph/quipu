//! Explicit local supersession; old signed operations are immutable.
use super::*;

/// Local bookkeeping only, never master acceptance or resolver evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Supersession {
    /// Operation retained for audit/retry; never retargeted or re-signed.
    pub old_operation_id: String,
    /// Fingerprint of all old signed bindings.
    pub old_fingerprint: String,
    /// Fresh operation that must be signed separately and admitted normally.
    pub new_operation_id: String,
    /// Fingerprint of all new bindings, before caller-owned signing.
    pub new_fingerprint: String,
}

/// A new proposal and the exact old signed operation it supersedes locally.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RebasedChangeSet {
    /// New frozen base and composed private proposals.
    pub overlay: ClientOverlay,
    /// New canonical signing request; it contains no signature or private key.
    pub prepared: PreparedChangeSet,
    /// Exact old wire object, retained unchanged.
    pub previous: SignedChangeSet,
    /// Local linkage, not authorization, acceptance or resolver evidence.
    pub supersession: Supersession,
}

/// Rebase either prepares a complete proposal or returns explicit conflicts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum RebaseOutcome {
    /// Caller must persist this result, sign the new request and submit it.
    Prepared {
        /// Complete local result, with the previous wire object preserved.
        rebased: Box<RebasedChangeSet>,
    },
    /// No replacement proposal or supersession is produced on conflict.
    Conflicts {
        /// Ours is canonical; theirs is the private proposal.
        records: Vec<crate::merge3::DecisionRecord>,
    },
}

impl ClientOverlay {
    /// Rebase onto a caught-up base of the same lineage, epoch, scope and shapes.
    /// The previous signature must verify and describe this exact overlay. A
    /// fresh operation ID and nonce are mandatory. Neither input is mutated;
    /// failures and conflicts leave the old operation available. This is local
    /// preparation, not current-authority admission or conflict resolution.
    pub fn rebase(
        &self,
        previous: &SignedChangeSet,
        previous_public_key: &str,
        canonical: &BaseJournal,
        shapes: &str,
        identity: ProposalIdentity,
    ) -> Result<RebaseOutcome> {
        previous.verify(previous_public_key)?;
        let old = &previous.envelope;
        let expected = self.prepare(ProposalIdentity {
            key_id: old.key_id.clone(),
            signer: old.signer.clone(),
            operation_id: old.operation_id.clone(),
            issued_at: old.issued_at.clone(),
            nonce: old.nonce.clone(),
        })?;
        if expected.envelope != *old || expected.body != previous.body {
            return Err(refuse(
                "rebase previous operation does not match this overlay",
            ));
        }
        if identity.operation_id == old.operation_id || identity.nonce == old.nonce {
            return Err(refuse("rebase requires a fresh operation ID and nonce"));
        }
        match self.preview(canonical, shapes)? {
            OverlayPreview::Conflicts { records } => Ok(RebaseOutcome::Conflicts { records }),
            OverlayPreview::Composed { triples } => {
                let mut overlay = Self::open(canonical.clone(), self.fork_id().into())?;
                overlay.replace(triples)?;
                let prepared = overlay.prepare(identity)?;
                let supersession = Supersession {
                    old_operation_id: old.operation_id.clone(),
                    old_fingerprint: previous.fingerprint()?,
                    new_operation_id: prepared.envelope.operation_id.clone(),
                    new_fingerprint: digest(&prepared.message()?),
                };
                Ok(RebaseOutcome::Prepared {
                    rebased: Box::new(RebasedChangeSet {
                        overlay,
                        prepared,
                        previous: previous.clone(),
                        supersession,
                    }),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests;
