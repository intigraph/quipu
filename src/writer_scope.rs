//! Exact, owner-enrolled machine write authority. This is not a read grant or
//! a human attestation. Only the HTTP authentication adapter enters this lane.
use std::cell::RefCell;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Error, Result, Store};
mod audit;
pub(crate) use audit::{Audit, defer_refusal, defer_verdicts};

/// Operations are deliberately closed: other mutation routes are refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteOperation {
    Update,
    Episode,
    Knot,
    Set,
}

impl WriteOperation {
    #[must_use]
    pub fn for_route(route: &str) -> Option<Self> {
        match route {
            "/update" => Some(Self::Update),
            "/episode" => Some(Self::Episode),
            "/knot" => Some(Self::Knot),
            "/set" => Some(Self::Set),
            _ => None,
        }
    }
}

/// A local registry instance, not a caller-provided creation timestamp. Pins
/// change on registry replacement, deletion/recreation AND relabelling.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphInstance {
    pub store_id: String,
    pub graph: String,
    pub generation: String,
}

/// Owner-local policy. A revision is immutable; withdrawal is a newer revoked
/// revision. Policy tables are excluded from shares, RDF and HTTP enrollment.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteGrant {
    pub version: u32,
    pub id: String,
    pub principal: String,
    pub credential_id: String,
    pub instance: GraphInstance,
    pub operations: std::collections::BTreeSet<WriteOperation>,
    pub valid_from: u64,
    pub valid_until: u64,
    pub revision: u64,
    pub revoked: bool,
}

pub(crate) fn denied() -> Error {
    Error::PolicyDenied("scoped writer denied".into())
}

impl WriteGrant {
    /// Principal/graph/validity selection shared with the read-side contract.
    /// Operation, credential, store and graph-instance checks remain separate.
    #[must_use]
    pub fn permits(
        &self,
        principal: &str,
        requested: &std::collections::BTreeSet<String>,
        now: u64,
    ) -> bool {
        let graphs = std::collections::BTreeSet::from([self.instance.graph.clone()]);
        crate::grant_scope::GrantScope {
            principal: &self.principal,
            graphs: &graphs,
            valid_from: self.valid_from,
            valid_until: self.valid_until,
            revoked: self.revoked,
        }
        .permits(principal, requested, now)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.version != 1
            || self.id.is_empty()
            || self.id.len() > 128
            || self.principal.len() > 2048
            || oxrdf::NamedNode::new(&self.principal).is_err()
            || self.credential_id.is_empty()
            || self.credential_id.len() > 128
            || self.operations.is_empty()
            || self.valid_from >= self.valid_until
            || self.valid_until > i64::MAX as u64
            || self.revision == 0
            || self.revision > i64::MAX as u64
            || self.instance.graph.len() > 2048
            || oxrdf::NamedNode::new(&self.instance.graph).is_err()
            || self.instance.store_id.is_empty()
            || self.instance.generation.len() != 64
            || !self
                .instance
                .generation
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
        {
            return Err(denied());
        }
        Ok(())
    }
}

/// The authenticated class is created by the server, never a request header.
#[must_use]
pub fn is_scoped_request() -> bool {
    crate::transaction_auth::current_identity()
        .is_some_and(|i| i.auth_class == "scoped_named_bearer")
}

pub(crate) struct Capability {
    pub principal: String,
    pub credential_id: String,
    pub instances: BTreeMap<i64, GraphInstance>,
    pub expires: u64,
    pub authorized_at: u64,
    pub audit: std::sync::Arc<std::sync::Mutex<Audit>>,
}

thread_local! {
    static CAPABILITY: RefCell<Option<Capability>> = const { RefCell::new(None) };
}

pub(crate) fn scoped<T>(capability: Capability, work: impl FnOnce() -> T) -> T {
    struct Restore(Option<Capability>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CAPABILITY.with(|slot| {
                slot.replace(self.0.take());
            });
        }
    }
    let _restore = Restore(CAPABILITY.with(|slot| slot.replace(Some(capability))));
    work()
}

/// Backstop for every normal graph transaction, including derived/retracted
/// writes. Missing capability never falls back to unattributed authority.
pub(crate) fn check(store: &Store, graph: i64) -> Result<()> {
    if !is_scoped_request() {
        return Ok(());
    }
    let identity = crate::transaction_auth::current_identity().ok_or_else(denied)?;
    CAPABILITY.with(|slot| {
        let slot = slot.borrow();
        let cap = slot.as_ref().ok_or_else(denied)?;
        let now = crate::time::epoch_secs();
        if identity.principal != cap.principal
            || identity.credential_id.as_deref() != Some(&cap.credential_id)
            || now < cap.authorized_at
            || now >= cap.expires
            || store.conn.is_autocommit()
        {
            return Err(denied());
        }
        let expected = cap.instances.get(&graph).ok_or_else(denied)?;
        if store.writer_graph_instance(&expected.graph)? != *expected {
            return Err(denied());
        }
        Ok(())
    })
}
