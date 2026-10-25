//! The applier's side of the scoped replication stream (`local-sync-mode.md`
//! §5, `shared-applier-api.md` rules 1-8): stream sessions, frame sequence,
//! signed coverage, and the freshness challenge.
//!
//! The caller pins the source's stream key and the scope digest with
//! [`Applier::trust_stream`]; neither is ever learned from the stream itself.
//! Without them every signed frame is refused.
//!
//! - **Session.** A signed `session` frame binds a session id to the source,
//!   the scope and the cursor it continues from. A `since` above the cursor
//!   would skip transactions: resume. Opening a session discards every proof
//!   and outstanding challenge of the previous one.
//! - **Sequence.** Each frame's `frame_seq` must be exactly one more than the
//!   last in its session, and each transaction must be newer than the last one
//!   the session sent. Anything else is a transport fault: discard the session
//!   and resume from the cursor. A dropped frame is never inferred from a gap
//!   in transaction ids, which is normal.
//! - **Coverage** says every in-scope transaction up to `W` was sent in this
//!   session. It is persisted with the cursor and never moves backwards. It
//!   proves completeness, never freshness.
//! - **Freshness** comes only from a coverage frame that answers an
//!   outstanding challenge of the current session, consumed on use: the bound
//!   is then `now - t_send` of that challenge, on the caller's monotonic clock.
//!   A heartbeat, a replayed or unsolicited nonce, a new session, a reopened
//!   applier or a clock discontinuity never yields a bound.

use std::collections::{HashMap, HashSet};

use rusqlite::{OptionalExtension, params};
use serde_json::Value as Json;

use super::{Applier, Outcome, Source, digest, refused, source_of};
use crate::error::Result;
use crate::replicate_wire::{Signed, event, verify};
use crate::store::Store;

/// What a reader may say about how current the replica is.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Freshness {
    /// The last source transaction applied, if any.
    pub source_tx: Option<i64>,
    /// Milliseconds since the last frame of any kind; `None` before the first.
    pub last_frame_age_ms: Option<u64>,
    /// The last verified coverage watermark, if the replica follows a stream.
    pub coverage: Option<i64>,
    /// An upper bound on staleness: `now - t_send` of the latest answered
    /// challenge of the open session. `None` is freshness UNKNOWN.
    pub staleness_bound_ms: Option<u64>,
    /// Whether `staleness_bound_ms` holds a bound.
    pub bounded: bool,
}

impl Freshness {
    /// Whether a `bounded=<max_ms>` read may be served.
    #[must_use]
    pub fn within(&self, max_ms: u64) -> bool {
        self.staleness_bound_ms.is_some_and(|b| b <= max_ms)
    }
}

/// The verified coverage watermark, persisted beside the cursor.
pub(super) const COVERAGE_DDL: &str = "CREATE TABLE IF NOT EXISTS replica_coverage (\
     id INTEGER PRIMARY KEY CHECK (id = 1), \
     store_id TEXT NOT NULL, log_epoch TEXT NOT NULL, watermark INTEGER NOT NULL)";

/// What the caller pinned for the stream.
#[derive(Debug, Clone)]
struct Trust {
    public_key_hex: String,
    scope: String,
}

/// The open session.
#[derive(Debug, Clone)]
struct Session {
    id: String,
    frame_seq: u64,
    /// The newest source transaction the session has sent (its `since` at first).
    last_tx: i64,
}

/// Stream state. Only the coverage watermark is persisted; sessions, challenges
/// and proofs are process memory, so a reopened applier holds none.
#[derive(Debug, Default)]
pub(super) struct Stream {
    trust: Option<Trust>,
    session: Option<Session>,
    coverage: Option<i64>,
    /// The source `coverage` was verified from. It anchors a replica whose
    /// scope has no applied transaction yet (coverage before any match).
    covered: Option<Source>,
    /// Outstanding challenges of the current session: nonce -> `t_send_ms`.
    outstanding: HashMap<[u8; 16], u64>,
    /// Every nonce issued in the current session, answered or not.
    issued: HashSet<[u8; 16]>,
    /// The latest proof: `(t_send_ms, W)`.
    proof: Option<(u64, i64)>,
}

