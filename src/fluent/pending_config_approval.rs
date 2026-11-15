//! Receiver-scoped approval requires a current separately governed config key.
use super::{Pending, invalid, profile, writer};
use crate::{Result, Store};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const CONTRACT: &str = "quipu-observable-config-acceptance/v1";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Approval {
    contract: String,
    request: String,
    receiver: String,
    profile_sha256: String,
    importer: String,
    verifier: String,
    signature: String,
    #[serde(skip)]
    wire: Option<Vec<u8>>,
}

impl Approval {
    /// Source-only signing helper; signing alone neither enrolls nor trusts a key.
    pub(crate) fn seal(
        store: &Store,
        request: &Pending,
        importer: &str,
        verifier: &str,
        signer: &quipu_signing::SigningIdentity,
    ) -> Result<Self> {
        if importer.len() > 4096 || verifier.len() > 4096 {
            return Err(invalid("approval identity exceeds bound"));
        }
        let mut approval = Self {
            contract: CONTRACT.into(),
            request: request.id(),
            receiver: store.store_id()?,
            profile_sha256: profile(store)?,
            importer: importer.into(),
            verifier: verifier.into(),
            signature: String::new(),
            wire: None,
        };
        approval.signature = signer.sign(&approval.message()?);
        Ok(approval)
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 16_384 {
            return Err(invalid("configuration approval exceeds bound"));
        }
        let mut approval: Self = serde_json::from_slice(bytes)
            .map_err(|_| invalid("malformed configuration approval"))?;
        approval.message()?;
        if approval.signature.len() != 128 {
            return Err(invalid("invalid config approval signature size"));
        }
        approval.wire = Some(bytes.to_vec());
        Ok(approval)
    }

    pub(crate) fn bytes(&self) -> Result<Vec<u8>> {
        match &self.wire {
            Some(bytes) => Ok(bytes.clone()),
            None => serde_json::to_vec(self).map_err(|_| invalid("config approval encoding")),
        }
    }

    fn message(&self) -> Result<Vec<u8>> {
        if self.contract != CONTRACT
            || self.profile_sha256.len() != 64
            || !self.profile_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(invalid("invalid configuration approval contract/profile"));
        }
        for iri in [
            &self.request,
            &self.receiver,
            &self.importer,
            &self.verifier,
        ] {
            if iri.len() > 4096 {
                return Err(invalid("approval identity exceeds bound"));
            }
            writer::iri(iri)?;
        }
        let mut bytes = b"quipu-observable-config-acceptance/v1\0".to_vec();
        bytes.extend(
            serde_json::to_vec(&(
                &self.contract,
                &self.request,
                &self.receiver,
                &self.profile_sha256,
                &self.importer,
                &self.verifier,
            ))
            .map_err(|_| invalid("config approval encoding"))?,
        );
        Ok(bytes)
    }

    pub(crate) fn scope_id(&self) -> Result<String> {
        let mut bytes = b"quipu-observable-config-scope/v1\0".to_vec();
        bytes.extend(self.message()?);
        Ok(format!(
            "urn:quipu:observable-config-scope:v1:{}",
            hex::encode(Sha256::digest(bytes))
        ))
    }

    /// Invoked inside the normal native writer transaction, never at decode.
    pub(super) fn verify(&self, store: &Store, request: &Pending, graph: i64) -> Result<()> {
        use crate::governance::verifier_registry::{self, Scope, Witness};
        let identity = crate::transaction_auth::current_identity()
            .ok_or_else(|| invalid("configuration acceptance needs authentic importer"))?;
        if self.signature.len() != 128
            || self.request != request.id()
            || self.receiver != store.store_id()?
            || self.profile_sha256 != profile(store)?
            || self.importer != identity.principal
            || identity.auth_class.is_empty()
            || request.registration().observable.graph != store.graph_iri_of(graph)
        {
            return Err(invalid(
                "configuration approval receiver/request/profile/importer mismatch",
            ));
        }
        let scope = self.scope_id()?;
        let registrations = verifier_registry::keyed_registrations(
            store,
            &self.verifier,
            Some(&scope),
            &Witness::now(),
            Scope::Governed,
        )?;
        if registrations.len() != 1
            || !quipu_signing::verify_hex(&registrations[0].key, &self.message()?, &self.signature)
        {
            return Err(invalid(
                "current governed configuration approval missing or invalid",
            ));
        }
        Ok(())
    }
}
