//! `quipu follow`: keep a read-only follower current from a primary's
//! committed change feed (aegis-tnh), so reads survive the primary's host
//! going away. `quipu-server --read-only` serves the follower database
//! (`server.replica`); this module is its only writer.
//!
//! - **Bootstrap.** Paced `GET /changes` pages from the applier's cursor
//!   (transaction 0 for a new follower), with `capture=old_and_new_values` so
//!   retracts carry the value they end. Reads only: no writer credential.
//! - **Tail.** `GET /changes/stream` from the cursor. The stream is
//!   event-driven, so an idle primary costs one heartbeat every 15 s.
//! - **One writer, whole transactions.** Every frame goes through the shared
//!   applier (`crate::applier`), so the follower is always at a source
//!   transaction boundary and its cursor survives a restart.
//! - **Never splice.** A changed source (restore, clone, removed history), a
//!   digest mismatch, a refused record or a revocation ends the run with
//!   [`Halt::Rebuild`]: an operator rebuilds the follower. A broken or dropped frame
//!   only reconnects from the cursor.
//!
//! - **Bounded pages.** A page body is read up to [`PAGE_MAX_BYTES`]. A larger
//!   page is asked again with half the transactions, and a single transaction
//!   over the cap ends the run with [`Halt::TooLarge`]: retrying the same
//!   request could never succeed.
//!
//! Graphs frozen into packs are not in the change feed, so a follower does not
//! hold them.

use std::io::BufRead;

use crate::applier::{Applier, Outcome};
use crate::store::Store;

/// The client label every follower request carries (`X-Quipu-Client`).
pub const CLIENT: &str = "replica-follower";

/// The most bytes a follower reads for one `/changes` page.
pub const PAGE_MAX_BYTES: u64 = 256 << 20;

/// A `/changes` page over [`PAGE_MAX_BYTES`], returned by [`Feed::page`] inside
/// an `std::io::Error`, so the bootstrap asks for fewer transactions instead
/// of retrying the same request.
#[derive(Debug)]
pub struct PageTooLarge;

impl std::fmt::Display for PageTooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the page is over the {} MiB page cap",
            PAGE_MAX_BYTES >> 20
        )
    }
}

impl std::error::Error for PageTooLarge {}

fn too_large(error: &std::io::Error) -> bool {
    matches!(error.get_ref(), Some(e) if e.is::<PageTooLarge>())
}

/// Where the frames come from. [`crate::follow_http::Http`] is the primary over
/// HTTP; tests drive a [`Store`] directly.
pub trait Feed {
    /// One `/changes` page after `since`, at most `limit` transactions, with
    /// old and new values: the response body, exactly as served.
    ///
    /// # Errors
    /// Any transport failure; the caller retries from its cursor.
    fn page(&mut self, since: i64, limit: usize) -> std::io::Result<String>;

    /// The SSE change stream from `since`, as a reader of its lines.
    ///
    /// # Errors
    /// Any transport failure; the caller retries from its cursor.
    fn stream(&mut self, since: i64) -> std::io::Result<Box<dyn BufRead>>;
}

/// Why a call returned before it finished.
#[derive(Debug)]
pub enum Halt {
    /// The primary could not be reached or the connection broke: retry from
    /// the cursor, which is durable.
    Transport(std::io::Error),
    /// The local store failed; nothing past the cursor was written.
    Store(crate::error::Error),
    /// The applier refused to continue: rebuild the follower. The text says
    /// why. Nothing is applied after it.
    Rebuild(String),
    /// The caller's frame budget ran out; resume later.
    Budget,
    /// One source transaction is over [`PAGE_MAX_BYTES`]: no page can carry
    /// it, so retrying cannot help. The text names where.
    TooLarge(String),
}

impl From<crate::error::Error> for Halt {
    fn from(error: crate::error::Error) -> Self {
        Self::Store(error)
    }
}

/// How hard a follower may lean on its primary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pace {
    /// Transactions per bootstrap page.
    pub page_txs: usize,
    /// Pause between bootstrap pages, in milliseconds.
    pub page_gap_ms: u64,
}

impl Default for Pace {
    fn default() -> Self {
        Self {
            page_txs: 200,
            page_gap_ms: 1_000,
        }
    }
}

/// What one frame means for the run.
enum Step {
    Continue,
    /// Reconnect from the cursor: the frame did not continue the sequence.
    Resume,
    Stop(Halt),
}

/// The follower: a store, its applier and a feed.
pub struct Follower<'a, F: Feed> {
    store: &'a mut Store,
    applier: Applier,
    feed: F,
    now_ms: fn() -> u64,
}

impl<'a, F: Feed> Follower<'a, F> {
    /// Open the follower on `store`, resuming its persisted cursor.
    ///
    /// # Errors
    /// A store error reading the applier's progress.
    pub fn open(store: &'a mut Store, feed: F) -> crate::error::Result<Self> {
        let applier = Applier::open(store)?;
        Ok(Self {
            store,
            applier,
            feed,
            now_ms: wall_ms,
        })
    }

    /// The last source transaction applied in full (0 before the first).
    #[must_use]
    pub fn cursor(&self) -> i64 {
        self.applier.progress().map_or(0, |p| p.source_tx)
    }