impl Stream {
    /// Load persisted coverage. With applied progress it must be the same
    /// source's; without (a scope with no transaction yet) the coverage row
    /// is itself the replica's anchor and cursor.
    pub(super) fn load(store: &Store, source: Option<&Source>) -> Result<Self> {
        store.conn.execute_batch(COVERAGE_DDL)?;
        let row: Option<(Source, i64)> = store
            .conn
            .query_row(
                "SELECT store_id, log_epoch, watermark FROM replica_coverage WHERE id = 1",
                [],
                |r| {
                    Ok((
                        Source {
                            store_id: r.get(0)?,
                            log_epoch: r.get(1)?,
                        },
                        r.get(2)?,
                    ))
                },
            )
            .optional()?;
        let row = row.filter(|(covered, _)| source.is_none_or(|s| s == covered));
        Ok(Self {
            coverage: row.as_ref().map(|(_, w)| *w),
            covered: row.map(|(covered, _)| covered),
            ..Self::default()
        })
    }

    /// The source persisted coverage belongs to, if any.
    pub(super) fn covered(&self) -> Option<&Source> {
        self.covered.as_ref()
    }

    pub(super) fn coverage(&self) -> Option<i64> {
        self.coverage
    }

    /// `now - t_send` of the latest proof while the replica holds its `W`.
    pub(super) fn staleness_bound(&self, held: i64, now_ms: u64) -> Option<u64> {
        let (t_send, watermark) = self.proof?;
        (held >= watermark).then(|| now_ms.saturating_sub(t_send))
    }

    fn forget_proofs(&mut self) {
        self.outstanding.clear();
        self.issued.clear();
        self.proof = None;
    }
}

impl Applier {
    /// Pin the source's stream public key (hex ed25519) and the scope digest
    /// this replica holds. Signed stream frames are refused until this is set.
    pub fn trust_stream(&mut self, public_key_hex: &str, scope_digest: &str) {
        self.stream.trust = Some(Trust {
            public_key_hex: public_key_hex.to_string(),
            scope: scope_digest.to_string(),
        });
    }

    /// What may be said about freshness at `now_ms`, on the caller's monotonic clock.
    #[must_use]
    pub fn freshness(&self, now_ms: u64) -> Freshness {
        let staleness_bound_ms = match self.stopped {
            Some(_) => None,
            None => self.stream.staleness_bound(self.resume_point(), now_ms),
        };
        Freshness {
            source_tx: self.progress.as_ref().map(|p| p.source_tx),
            last_frame_age_ms: self.last_frame_ms.map(|t| now_ms.saturating_sub(t)),
            coverage: self.stream.coverage(),
            bounded: staleness_bound_ms.is_some(),
            staleness_bound_ms,
        }
    }

    /// The last verified coverage watermark, if any.
    #[must_use]
    pub fn coverage(&self) -> Option<i64> {
        self.stream.coverage
    }

    /// The open stream session's id, if any.
    #[must_use]
    pub fn session(&self) -> Option<&str> {
        self.stream.session.as_ref().map(|s| s.id.as_str())
    }

    /// Record a challenge before sending it. `t_send_ms` is read from the SAME
    /// monotonic clock later passed to [`Applier::freshness`].
    ///
    /// # Errors
    /// Why the challenge cannot count: not the open session, or a nonce
    /// already issued in it.
    pub fn challenge(
        &mut self,
        session: &str,
        nonce: [u8; 16],
        t_send_ms: u64,
    ) -> std::result::Result<(), String> {
        if self.session() != Some(session) {
            return Err("not the open stream session".into());
        }
        if !self.stream.issued.insert(nonce) {
            return Err("nonce already issued in this session".into());
        }
        self.stream.outstanding.insert(nonce, t_send_ms);
        Ok(())
    }

    /// The caller's monotonic clock was discontinuous (a suspend was detected):
    /// no earlier challenge time can bound staleness any more.
    pub fn clock_discontinuity(&mut self) {
        self.stream.forget_proofs();
    }

    /// The transport closed the session: its proofs end with it.
    pub fn end_session(&mut self) {
        self.stream.session = None;
        self.stream.forget_proofs();
    }

