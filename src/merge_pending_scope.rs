//! Bounded current-slot snapshots for the common pending ledger.
//! This is not a declared-base/history-retention or writer admission proof.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::merge_lists::Slot;
use crate::merge_session::{Path, Request};
use crate::resolver_contract::Slot as ReadSlot;
use crate::store::Store;
use crate::store::slot_read_batch::BatchReadLimits;
use crate::write_base::{LegacyBasePolicy, WriterSnapshot, prepare_slot_reads};
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Scope {
    pub schema: String,
    pub graph: String,
    pub valid_at: String,
    pub slots: Vec<ReadSlot>,
}

pub(super) struct Snapshot {
    pub graph: String,
    pub guards: BTreeMap<Slot, String>,
}

fn invalid() -> Error {
    Error::InvalidValue("partial merge: unsupported or incomplete bounded scope".into())
}

impl Scope {
    fn keys(&self) -> Result<BTreeSet<Slot>> {
        if self.schema != "quipu-writer-read-scope/v1"
            || self.graph != crate::schema::ROOT_GRAPH_IRI
            || self.slots.len() > 256
            || self
                .slots
                .iter()
                .try_fold(0usize, |size, slot| {
                    size.checked_add(slot.subject.len())?
                        .checked_add(slot.predicate.len())
                })
                .is_none_or(|size| size > 65536)
            || crate::time::normalize_rfc3339_utc(&self.valid_at).as_deref()
                != Some(self.valid_at.as_str())
        {
            return Err(invalid());
        }
        let mut keys = BTreeSet::new();
        for slot in &self.slots {
            let subject = slot.subject.parse::<oxrdf::Term>().map_err(|_| invalid())?;
            if !matches!(subject, oxrdf::Term::NamedNode(_))
                || subject.to_string() != slot.subject
                || oxrdf::NamedNode::new(slot.predicate.clone()).is_err()
                || !keys.insert((slot.subject.clone(), slot.predicate.clone()))
            {
                return Err(invalid());
            }
        }
        Ok(keys)
    }

    pub(crate) fn validate_request(&self, request: &Request) -> Result<()> {
        let keys = self.keys()?;
        // No list-cell closure or non-ROOT persistence is admitted by this slice.
        if request.path != Path::Share
            || !request.profile.lists.is_empty()
            || request
                .profile
                .units
                .iter()
                .any(|unit| !unit.is_subset(&keys))
            || request
                .active_slots
                .as_ref()
                .is_some_and(|active| !active.is_subset(&keys))
            || request
                .base
                .len()
                .saturating_add(request.ours.len())
                .saturating_add(request.theirs.len())
                .saturating_add(request.shapes.len())
                > 8 * 1024 * 1024
        {
            return Err(invalid());
        }
        for text in [&request.base, &request.ours, &request.theirs] {
            let graph = crate::share_merge::parse_graph(text, "bounded pending input")?;
            if graph.iter().any(|triple| {
                !keys.contains(&(
                    triple.subject.to_string(),
                    triple.predicate.as_str().to_owned(),
                ))
            }) {
                return Err(invalid());
            }
        }
        Ok(())
    }

    /// Current-cut evidence only. The caller still owns reservation/admission.
    /// Each snapshot charges both physical cuts, including invisible history.
    pub(super) fn snapshot(&self, store: &Store) -> Result<Snapshot> {
        self.keys()?;
        let head_tx = u64::try_from(store.latest_tx_id()?).map_err(|_| invalid())?;
        let writer = WriterSnapshot {
            store_id: store.store_id()?,
            log_epoch: store.log_epoch()?,
            graph: self.graph.clone(),
            oldest_complete_tx: head_tx,
            head_tx,
        };
        let prepared = prepare_slot_reads(
            None,
            &writer,
            LegacyBasePolicy::UseCurrentAndFlag,
            &self.slots,
            256,
            &self.valid_at,
        )?;
        let views = store.read_prepared_slot_batch(
            &prepared,
            BatchReadLimits {
                claims_per_read: 4096,
                bytes_per_read: 4 * 1024 * 1024,
                total_claims: 65536,
                total_bytes: 16 * 1024 * 1024,
            },
        )?;
        let mut graph = crate::share_merge::Graph::new();
        let mut guards = BTreeMap::new();
        for view in views.slots {
            let subject = view
                .slot
                .subject
                .parse::<oxrdf::Term>()
                .map_err(|_| invalid())?;
            let oxrdf::Term::NamedNode(subject) = subject else {
                return Err(invalid());
            };
            let predicate =
                oxrdf::NamedNode::new(view.slot.predicate.clone()).map_err(|_| invalid())?;
            for fact in view.current {
                graph.insert(oxrdf::Triple::new(
                    subject.clone(),
                    predicate.clone(),
                    crate::rdf::value_to_term(store, &fact.value)?,
                ));
            }
            let key = (view.slot.subject, view.slot.predicate);
            // Scope, valid time, store/epoch and ALL bounded physical claims bind
            // the guard. Equal visible values cannot hide different provenance.
            let bytes = serde_json::to_vec(&(
                "quipu-scoped-guard/v1",
                &writer.store_id,
                &writer.log_epoch,
                self,
                &key,
                view.current_claims_digest,
            ))
            .map_err(|e| Error::Serialization(e.to_string()))?;
            guards.insert(key, crate::share::sha256(&bytes));
        }
        Ok(Snapshot {
            graph: crate::merge_session::render(&graph),
            guards,
        })
    }
}
