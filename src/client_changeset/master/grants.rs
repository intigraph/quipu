//! Owner-local exact write grants. No transport, RDF issuer or enrollment path.
use super::refuse;
use crate::{Result, Store};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Explicit master-local authority, distinct from signature artifact attestation.
/// Real issuance is a separate trust-root operation; fixtures do not enroll it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientWriteGrant {
    pub id: String,
    pub revision: u64,
    pub key_id: String,
    pub signer: String,
    pub public_key: String,
    pub store_id: String,
    pub log_epoch: String,
    pub graph: String,
    pub predicates: BTreeSet<String>,
    pub valid_from: u64,
    pub valid_until: u64,
    pub revoked: bool,
}

impl ClientWriteGrant {
    fn validate(&self) -> Result<()> {
        if self.id.is_empty()
            || self.id.len() > 128
            || self.revision == 0
            || self.revision > i64::MAX as u64
            || self.graph != "ROOT"
            || self.store_id.is_empty()
            || self.log_epoch.is_empty()
            || self.signer.is_empty()
            || self.valid_from >= self.valid_until
            || self.valid_until > i64::MAX as u64
            || self.predicates.is_empty()
            || self.predicates.len() > 64
            || oxrdf::NamedNode::new(&self.key_id).is_err()
            || self
                .predicates
                .iter()
                .any(|p| oxrdf::NamedNode::new(p).is_err())
            || self.public_key.len() != 64
            || !self.public_key.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(refuse("invalid exact client write grant"));
        }
        Ok(())
    }
}

impl Store {
    /// Trusted local owner API ONLY. Not callable over RDF/HTTP/MCP and never
    /// from a scoped request or nested admission. Revocations use newer revisions.
    pub fn apply_client_write_grant(&mut self, grant: &ClientWriteGrant) -> Result<()> {
        self.ensure_ownership_writable()?;
        if !self.conn.is_autocommit() || crate::writer_scope::is_scoped_request() {
            return Err(refuse("client grant enrollment requires local owner"));
        }
        grant.validate()?;
        crate::share_merge::core::atomic(self, |store| {
            if grant.store_id != store.store_id()? || grant.log_epoch != store.log_epoch()? {
                return Err(refuse("grant lineage mismatch"));
            }
            let previous: Option<i64> = store
                .conn
                .query_row(
                    "SELECT revision FROM client_changeset_grants WHERE id=?1",
                    [&grant.id],
                    |r| r.get(0),
                )
                .optional()?;
            if previous.is_some_and(|r| r >= grant.revision as i64) {
                return Err(refuse("grant revision is not newer"));
            }
            store.conn.execute(
                "INSERT INTO client_changeset_grants VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE \
                 SET revision=excluded.revision,payload=excluded.payload",
                rusqlite::params![
                    grant.id,
                    grant.revision as i64,
                    serde_json::to_string(grant)
                        .map_err(|e| crate::Error::Serialization(e.to_string()))?
                ],
            )?;
            Ok(())
        })
    }
}

#[cfg(feature = "shacl")]
pub(super) fn authorize(
    store: &Store,
    proposal: &super::SignedChangeSet,
    changes: &crate::changeset::ChangeSet,
) -> Result<String> {
    use crate::governance::verifier_registry::{Scope, Witness, keyed_registrations};
    let e = &proposal.envelope;
    let now = crate::time::epoch_secs();
    let touched: BTreeSet<_> = changes
        .deletes
        .iter()
        .chain(&changes.inserts)
        .map(|s| s.to_quad().map(|q| q.predicate.as_str().to_string()))
        .collect::<Result<_>>()?;
    let registrations =
        keyed_registrations(store, &e.signer, None, &Witness::now(), Scope::Governed)?;
    if store
        .conn
        .query_row("SELECT COUNT(*) FROM client_changeset_grants", [], |r| {
            r.get::<_, i64>(0)
        })?
        > 128
    {
        return Err(refuse("client grant read budget exceeded"));
    }
    let mut statement = store
        .conn
        .prepare("SELECT payload FROM client_changeset_grants")?;
    let mut accepted = BTreeSet::new();
    for value in statement.query_map([], |r| r.get::<_, String>(0))? {
        let grant: ClientWriteGrant =
            serde_json::from_str(&value?).map_err(|_| refuse("client write grant unreadable"))?;
        grant.validate()?;
        if !grant.revoked
            && grant.valid_from <= now
            && now < grant.valid_until
            && grant.key_id == e.key_id
            && grant.signer == e.signer
            && grant.graph == e.graph
            && grant.store_id == store.store_id()?
            && grant.log_epoch == store.log_epoch()?
            && touched.is_subset(&grant.predicates)
            && registrations
                .iter()
                .any(|r| r.registration == grant.key_id && r.key == grant.public_key)
        {
            accepted.insert(grant.public_key);
        }
    }
    if accepted.len() != 1 {
        return Err(refuse("current exact client authority unavailable"));
    }
    Ok(accepted.into_iter().next().expect("exactly one"))
}
