//! The source's side of the scoped replication stream (`local-sync-mode.md`
//! §5 and §6): the frames of one stream session, built from the store with no
//! I/O of its own. The server route (`server/replicate.rs`) drives it; tests
//! drive it directly into the applier.
//!
//! - **Scope.** The same graphs an as-of share would serve, resolved by the
//!   same rules (`asof_share::scope_lists` and `resolve_graphs`), and the same
//!   scope digest, so a replica bootstrapped from a share tails exactly it.
//! - **Snapshot.** Each poll reads its page and the head in ONE read
//!   transaction. Coverage `W` is that head when the page held every
//!   transaction after the cursor, and the page's last transaction when it was
//!   cut short: every in-scope transaction up to `W` is then in this session.
//! - **Challenge.** A nonce is answered only by a poll that read after the
//!   challenge arrived and reached the head, so its `W` was observed after
//!   receipt. A short page leaves the nonce for the next poll.
//! - **Open.** The opening epoch and the scope checks are one snapshot, epoch
//!   first, so a change committed during the open is caught by `epoch_end`.
//! - **Epoch.** A poll that finds a different store or log epoch than the
//!   session opened on ends the session with a signed `epoch_end`.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{Value as Json, json};

use crate::asof_share::{CutError, refuse_frozen, resolve_graphs, scope_digest, scope_lists};
use crate::error::{Error, Result};
use crate::feed_parts::{PartBuilder, RecordTooLarge, fits_frame_len};
use crate::replicate_wire::{Signed, event, message};
use crate::signing::SigningIdentity;
use crate::store::Store;
use crate::store::changes::Capture;

/// What a follower asks the stream for.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamRequest {
    /// The scope graphs (registered, committed), as for an as-of share.
    pub graphs: Vec<String>,
    /// Context graphs the scope needs; their records are streamed too.
    #[serde(default)]
    pub context_graphs: Vec<String>,
    /// The follower's cursor: the stream continues after it.
    pub since: i64,
    /// The source lineage the follower holds; a mismatch is a refusal.
    pub expected_store_id: String,
}

/// One frame, as the transport sends it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The SSE event name (`replicate_wire::event`).
    pub event: &'static str,
    /// The frame's data, exactly as sent.
    pub data: String,
}

/// The source's transfer limits (local-sync-mode.md §6). A transaction over
/// `tx_max_rows` or `tx_max_bytes` in scope ends the session with
/// `tx_too_large` and the cursor unchanged; recovery is a re-bootstrap from an
/// as-of share at or after it. A poll stops adding transactions once the page
/// would pass `page_max_rows` or `page_max_bytes` (one transaction that
/// fits the transaction limits is always taken).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Most in-scope records one transaction may carry.
    pub tx_max_rows: u64,
    /// Most encoded bytes one transaction's in-scope records may take, as an
    /// upper bound counted before they are built (`Store::tx_encoded_bound`).
    pub tx_max_bytes: u64,
    /// Rows one poll reads before it stops adding transactions.
    pub page_max_rows: u64,
    /// Encoded bytes (the same bound) one poll reads before it stops.
    pub page_max_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            tx_max_rows: 100_000,
            tx_max_bytes: 64 << 20,
            page_max_rows: 10_000,
            page_max_bytes: 16 << 20,
        }
    }
}

/// One stream session at the source.
pub struct Session {
    id: String,
    identity: Arc<SigningIdentity>,
    store_id: String,
    log_epoch: String,
    local_epoch: String,
    ownership: crate::store::OwnershipState,
    scope: String,
    graphs: Vec<i64>,
    since: i64,
    cursor: i64,
    coverage: Option<i64>,
    seq: u64,
    ended: bool,
    part_max_len: Option<usize>,
    limits: Limits,
    /// A relay: the store is a replica, served in its source's numbering.
    relay: bool,
    /// A relay's fresh upstream proof (`W_up`), not yet spent on a nonce.
    upstream_proof: Option<i64>,
}

impl Session {
    /// Open a session on `store` for `request`, returning it and its signed
    /// opening frame. `id` must be unpredictable (128 random bits, hex).
    ///
    /// # Errors
    /// [`CutError::Refused`] for a scope the source does not serve, a lineage
    /// mismatch or a cursor ahead of the source.
    pub fn open(
        store: &Store,
        request: &StreamRequest,
        id: String,
        identity: Arc<SigningIdentity>,
    ) -> std::result::Result<(Self, Frame), CutError> {
        Self::open_between(store, request, id, identity, || ())
    }