    /// Page from the cursor until a page reaches the primary's watermark.
    /// Returns the number of pages read.
    ///
    /// # Errors
    /// [`Halt`]: a transport or store failure (retry from the durable
    /// cursor), or a refusal to continue.
    pub fn bootstrap(&mut self, pace: Pace, sleep: &mut dyn FnMut(u64)) -> Result<usize, Halt> {
        let mut pages = 0;
        let mut limit = pace.page_txs.max(1);
        loop {
            let since = self.cursor();
            let body = match self.feed.page(since, limit) {
                Ok(body) => body,
                Err(error) if too_large(&error) && limit > 1 => {
                    limit = limit.div_ceil(2);
                    sleep(pace.page_gap_ms);
                    continue;
                }
                Err(error) if too_large(&error) => {
                    return Err(Halt::TooLarge(format!(
                        "the source transaction after {since} alone is over the {} MiB page cap",
                        PAGE_MAX_BYTES >> 20
                    )));
                }
                Err(error) => return Err(Halt::Transport(error)),
            };
            pages += 1;
            // Back toward the paced size once pages fit again.
            limit = (limit * 2).min(pace.page_txs.max(1));
            if let Step::Stop(halt) = self.apply("quipu.changes", &body)? {
                return Err(halt);
            }
            let page: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            let watermark = page["watermark_tx"].as_i64().unwrap_or(i64::MAX);
            let next = page["next_tx"].as_i64().unwrap_or(since);
            // Caught up: the page reached the head, or it moved nothing.
            if next >= watermark || self.cursor() >= watermark || next <= since {
                // Only now may the follower serve reads in the primary's place.
                crate::follow_ready::mark_caught_up(self.store)?;
                return Ok(pages);
            }
            sleep(pace.page_gap_ms);
        }
    }

    /// Read one stream connection from the cursor until it ends or a frame
    /// asks to resume (`Ok`: reconnect from the cursor), or until `max_frames`
    /// frames have been handled (`None`: no limit).
    ///
    /// # Errors
    /// [`Halt`]: a transport or store failure, a refusal to continue, or the
    /// frame budget.
    pub fn tail_once(&mut self, max_frames: Option<usize>) -> Result<(), Halt> {
        let reader = self.feed.stream(self.cursor()).map_err(Halt::Transport)?;
        for (handled, event) in SseEvents::new(reader).enumerate() {
            let (name, data) = event.map_err(Halt::Transport)?;
            match self.apply(&name, &data)? {
                Step::Continue => {}
                Step::Resume => return Ok(()),
                Step::Stop(halt) => return Err(halt),
            }
            if max_frames.is_some_and(|max| handled + 1 >= max) {
                return Err(Halt::Budget);
            }
        }
        Ok(())
    }

    fn apply(&mut self, event: &str, data: &str) -> Result<Step, Halt> {
        let outcome = self
            .applier
            .apply_frame(self.store, event, data, (self.now_ms)())?;
        Ok(match outcome {
            Outcome::Resume { .. } => Step::Resume,
            Outcome::Rebootstrap { reason }
            | Outcome::Refused { reason }
            | Outcome::Revoked { reason } => Step::Stop(Halt::Rebuild(reason)),
            Outcome::Stopped { reason } => Step::Stop(Halt::Rebuild(format!("{reason:?}"))),
            _ => Step::Continue,
        })
    }
}

/// What a follower database holds, so a reader can state its staleness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    /// The store the data came from: the source's id in a follower built by
    /// the applier, otherwise this store's own id (a file-level copy).
    pub lineage: String,
    /// The last source transaction held in full.
    pub source_tx: i64,
    /// When that transaction committed at the source (the applier writes each
    /// one with the source's timestamp), if any transaction exists.
    pub source_time: Option<String>,
}

/// The [`Position`] of `store`. Reads only, so it works on a read-only open.
///
/// # Errors
/// A store error reading the progress record or the transaction log.
pub fn position(store: &Store) -> crate::error::Result<Position> {
    let has_progress: bool = store.conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'replica_progress')",
        [],
        |r| r.get(0),
    )?;
    let progress: Option<(String, i64)> = if has_progress {
        rusqlite::OptionalExtension::optional(store.conn.query_row(
            "SELECT store_id, source_tx FROM replica_progress WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ))?
    } else {
        None
    };
    let source_time: Option<String> = rusqlite::OptionalExtension::optional(store.conn.query_row(
        "SELECT timestamp FROM transactions ORDER BY id DESC LIMIT 1",
        [],
        |r| r.get(0),
    ))?;
    Ok(match progress {
        Some((lineage, source_tx)) => Position {
            lineage,
            source_tx,
            source_time,
        },
        None => Position {
            lineage: store.store_id()?,
            source_tx: store.latest_tx_id()?,
            source_time,
        },
    })
}

fn wall_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Server-Sent Events from a line reader: `(event, data)` per dispatched
/// event. Comments (heartbeats) and `id:` lines are skipped; the applier
/// carries its own cursor. A read error ends the iteration with that error.
pub struct SseEvents<R: BufRead> {
    reader: R,
}

impl<R: BufRead> SseEvents<R> {
    /// Parse `reader`.
    pub fn new(reader: R) -> Self {
        Self { reader }
    }
}

impl<R: BufRead> Iterator for SseEvents<R> {
    type Item = std::io::Result<(String, String)>;

    fn next(&mut self) -> Option<Self::Item> {
        let (mut name, mut data, mut has_data) = (String::new(), String::new(), false);
        let mut line = String::new();
        loop {
            line.clear();
            match self.reader.read_line(&mut line) {
                Ok(0) => return None,
                Ok(_) => {}
                Err(error) => return Some(Err(error)),
            }
            let line = line.trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                if has_data {
                    let name = if name.is_empty() {
                        "message".into()
                    } else {
                        name
                    };
                    return Some(Ok((name, data)));
                }
                name.clear();
                continue;
            }
            if line.starts_with(':') {
                continue;
            }
            let (field, value) = line.split_once(':').unwrap_or((line, ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match field {
                "event" => value.clone_into(&mut name),
                "data" => {
                    if has_data {
                        data.push('\n');
                    }
                    data.push_str(value);
                    has_data = true;
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
#[path = "follow_tests.rs"]
mod tests;
