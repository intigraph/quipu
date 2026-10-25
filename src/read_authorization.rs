//! Owner-local read policy and request-owned capabilities.
//!
//! This module never derives authority from a tenant store, imported RDF,
//! provenance headers, or write permission. The policy store must be a separate
//! owner-controlled file, unavailable to tenant handlers. Opening its owner API
//! is trusted local administration, like opening a raw `Store` locally.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Error, Result, Store};

pub mod profile;
pub mod routes;
mod view;
pub use view::ReadView;
#[cfg(test)]
mod tests;

const CONTROL: &str = "urn:quipu:read-policy:control";
const RECORD: &str = "urn:quipu:read-policy:record";
const REVISION: &str = "urn:quipu:read-policy:ReadGrantRevision";

fn revision_iri(grant: &ReadGrant) -> String {
    format!(
        "urn:quipu:read-policy:revision:{:x}:{}",
        Sha256::digest(grant.id.as_bytes()),
        grant.revision
    )
}

fn denied() -> Error {
    Error::PolicyDenied("scoped read denied".into())
}

fn valid_iri(value: &str) -> bool {
    value.len() <= 2048 && oxrdf::NamedNode::new(value).is_ok()
}

/// Read authority is independent for each operation. There is no write variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadOperation {
    Query,
    Metadata,
    Export,
    Replicate,
}

/// One versioned exact grant. Unknown fields/operations are not forward grants.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadGrant {
    pub version: u32,
    pub id: String,
    pub principal: String,
    pub graphs: BTreeSet<String>,
    pub operations: BTreeSet<ReadOperation>,
    pub valid_from: u64,
    pub valid_until: u64,
    pub max_offline_seconds: u64,
    pub issuer: String,
    pub revision: u64,
    pub revoked: bool,
}

impl ReadGrant {
    fn validate(&self, owner: &str) -> Result<()> {
        if self.version != 1
            || !valid_iri(&self.id)
            || !valid_iri(&self.principal)
            || self.issuer != owner
            || self.revision == 0
            || self.valid_from >= self.valid_until
            || self.max_offline_seconds == 0
            || self.max_offline_seconds > 300
            || self.graphs.is_empty()
            || self.graphs.len() > 64
            || self.graphs.iter().any(|g| !valid_iri(g) || g == CONTROL)
            || self.operations.is_empty()
        {
            return Err(denied());
        }
        Ok(())
    }
}

/// Identity created by an authentication adapter, never deserialized from JSON.
#[derive(Debug, Clone)]
pub struct ReadIdentity {
    principal: String,
    credential: String,
    valid_until: u64,
}

impl ReadIdentity {
    /// Verify through the existing named-credential verifier. No shared bearer,
    /// caller principal string or provenance field is accepted by this adapter.
    pub fn authenticate_named(
        registry: &crate::crew_credentials::CredentialRegistry,
        header: Option<&str>,
    ) -> Result<Self> {
        let principal = registry.authenticate(header).ok_or_else(denied)?;
        Ok(Self {
            principal: principal.iri,
            credential: principal.credential_id,
            // Named registry v1 has no credential expiry. The grant still
            // supplies a finite bound; this is not an offline lease.
            valid_until: u64::MAX,
        })
    }

    /// Trusted serving adapter only. The caller must already have verified and
    /// rechecked the enrolled session; declared transaction identity is not proof.
    pub fn from_verified_request(
        identity: &crate::transaction_auth::Identity,
        valid_until: u64,
    ) -> Result<Self> {
        if !matches!(
            identity.auth_class.as_str(),
            "named_bearer" | "attested_session"
        ) || !valid_iri(&identity.principal)
        {
            return Err(denied());
        }
        let credential = identity
            .credential_id
            .clone()
            .filter(|s| !s.is_empty())
            .ok_or_else(denied)?;
        Ok(Self {
            principal: identity.principal.clone(),
            credential,
            valid_until,
        })
    }
}

/// Administrative handle. This private store is never attached to a data store
/// and is never exposed by the HTTP/MCP tenant facade. No tenant/import API is
/// offered; ordinary RDF bearing these predicates does not change this policy.
pub struct OwnerReadPolicy {
    store: Store,
    owner: String,
}