    /// [`Session::open`], running `between` after the epoch is read and before
    /// the scope is checked (tests interleave a concurrent write there).
    ///
    /// The epoch and every check are read in ONE read transaction, epoch
    /// first: a freeze or other history change that commits during the open
    /// either precedes the whole snapshot (and is refused or served in its new
    /// epoch) or follows it, in which case the session carries the OLD epoch
    /// and its first poll ends it with `epoch_end`. It can never open on the
    /// new epoch with a scope checked before the change.
    fn open_between(
        store: &Store,
        request: &StreamRequest,
        id: String,
        identity: Arc<SigningIdentity>,
        between: impl FnOnce(),
    ) -> std::result::Result<(Self, Frame), CutError> {
        let (scope, context) = scope_lists(&request.graphs, &request.context_graphs)?;
        let read = store
            .conn
            .is_autocommit()
            .then(|| store.conn.unchecked_transaction())
            .transpose()?;
        let log_epoch = store.log_epoch()?;
        let ownership = store.ownership_state()?;
        between();
        let store_id = store.store_id()?;
        if store_id != request.expected_store_id {
            return Err(refused("replication source store identity mismatch"));
        }
        if request.since < 0 || request.since > store.latest_tx_id()? {
            return Err(refused("the cursor is not a transaction of this source"));
        }
        let resolved = resolve_graphs(store, &scope, &context)?;
        refuse_frozen(store, &resolved)?;
        drop(read);
        let graphs = resolved.into_iter().map(|(_, id)| id).collect();
        let session = Self {
            id,
            identity,
            store_id,
            local_epoch: log_epoch.clone(),
            ownership,
            log_epoch,
            scope: scope_digest(&scope, &context),
            graphs,
            since: request.since,
            cursor: request.since,
            coverage: None,
            seq: 0,
            ended: false,
            part_max_len: None,
            limits: Limits::default(),
            relay: false,
            upstream_proof: None,
        };
        let frame = session.session_frame();
        Ok((session, frame))
    }

    /// Placement admission captured for this session; INACTIVE is legacy
    /// compatibility, never an ownership-authorized capability.
    #[must_use]
    pub fn ownership_state(&self) -> crate::store::OwnershipState {
        self.ownership
    }

    /// The session id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The scope digest the session serves.
    #[must_use]
    pub fn scope(&self) -> &str {
        &self.scope
    }

    /// Whether the session has ended (its epoch ended, or a transaction cannot
    /// be framed); the transport closes it after the last frame.
    #[must_use]
    pub fn ended(&self) -> bool {
        self.ended
    }

    /// A relay's upstream proof: the relay's own challenge, sent AFTER the
    /// child's challenge arrived, was accepted upstream at coverage
    /// `watermark` (§6, "Freshness, per hop"). The next child nonce may then
    /// be answered once the relay holds that coverage; one proof answers one
    /// nonce. A relay answers no nonce without one.
    pub fn upstream_proof(&mut self, watermark: i64) {
        self.upstream_proof = Some(watermark);
    }

    /// Split parts at `max_len` bytes instead of the frame budget (tests).
    #[must_use]
    pub fn with_part_max_len(mut self, max_len: usize) -> Self {
        self.part_max_len = Some(max_len);
        self
    }

    /// Serve under `limits` instead of the defaults.
    #[must_use]
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Read at most `max_txs` new transactions and the coverage they reach.
    /// `nonce` is an unanswered challenge: it is answered if this poll reaches
    /// the head, and returned unanswered (`false`) otherwise. On a relay it is
    /// answered only after [`Self::upstream_proof`]. A poll that adds nothing
    /// sends nothing unless it answers a nonce.
    ///
    /// # Errors
    /// A store error; nothing was sent and the session may be polled again.
    pub fn poll(
        &mut self,
        store: &Store,
        max_txs: usize,
        nonce: Option<[u8; 16]>,
    ) -> Result<(Vec<Frame>, bool)> {
        if self.ended {
            return Ok((Vec::new(), false));
        }
        let read = store
            .conn
            .is_autocommit()
            .then(|| store.conn.unchecked_transaction())
            .transpose()?;
        let local_epoch = store.log_epoch()?;
        store.ownership_state()?;
        let outcome = if local_epoch != self.local_epoch {
            Outcome::EpochEnd
        } else if self.relay {
            self.relay_page(store, max_txs.max(1))?
        } else {
            self.source_page(store, max_txs.max(1))?
        };
        drop(read);
        let mut frames = Vec::new();
        let Page { by_tx, full, head } = match outcome {
            Outcome::Page(page) => page,
            Outcome::EpochEnd => {
                self.ended = true;
                frames.push(self.control("epoch_end", "the source log epoch changed"));
                return Ok((frames, false));
            }
            Outcome::TooLarge(reason) => {
                self.ended = true;
                frames.push(self.control("tx_too_large", &reason));
                return Ok((frames, false));
            }
        };
        for (tx, records) in by_tx {
            if !self.tx_frames(tx, &records, &mut frames) {
                self.ended = true;
                frames.push(self.control(
                    "tx_too_large",
                    &format!("source tx {tx} has a record larger than a frame"),
                ));
                return Ok((frames, false));
            }
            self.cursor = tx;
        }
        let watermark = if full {
            head.max(self.cursor)
        } else {
            self.cursor
        };
        let proven = !self.relay || self.upstream_proof.is_some_and(|w| watermark >= w);
        let answer = nonce.filter(|_| full && proven);
        if answer.is_some() && self.relay {
            self.upstream_proof = None;
        }
        if answer.is_some() || self.coverage.is_none_or(|c| watermark > c) {
            frames.push(self.coverage_frame(watermark, answer));
        }
        Ok((frames, answer.is_some()))
    }

