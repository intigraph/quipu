//! Private overlay and caller-owned signing preparation.
use super::*;

/// An overlay is private proposal state, never a mutation of its base journal.
/// Canonical catch-up is owned by the applier and cannot overwrite this value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientOverlay {
    base: BaseJournal,
    fork_id: String,
    proposed: BTreeSet<String>,
}

/// Caller-owned identity and replay binding. A key stays outside serialized
/// overlay state and outside the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalIdentity {
    /// Governed registration IRI holding the signing key.
    pub key_id: String,
    /// Verifier name on that same registration.
    pub signer: String,
    /// Non-nil lowercase UUID retained for identical retries.
    pub operation_id: String,
    /// Claimed signing time; never used to backdate authority.
    pub issued_at: String,
    /// 128-bit lowercase hex value, included in the signature.
    pub nonce: String,
}

impl ClientOverlay {
    /// Open private proposals over an immutable, validated base.
    pub fn open(base: BaseJournal, fork_id: String) -> Result<Self> {
        base.validate()?;
        oxrdf::NamedNode::new(fork_id.clone()).map_err(|_| refuse("invalid overlay identity"))?;
        let proposed = base.rows.iter().map(|row| row.triple.clone()).collect();
        Ok(Self {
            base,
            fork_id,
            proposed,
        })
    }

    /// The frozen base, independent of canonical catch-up.
    pub fn base(&self) -> &BaseJournal {
        &self.base
    }
    /// The persistent local fork bound by every proposal from this overlay.
    pub fn fork_id(&self) -> &str {
        &self.fork_id
    }
    /// The locally proposed graph.
    pub fn read(&self) -> &BTreeSet<String> {
        &self.proposed
    }

    /// Replace local proposals after canonical/skolemized term checks.
    pub fn replace(&mut self, proposed: impl IntoIterator<Item = String>) -> Result<()> {
        let proposed: BTreeSet<_> = proposed.into_iter().collect();
        for triple in &proposed {
            let s = Statement::new(triple, None);
            let quad = s.to_quad()?;
            if s.has_blank_node() || Statement::from_quad(&quad).triple != *triple {
                return Err(refuse("overlay must use canonical skolemized triples"));
            }
        }
        self.proposed = proposed;
        Ok(())
    }

    /// Build the exact unsigned body to hand to an external/shared signer.
    /// Validate and construct the existing unsigned interchange format.
    pub fn changeset(&self) -> Result<ChangeSet> {
        self.base.validate()?;
        oxrdf::NamedNode::new(self.fork_id.clone())
            .map_err(|_| refuse("invalid overlay identity"))?;
        let after = self.proposed.iter().map(|triple| {
            Statement::new(
                triple,
                (self.base.graph != "ROOT").then(|| self.base.graph.clone()),
            )
        });
        Ok(ChangeSet::from_states(
            Header {
                base_tx: Some(self.base.base_tx),
                store_id: Some(self.base.store_id.clone()),
                shapes_hash: Some(self.base.shapes_hash.clone()),
                source: Some("client-overlay".into()),
                ..Header::default()
            },
            self.base.statements(),
            after,
        ))
    }

    /// Prepare canonical bytes without accepting or serializing a private key.
    pub fn prepare(&self, identity: ProposalIdentity) -> Result<PreparedChangeSet> {
        let changeset = self.changeset()?;
        let body = changeset.to_sparql_update();
        let envelope = Envelope {
            audience: self.base.store_id.clone(),
            store_id: self.base.store_id.clone(),
            log_epoch: self.base.log_epoch.clone(),
            fork_id: self.fork_id.clone(),
            graph: self.base.graph.clone(),
            scope_digest: crate::replica_manifest::scope_digest(
                std::slice::from_ref(&self.base.graph),
                &[],
            ),
            base_tx: self.base.base_tx,
            base_slice_hash: self.base.slice_hash()?,
            shapes_hash: self.base.shapes_hash.clone(),
            delta_sha256: format!("sha256:{}", changeset.digest()),
            body_sha256: digest(body.as_bytes()),
            key_id: identity.key_id,
            signer: identity.signer,
            operation_id: identity.operation_id,
            issued_at: identity.issued_at,
            nonce: identity.nonce,
        };
        let prepared = PreparedChangeSet { envelope, body };
        prepared.message()?;
        Ok(prepared)
    }

    /// Sign with a caller-owned private key using the shared signing crate.
    pub fn sign(
        &self,
        key: &ring::signature::Ed25519KeyPair,
        identity: ProposalIdentity,
    ) -> Result<SignedChangeSet> {
        let prepared = self.prepare(identity)?;
        let signature = quipu_signing::sign_hex(key, &prepared.message()?);
        Ok(SignedChangeSet {
            envelope: prepared.envelope,
            body: prepared.body,
            signature,
        })
    }
}

/// Unsigned public signing request. Crypto custody stays with the caller.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedChangeSet {
    /// Every source, fork, scope and content binding to sign.
    pub envelope: Envelope,
    /// Existing unsigned interchange format, not a second patch language.
    pub body: String,
}

impl PreparedChangeSet {
    /// Validate the content and return the shared canonical message.
    pub fn message(&self) -> Result<Vec<u8>> {
        validate_body(&self.envelope, &self.body)?;
        self.envelope.message()
    }
}