    pub(super) fn stream_frame(
        &mut self,
        store: &mut Store,
        name: &str,
        data: &str,
    ) -> Result<Outcome> {
        let Ok(frame) = serde_json::from_str::<Json>(data) else {
            return Ok(self.fault());
        };
        if name == event::SESSION {
            return Ok(self.open_session(&frame));
        }
        // Every later frame continues the open session, exactly in order.
        let (Some(session), Some(seq)) = (frame["session"].as_str(), frame["frame_seq"].as_u64())
        else {
            return Ok(self.fault());
        };
        match &self.stream.session {
            Some(s) if s.id == session && seq == s.frame_seq + 1 => {}
            _ => return Ok(self.fault()),
        }
        if let Some(open) = self.stream.session.as_mut() {
            open.frame_seq = seq;
        }
        let outcome = match name {
            event::TX => self.stream_tx(store, &frame),
            event::TX_BEGIN => {
                if self.chunk.is_some() {
                    return Ok(self.fault());
                }
                let Some(tx) = frame["source_tx"].as_i64() else {
                    return Ok(self.fault());
                };
                if !self.newer_than_session(tx) {
                    return Ok(self.fault());
                }
                Ok(self.begin_chunk(data))
            }
            event::TX_PART => Ok(self.chunk_part(data)),
            event::TX_COMMIT => {
                let tx = self.chunk.as_ref().map(|c| c.source_tx);
                let outcome = self.commit_chunk(store, data)?;
                self.sent(tx, &outcome);
                Ok(outcome)
            }
            event::COVERAGE => self.stream_coverage(store, &frame),
            event::CONTROL => self.stream_control(store, &frame),
            _ => Ok(Outcome::Ignored),
        }?;
        // Any recovery (a gap, or a broken or refused chunk) ends the session
        // and its proofs, whichever path found it.
        if matches!(outcome, Outcome::Resume { .. }) {
            return Ok(self.fault());
        }
        Ok(outcome)
    }

    /// A transport fault: drop the session (and any open chunk) and resume.
    fn fault(&mut self) -> Outcome {
        self.chunk = None;
        self.end_session();
        Outcome::Resume {
            from: self.resume_point(),
        }
    }

    fn newer_than_session(&self, tx: i64) -> bool {
        self.stream.session.as_ref().is_some_and(|s| tx > s.last_tx)
    }

    /// Note a transaction the session has now sent (applied, or a checked duplicate).
    fn sent(&mut self, tx: Option<i64>, outcome: &Outcome) {
        if let (Some(tx), Outcome::Applied { .. } | Outcome::Duplicate) = (tx, outcome)
            && let Some(s) = self.stream.session.as_mut()
        {
            s.last_tx = tx;
        }
    }

    /// The pinned scope and the frame's source, for building what it signs.
    fn signing_context(&self, frame: &Json) -> Option<(String, Source)> {
        Some((self.stream.trust.as_ref()?.scope.clone(), source_of(frame)?))
    }

    /// Whether the frame's signature is the pinned key's over `signed`.
    fn signed_by_source(&self, frame: &Json, signed: &Signed<'_>) -> bool {
        let (Some(trust), Some(signature)) = (&self.stream.trust, frame["signature"].as_str())
        else {
            return false;
        };
        verify(&trust.public_key_hex, signed, signature)
    }

    fn open_session(&mut self, frame: &Json) -> Outcome {
        self.chunk = None;
        self.end_session();
        let Some(trust) = self.stream.trust.clone() else {
            return refused("no stream key is pinned for this replica");
        };
        let (Some(id), Some(since), Some(scope)) = (
            frame["session"].as_str(),
            frame["since"].as_i64(),
            frame["scope"].as_str(),
        ) else {
            return refused("session frame is incomplete");
        };
        if frame["frame_seq"].as_u64() != Some(0) {
            return self.fault();
        }
        let Some(source) = source_of(frame) else {
            return refused("session frame does not name its source store and log epoch");
        };
        let signed = Signed::Session {
            session: id,
            store_id: &source.store_id,
            log_epoch: &source.log_epoch,
            scope,
            since,
        };
        if !self.signed_by_source(frame, &signed) {
            return refused("session frame is not signed by the pinned stream key");
        }
        if let Some(outcome) = self.check_source(&source) {
            return outcome;
        }
        if scope != trust.scope {
            return Outcome::Rebootstrap {
                reason: format!("stream scope {scope} is not this replica's {}", trust.scope),
            };
        }
        let from = self.resume_point();
        if since > from {
            return Outcome::Resume { from };
        }
        self.stream.session = Some(Session {
            id: id.to_string(),
            frame_seq: 0,
            last_tx: since,
        });
        Outcome::SessionOpened {
            session: id.to_string(),
        }
    }

    fn stream_tx(&mut self, store: &mut Store, frame: &Json) -> Result<Outcome> {
        if self.chunk.is_some() {
            return Ok(self.fault());
        }
        let Some(tx) = frame["source_tx"].as_i64() else {
            return Ok(self.fault());
        };
        if !self.newer_than_session(tx) {
            return Ok(self.fault());
        }
        let Some(source) = source_of(frame) else {
            return Ok(refused(
                "tx frame does not name its source store and log epoch",
            ));
        };
        if let Some(outcome) = self.check_source(&source) {
            return Ok(outcome);
        }
        let records = frame["records"].as_array().cloned().unwrap_or_default();
        if records.is_empty() || records.iter().any(|r| r["tx"].as_i64() != Some(tx)) {
            return Ok(refused(
                "tx frame records do not all belong to its source tx",
            ));
        }
        let outcome = self.apply_records(store, &source, &records)?;
        self.sent(Some(tx), &outcome);
        Ok(outcome)
    }