impl OwnerReadPolicy {
    /// Explicit owner-local construction. This does not configure any listener.
    pub fn open(path: &str, owner: &str) -> Result<Self> {
        if !valid_iri(owner) {
            return Err(denied());
        }
        let store = Store::open(path)?;
        store.graph_create(CONTROL)?;
        let policy = Self {
            store,
            owner: owner.into(),
        };
        policy.snapshot()?;
        Ok(policy)
    }

    /// Open existing authority without migrations, registration or creation.
    /// This is the only serving path; an absent/malformed policy never becomes
    /// a newly initialized empty policy or a legacy open-read profile.
    pub fn open_existing(path: &str, owner: &str) -> Result<Self> {
        if !valid_iri(owner) {
            return Err(denied());
        }
        let policy = Self {
            store: Store::open_read_only(path)?,
            owner: owner.into(),
        };
        policy.snapshot()?;
        Ok(policy)
    }

    /// Persist the revision and authorization generation in ONE guarded store
    /// transaction. The committed transaction id is the persistent generation;
    /// there is no independently written counter that could lag a revocation.
    pub fn apply(&mut self, grant: &ReadGrant, provenance: &str) -> Result<u64> {
        grant.validate(&self.owner)?;
        if provenance.is_empty() || provenance.len() > 2048 {
            return Err(denied());
        }
        // Serialize writers before inspecting the previous revision. A second
        // owner handle cannot race a revocation with a stale revision check.
        self.store.conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.apply_in_transaction(grant, provenance)
        }));
        match result {
            Ok(Ok(generation)) => {
                if let Err(error) = self.store.conn.execute_batch("COMMIT") {
                    let _ = self.store.conn.execute_batch("ROLLBACK");
                    self.store.term_cache.borrow_mut().clear_persistent();
                    return Err(error.into());
                }
                Ok(generation)
            }
            Ok(Err(error)) => {
                let _ = self.store.conn.execute_batch("ROLLBACK");
                self.store.term_cache.borrow_mut().clear_persistent();
                Err(error)
            }
            Err(panic) => {
                let _ = self.store.conn.execute_batch("ROLLBACK");
                self.store.term_cache.borrow_mut().clear_persistent();
                std::panic::resume_unwind(panic)
            }
        }
    }

    fn apply_in_transaction(&mut self, grant: &ReadGrant, provenance: &str) -> Result<u64> {
        let previous = self.snapshot()?;
        if previous
            .grants
            .get(&grant.id)
            .is_some_and(|g| grant.revision <= g.revision)
        {
            return Err(denied());
        }
        let graph = self.store.committed_graph_id(CONTROL)?.ok_or_else(denied)?;
        let entity = self.store.intern(&revision_iri(grant))?;
        let datum = crate::store::Datum {
            entity,
            attribute: self.store.intern(RECORD)?,
            value: crate::Value::Str(serde_json::to_string(grant).map_err(|_| denied())?),
            valid_from: crate::time::now_iso(),
            valid_to: None,
            op: crate::types::Op::Assert,
        };
        let typed = crate::store::Datum {
            entity,
            attribute: self.store.intern(crate::namespace::RDF_TYPE)?,
            value: crate::Value::Ref(self.store.intern(REVISION)?),
            ..datum.clone()
        };
        let tx = self.store.transact_to_graph(
            &[datum, typed],
            &crate::time::now_iso(),
            Some(&self.owner),
            Some(provenance),
            graph,
        )?;
        u64::try_from(tx).map_err(|_| denied())
    }

    /// Fresh authoritative snapshot; no last-known-good policy fallback.
    pub fn snapshot(&self) -> Result<ReadPolicy> {
        let read = self
            .store
            .conn
            .is_autocommit()
            .then(|| self.store.conn.unchecked_transaction())
            .transpose()?;
        let generation = u64::try_from(self.store.latest_tx_id()?).map_err(|_| denied())?;
        let graph = self.store.committed_graph_id(CONTROL)?.ok_or_else(denied)?;
        let mut grants: BTreeMap<String, ReadGrant> = BTreeMap::new();
        let mut typed = BTreeSet::new();
        let mut recorded = BTreeSet::new();
        for fact in self.store.current_facts_in_graph(graph)? {
            if self.store.resolve(fact.attribute)? == crate::namespace::RDF_TYPE {
                let crate::Value::Ref(class) = fact.value else {
                    return Err(denied());
                };
                if self.store.resolve(class)? != REVISION {
                    return Err(denied());
                }
                typed.insert(fact.entity);
                continue;
            }
            if self.store.resolve(fact.attribute)? != RECORD {
                return Err(denied());
            }
            let crate::Value::Str(json) = fact.value else {
                return Err(denied());
            };
            let grant: ReadGrant = serde_json::from_str(&json).map_err(|_| denied())?;
            grant.validate(&self.owner)?;
            if self.store.resolve(fact.entity)? != revision_iri(&grant)
                || !recorded.insert(fact.entity)
            {
                return Err(denied());
            }
            match grants.get(&grant.id) {
                Some(old) if old.revision == grant.revision => return Err(denied()),
                Some(old) if old.revision > grant.revision => {}
                _ => {
                    grants.insert(grant.id.clone(), grant);
                }
            }
        }
        if typed != recorded {
            return Err(denied());
        }
        let bytes = serde_json::to_vec(&(generation, &grants)).map_err(|_| denied())?;
        let digest = format!("{:x}", Sha256::digest(bytes));
        if let Some(read) = read {
            read.commit()?;
        }
        Ok(ReadPolicy {
            generation,
            digest,
            grants,
        })
    }
}