    /// How many transactions after `after` (in the store's own numbering) one
    /// page takes under the limits (local-sync-mode.md §6), and whether those
    /// are every transaction after it. Sizes are counted from SQL lengths, so
    /// an oversized transaction is refused, and a page is cut at its budget,
    /// before any record is built. `Err` is the `tx_too_large` reason, when
    /// the first transaction alone is over the transaction limits.
    fn budget(
        &self,
        store: &Store,
        after: i64,
        max_txs: usize,
    ) -> Result<std::result::Result<(usize, bool), String>> {
        let counts = store.tx_rows_in_graphs(after, max_txs, &self.graphs)?;
        let limits = self.limits;
        let (mut take, mut rows, mut bytes) = (0, 0, 0);
        for &(tx, tx_rows) in &counts {
            // Rows first: a transaction over the row limit is refused without
            // reading its records at all.
            let tx_bytes = if tx_rows > limits.tx_max_rows {
                None
            } else {
                Some(store.tx_encoded_bound(tx, &self.graphs, limits.tx_max_bytes)?)
            };
            if tx_bytes.is_none_or(|b| b > limits.tx_max_bytes) {
                if take == 0 {
                    let size = tx_bytes.map_or_else(
                        || format!("{tx_rows} rows"),
                        |b| format!("over {b} encoded bytes"),
                    );
                    return Ok(Err(format!(
                        "source tx {tx} has {size} in scope; the limit is {} rows and {} bytes",
                        limits.tx_max_rows, limits.tx_max_bytes
                    )));
                }
                break;
            }
            let tx_bytes = tx_bytes.unwrap_or_default();
            if take > 0
                && (rows + tx_rows > limits.page_max_rows
                    || bytes + tx_bytes > limits.page_max_bytes)
            {
                break;
            }
            rows += tx_rows;
            bytes += tx_bytes;
            take += 1;
        }
        Ok(Ok((take, counts.len() < max_txs && take == counts.len())))
    }

    /// The next page of whole source transactions on a source store.
    fn source_page(&self, store: &Store, max_txs: usize) -> Result<Outcome> {
        if store.store_id()? != self.store_id || store.log_epoch()? != self.log_epoch {
            return Ok(Outcome::EpochEnd);
        }
        let (take, full) = match self.budget(store, self.cursor, max_txs)? {
            Ok(taken) => taken,
            Err(reason) => return Ok(Outcome::TooLarge(reason)),
        };
        let page = store.changes_in_graphs(
            self.cursor,
            take.max(1),
            Capture::OldAndNewValues,
            &self.graphs,
        )?;
        let mut by_tx: BTreeMap<i64, Vec<Json>> = BTreeMap::new();
        for record in page.records {
            by_tx
                .entry(record["tx"].as_i64().unwrap_or_default())
                .or_default()
                .push(record);
        }
        Ok(Outcome::Page(Page {
            by_tx,
            full,
            head: page.watermark_tx,
        }))
    }

    /// A signed heartbeat: the last coverage again, so an idle follower keeps
    /// proving completeness. Never a freshness proof.
    #[must_use]
    pub fn heartbeat(&mut self) -> Frame {
        let watermark = self.coverage.unwrap_or(self.cursor);
        self.coverage_frame(watermark, None)
    }

    fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    fn source(&self) -> Json {
        json!({"store_id": self.store_id, "log_epoch": self.log_epoch})
    }