    fn stream_coverage(&mut self, store: &Store, frame: &Json) -> Result<Outcome> {
        let session = self.session().unwrap_or_default().to_string();
        let seq = frame["frame_seq"].as_u64().unwrap_or_default();
        let (Some(watermark), Some(nonce)) = (
            frame["watermark"].as_i64(),
            frame["nonce"]
                .as_str()
                .or(frame["nonce"].is_null().then_some("")),
        ) else {
            return Ok(self.fault());
        };
        let Some((scope, source)) = self.signing_context(frame) else {
            return Ok(self.fault());
        };
        let signed = Signed::Coverage {
            session: &session,
            store_id: &source.store_id,
            log_epoch: &source.log_epoch,
            scope: &scope,
            frame_seq: seq,
            watermark,
            nonce,
        };
        if !self.signed_by_source(frame, &signed) {
            // Unsigned or wrongly signed coverage proves nothing.
            return Ok(self.fault());
        }
        if let Some(outcome) = self.check_source(&source) {
            return Ok(outcome);
        }
        // Coverage follows the transactions it covers and never regresses.
        let last_tx = self.stream.session.as_ref().map_or(0, |s| s.last_tx);
        if self.chunk.is_some()
            || watermark < last_tx
            || self.stream.coverage.is_some_and(|c| watermark < c)
        {
            return Ok(self.fault());
        }
        store.conn.execute(
            "INSERT INTO replica_coverage (id, store_id, log_epoch, watermark) VALUES (1, ?1, ?2, ?3) \
             ON CONFLICT(id) DO UPDATE SET store_id = ?1, log_epoch = ?2, watermark = ?3",
            params![source.store_id, source.log_epoch, watermark],
        )?;
        self.stream.coverage = Some(watermark);
        self.stream.covered = Some(source);
        if let Some(s) = self.stream.session.as_mut() {
            s.last_tx = s.last_tx.max(watermark);
        }
        let answered = parse_nonce(nonce).and_then(|n| self.stream.outstanding.remove(&n));
        Ok(match answered {
            Some(t_send_ms) => {
                self.stream.proof = Some((t_send_ms, watermark));
                Outcome::ProofAccepted { t_send_ms }
            }
            None => Outcome::CoverageAdvanced { watermark },
        })
    }

    fn stream_control(&mut self, store: &Store, frame: &Json) -> Result<Outcome> {
        let session = self.session().unwrap_or_default().to_string();
        let seq = frame["frame_seq"].as_u64().unwrap_or_default();
        let (Some(kind), Some(reason)) = (frame["kind"].as_str(), frame["reason"].as_str()) else {
            return Ok(self.fault());
        };
        let Some((scope, source)) = self.signing_context(frame) else {
            return Ok(self.fault());
        };
        let signed = Signed::Control {
            session: &session,
            store_id: &source.store_id,
            log_epoch: &source.log_epoch,
            scope: &scope,
            frame_seq: seq,
            kind,
            reason,
        };
        if !self.signed_by_source(frame, &signed) {
            return Ok(self.fault());
        }
        if let Some(outcome) = self.check_source(&source) {
            return Ok(outcome);
        }
        Ok(match kind {
            "revoked" => {
                self.chunk = None;
                self.end_session();
                let stop = digest::Stop::Revoked {
                    reason: reason.to_string(),
                };
                digest::persist_stop(store, &stop)?;
                let outcome = Outcome::Revoked {
                    reason: reason.to_string(),
                };
                self.stopped = Some(outcome.clone());
                outcome
            }
            "tx_too_large" => {
                self.end_session();
                Outcome::Rebootstrap {
                    reason: format!("the source cannot frame a transaction: {reason}"),
                }
            }
            "epoch_end" => {
                self.end_session();
                Outcome::Rebootstrap {
                    reason: format!("the source ended its log epoch: {reason}"),
                }
            }
            _ => self.fault(),
        })
    }
}

fn parse_nonce(hex_nonce: &str) -> Option<[u8; 16]> {
    hex::decode(hex_nonce).ok()?.try_into().ok()
}

#[cfg(test)]
#[path = "applier_stream_tests.rs"]
mod tests;