/// Immutable qualified policy. Its fields cannot be supplied by a tenant.
pub struct ReadPolicy {
    generation: u64,
    digest: String,
    grants: BTreeMap<String, ReadGrant>,
}

impl ReadPolicy {
    /// The explicit dataset is checked in full BEFORE looking up source graphs.
    /// None selects exactly the graphs granted for this operation. Delegation
    /// is deliberately unsupported; callers cannot submit a principal chain.
    pub fn authorize(
        &self,
        identity: &ReadIdentity,
        operation: ReadOperation,
        dataset: Option<&BTreeSet<String>>,
        now: u64,
    ) -> Result<AuthorizedRead> {
        if now >= identity.valid_until {
            return Err(denied());
        }
        let mut graphs = BTreeSet::new();
        let mut expiry = identity.valid_until;
        for grant in self.grants.values().filter(|g| {
            !g.revoked
                && g.principal == identity.principal
                && g.operations.contains(&operation)
                && g.valid_from <= now
                && now < g.valid_until
        }) {
            graphs.extend(grant.graphs.iter().cloned());
            expiry = expiry.min(grant.valid_until);
        }
        if graphs.is_empty() {
            return Err(denied());
        }
        if let Some(selected) = dataset {
            if selected.is_empty() || !selected.is_subset(&graphs) {
                return Err(denied());
            }
            graphs = selected.clone();
        }
        Ok(AuthorizedRead {
            principal: identity.principal.clone(),
            credential: identity.credential.clone(),
            operation,
            graphs,
            generation: self.generation,
            digest: self.digest.clone(),
            expiry,
            authorized_at: now,
        })
    }
}

/// Immutable authorization; source store/epoch are added when a view is opened.
/// No raw store or connection is obtainable from this capability.
pub struct AuthorizedRead {
    principal: String,
    credential: String,
    operation: ReadOperation,
    graphs: BTreeSet<String>,
    generation: u64,
    digest: String,
    expiry: u64,
    authorized_at: u64,
}

impl AuthorizedRead {
    /// Recheck before buffered publication. Streaming/lease publication is R3.
    pub fn recheck(&self, policy: &ReadPolicy, identity: &ReadIdentity, now: u64) -> Result<()> {
        if self.generation != policy.generation
            || self.digest != policy.digest
            || self.principal != identity.principal
            || self.credential != identity.credential
            || now >= self.expiry
            || now < self.authorized_at
            || now >= identity.valid_until
        {
            return Err(denied());
        }
        Ok(())
    }
}