    fn session_frame(&self) -> Frame {
        let signature = self.identity.sign(&message(&Signed::Session {
            session: &self.id,
            store_id: &self.store_id,
            log_epoch: &self.log_epoch,
            scope: &self.scope,
            since: self.since,
        }));
        let data = json!({"session": self.id, "frame_seq": 0, "source": self.source(),
                          "scope": self.scope, "since": self.since, "signature": signature});
        Frame {
            event: event::SESSION,
            data: data.to_string(),
        }
    }

    /// One transaction's frames: one `tx` frame, or begin/parts/commit when
    /// it does not fit. `false` when a single record exceeds a part.
    fn tx_frames(&mut self, tx: i64, records: &[Json], out: &mut Vec<Frame>) -> bool {
        let seq = self.seq + 1;
        let data = json!({"session": self.id, "frame_seq": seq, "source": self.source(),
                          "source_tx": tx, "records": records})
        .to_string();
        if self.part_max_len.is_none() && fits_frame_len(data.len()) {
            self.seq = seq;
            out.push(Frame {
                event: event::TX,
                data,
            });
            return true;
        }
        let builder = match self.part_max_len {
            Some(max_len) => PartBuilder::with_max_len(tx, max_len),
            None => PartBuilder::new(tx),
        };
        let mut builder = builder.in_session(&self.id, seq + 1);
        let mut parts = Vec::new();
        for record in records {
            match builder.push(record) {
                Ok(closed) => parts.extend(closed),
                Err(RecordTooLarge) => return false,
            }
        }
        let (last, summary) = builder.finish();
        parts.extend(last);
        let begin = self.next_seq();
        out.push(Frame {
            event: event::TX_BEGIN,
            data: json!({"session": self.id, "frame_seq": begin, "source_tx": tx,
                         "parts": summary.parts, "records": summary.records,
                         "digest": summary.digest, "since": self.since})
            .to_string(),
        });
        for part in parts {
            self.next_seq();
            out.push(Frame {
                event: event::TX_PART,
                data: part,
            });
        }
        let commit = self.next_seq();
        out.push(Frame {
            event: event::TX_COMMIT,
            data: json!({"session": self.id, "frame_seq": commit, "source_tx": tx,
                         "digest": summary.digest, "since": self.since,
                         "source": self.source()})
            .to_string(),
        });
        true
    }

    fn coverage_frame(&mut self, watermark: i64, nonce: Option<[u8; 16]>) -> Frame {
        let seq = self.next_seq();
        let nonce_hex = nonce.map(hex::encode).unwrap_or_default();
        let signature = self.identity.sign(&message(&Signed::Coverage {
            session: &self.id,
            store_id: &self.store_id,
            log_epoch: &self.log_epoch,
            scope: &self.scope,
            frame_seq: seq,
            watermark,
            nonce: &nonce_hex,
        }));
        self.coverage = Some(watermark);
        let nonce = if nonce_hex.is_empty() {
            Json::Null
        } else {
            json!(nonce_hex)
        };
        Frame {
            event: event::COVERAGE,
            data: json!({"session": self.id, "frame_seq": seq, "source": self.source(),
                         "watermark": watermark, "nonce": nonce, "signature": signature})
            .to_string(),
        }
    }

    fn control(&mut self, kind: &str, reason: &str) -> Frame {
        let seq = self.next_seq();
        let signature = self.identity.sign(&message(&Signed::Control {
            session: &self.id,
            store_id: &self.store_id,
            log_epoch: &self.log_epoch,
            scope: &self.scope,
            frame_seq: seq,
            kind,
            reason,
        }));
        Frame {
            event: event::CONTROL,
            data: json!({"session": self.id, "frame_seq": seq, "source": self.source(),
                         "kind": kind, "reason": reason, "signature": signature})
            .to_string(),
        }
    }
}

/// What one poll read: a page, or the reason the session ends.
enum Outcome {
    Page(Page),
    EpochEnd,
    TooLarge(String),
}

/// One page of whole source transactions, and whether it reached the head.
struct Page {
    by_tx: BTreeMap<i64, Vec<Json>>,
    full: bool,
    /// The coverage the page may claim when it is full.
    head: i64,
}

fn refused(message: &str) -> CutError {
    CutError::Refused(Error::InvalidValue(message.into()))
}

#[path = "replicate_source_relay.rs"]
mod relay;

#[cfg(test)]
#[path = "replicate_source_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "replicate_source_limits_tests.rs"]
mod limits_tests;

#[cfg(test)]
#[path = "replicate_source_ownership_tests.rs"]
mod ownership_tests;
