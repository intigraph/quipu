//! A relay's side of the stream (`shared-applier-api.md` §6): a replica
//! serves its children in its source's transaction numbering, from what its
//! applier wrote, never past the coverage it verified upstream.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{Value as Json, json};

use super::{Frame, Limits, Outcome, Page, Session, StreamRequest, refused};
use crate::applier::map;
use crate::asof_share::{CutError, refuse_frozen, resolve_graphs, scope_digest, scope_lists};
use crate::error::Result;
use crate::signing::SigningIdentity;
use crate::store::Store;
use crate::store::changes::Capture;

/// Local transactions a relay reads beyond a page, so the page's last source
/// transaction is always whole: a source transaction is at most one local
/// transaction per scope graph, and a scope has at most 64.
const RELAY_SLACK: usize = 64;

impl Session {
    /// Open a RELAY session on a replica store (`shared-applier-api.md` §6).
    /// Frames name the replica's source and its transaction numbering. The
    /// child's scope must lie inside `allowed` (the relay's own scope and
    /// context lists), and `identity` is the RELAY's stream key. Coverage never
    /// passes the coverage the relay verified upstream.
    ///
    /// # Errors
    /// [`CutError::Refused`] when the store is not a replica, the scope is not
    /// inside the relay's, the lineage differs, or the cursor is outside what
    /// the relay holds.
    pub fn open_relay(
        store: &Store,
        request: &StreamRequest,
        allowed: (&[String], &[String]),
        id: String,
        identity: Arc<SigningIdentity>,
    ) -> std::result::Result<(Self, Frame), CutError> {
        let (scope, context) = scope_lists(&request.graphs, &request.context_graphs)?;
        // One snapshot for every check, as `open_between` does for a source.
        let read = store
            .conn
            .is_autocommit()
            .then(|| store.conn.unchecked_transaction())
            .transpose()?;
        let held: Vec<&String> = allowed.0.iter().chain(allowed.1).collect();
        if scope.iter().chain(&context).any(|g| !held.contains(&g)) {
            return Err(refused("a relay serves only graphs inside its own scope"));
        }
        let Some((source, source_tx, coverage)) = map::position(store)? else {
            return Err(refused("this store is not a replica, so it cannot relay"));
        };
        if source.store_id != request.expected_store_id {
            return Err(refused("replication source store identity mismatch"));
        }
        if map::ended(store, &source)? {
            return Err(refused(
                "this relay's source ended its log; the relay must rebuild",
            ));
        }
        let held_to = coverage.unwrap_or(source_tx).max(source_tx);
        if request.since > held_to || map::local_cursor(store, &source, request.since)?.is_none() {
            return Err(refused("the cursor is outside what this relay holds"));
        }
        let resolved = resolve_graphs(store, &scope, &context)?;
        refuse_frozen(store, &resolved)?;
        drop(read);
        let graphs = resolved.into_iter().map(|(_, id)| id).collect();
        let session = Self {
            id,
            identity,
            store_id: source.store_id,
            log_epoch: source.log_epoch,
            scope: scope_digest(&scope, &context),
            graphs,
            since: request.since,
            cursor: request.since,
            coverage: None,
            seq: 0,
            ended: false,
            part_max_len: None,
            limits: Limits::default(),
            relay: true,
            upstream_proof: None,
        };
        let frame = session.session_frame();
        Ok((session, frame))
    }

    /// The next page on a relay: its local log read after the local cursor for
    /// the session's source cursor, each record restated in source numbering,
    /// under the same limits counted on local transactions. A local
    /// transaction the applier did not write, or a source that changed, ends
    /// the session; so does one source transaction that cannot fit a page.
    pub(super) fn relay_page(&self, store: &Store, max_txs: usize) -> Result<Outcome> {
        let Some((source, _, coverage)) = map::position(store)? else {
            return Ok(Outcome::EpochEnd);
        };
        // The upstream ended this lineage (the relay's applier recorded the
        // signed epoch_end, or stopped): end the child's session the same way.
        if source.store_id != self.store_id
            || source.log_epoch != self.log_epoch
            || map::ended(store, &source)?
        {
            return Ok(Outcome::EpochEnd);
        }
        // Serve only what upstream coverage has verified: a transaction applied
        // before its coverage frame arrives waits for it.
        let Some(verified) = coverage else {
            return Ok(Outcome::Page(Page {
                by_tx: BTreeMap::new(),
                full: false,
                head: self.cursor,
            }));
        };
        let Some(local) = map::local_cursor(store, &source, self.cursor)? else {
            return Ok(Outcome::EpochEnd);
        };
        let (take, all_in) = match self.budget(store, local, max_txs + RELAY_SLACK)? {
            Ok(taken) => taken,
            Err(reason) => return Ok(Outcome::TooLarge(reason)),
        };
        let page =
            store.changes_in_graphs(local, take.max(1), Capture::OldAndNewValues, &self.graphs)?;
        let mut by_tx: BTreeMap<i64, Vec<Json>> = BTreeMap::new();
        let mut unverified = false;
        for mut record in page.records {
            let local_tx = record["tx"].as_i64().unwrap_or_default();
            let Some(tx) = map::source_tx_of(store, &source, local_tx)? else {
                return Ok(Outcome::EpochEnd);
            };
            if tx <= self.cursor {
                continue;
            }
            if tx > verified {
                unverified = true;
                continue;
            }
            record["tx"] = json!(tx);
            by_tx.entry(tx).or_default().push(record);
        }
        if !all_in && !unverified {
            // The last source transaction may continue past the page. (Past
            // the verified watermark it was already left out, and every
            // transaction before it is whole.)
            let last = by_tx.pop_last();
            if by_tx.is_empty()
                && let Some((tx, _)) = last
            {
                return Ok(Outcome::TooLarge(format!(
                    "source tx {tx} does not fit one relay page under the limits"
                )));
            }
        }
        while by_tx.len() > max_txs {
            by_tx.pop_last();
        }
        // Reading past the verified watermark means every verified transaction
        // after the cursor is in this page.
        let full = (all_in || unverified) && by_tx.len() < max_txs;
        Ok(Outcome::Page(Page {
            by_tx,
            full,
            head: verified,
        }))
    }
}
