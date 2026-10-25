//! The shared applier, slice 2: a replica bootstrapped from an as-of share
//! (`docs/design/as-of-share.md`), then tailed by slice 1 from its watermark.
//!
//! The share carries the source's live facts at one transaction as ordinary
//! change records, so bootstrap and tail use ONE record-to-datum conversion
//! and the replica proves equality with the same live digest. It arrives as
//! one `quipu.asof` frame, or, past the frame budget, as `quipu.asof.begin`,
//! numbered `quipu.asof.part` frames and a `quipu.asof.commit` barrier, with
//! the same digest chain as a chunked transaction.
//!
//! Bootstrap is all or nothing, and it never holds the share in memory. The
//! begin frame (or the single frame) opens ONE local savepoint; each part's
//! facts are written as it arrives, in batches the size of a part, and its
//! canonical lines go to a temporary table so the per-graph digests can be
//! computed in order at the commit without keeping the records. Every check
//! refuses by rolling the savepoint back, with nothing written; on success
//! the facts and the cursor `(store_id, log_epoch, watermark_tx)` commit
//! together.

use std::collections::BTreeMap;

use rusqlite::params;
use serde_json::Value as Json;
use sha2::{Digest, Sha256};

use super::{Applier, Chunk, Outcome, Progress, Source, record_to_datum, refused, source_of};
use crate::error::Result;
use crate::store::Store;

/// The share schema this applier accepts.
pub const SHARE_SCHEMA: &str = "quipu-asof-share/v1";

/// SHA-256 over one graph's records as the share manifest states them: each
/// record as compact UTF-8 JSON with recursively sorted keys, the lines
/// sorted, joined by newlines. A producer and a follower compute it the same
/// way, so a record dropped, added or altered in transit is detected.
#[must_use]
pub fn share_graph_digest(records: &[&Json]) -> String {
    let mut lines: Vec<String> = records.iter().map(|r| canonical(r)).collect();
    lines.sort();
    hex::encode(Sha256::digest(lines.join("\n").as_bytes()))
}

/// Compact JSON with object keys sorted at every level, whatever order the
/// JSON library keeps keys in.
fn canonical(v: &Json) -> String {
    match v {
        Json::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let body: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{}:{}", Json::String(k.clone()), canonical(&map[k])))
                .collect();
            format!("{{{}}}", body.join(","))
        }
        Json::Array(items) => {
            let body: Vec<String> = items.iter().map(canonical).collect();
            format!("[{}]", body.join(","))
        }
        other => other.to_string(),
    }
}

/// Lines of the share in progress, one per record, for the commit's digests.
const LINES_DDL: &str = "CREATE TEMP TABLE IF NOT EXISTS replica_share_lines \
     (graph TEXT NOT NULL, line TEXT NOT NULL); DELETE FROM temp.replica_share_lines";

/// An as-of share being written inside the `replica_bootstrap` savepoint.
pub(super) struct ShareStream {
    /// The digest chain of a chunked share; `None` for a single frame.
    chain: Option<Chunk>,
    source: Source,
    watermark: i64,
    /// Per graph, the manifest's `(records, digest)`.
    graphs: BTreeMap<String, (usize, String)>,
    /// Per graph, the records written so far.
    counts: BTreeMap<String, usize>,
}

impl Applier {
    /// One frame of an as-of share. Only an empty replica can be bootstrapped.
    ///
    /// A store error while a share is open rolls the WHOLE share back before
    /// it is returned, so no fact of a half-applied share is ever visible,
    /// not even on this connection (review 6388 P2: a failed progress write
    /// after the share was taken left its savepoint open).
    pub(super) fn bootstrap_frame(
        &mut self,
        store: &mut Store,
        event: &str,
        data: &str,
    ) -> Result<Outcome> {
        let result = self.share_frame(store, event, data);
        if result.is_err() {
            self.share = None;
            discard_open_share(store)?;
        }
        result
    }

    fn share_frame(&mut self, store: &mut Store, event: &str, data: &str) -> Result<Outcome> {
        let Ok(frame) = serde_json::from_str::<Json>(data) else {
            self.abort_share(store)?;
            return Ok(refused("share frame is not JSON"));
        };
        let records = frame["records"].as_array().map_or(&[][..], Vec::as_slice);
        match event {
            "quipu.asof" => {
                self.abort_share(store)?;
                if let Some(outcome) = self.start_share(store, &frame["manifest"], None)? {
                    return Ok(outcome);
                }
                if let Some(outcome) = self.write_share(store, records)? {
                    return Ok(outcome);
                }
                self.finish_share(store)
            }
            "quipu.asof.begin" => {
                // A new begin replaces a share in progress: a reconnect
                // restarts the cut, it never resumes half-way.
                self.abort_share(store)?;
                let (Some(watermark), Some(parts), Some(count), Some(digest)) = (
                    frame["manifest"]["watermark_tx"].as_i64(),
                    frame["parts"].as_u64(),
                    frame["records"].as_u64(),
                    frame["digest"].as_str(),
                ) else {
                    return Ok(refused("share begin frame is incomplete"));
                };
                let chain = Chunk::new(watermark, parts, count, digest, false);
                if let Some(outcome) = self.start_share(store, &frame["manifest"], Some(chain))? {
                    return Ok(outcome);
                }
                Ok(Outcome::Buffered {
                    source_tx: watermark,
                })
            }
            "quipu.asof.part" => {
                let accepted = self
                    .share
                    .as_mut()
                    .and_then(|s| s.chain.as_mut())
                    .is_some_and(|chain| chain.accept(&frame, data));
                if !accepted {
                    self.abort_share(store)?;
                    return Ok(Outcome::Resume { from: 0 });
                }
                if let Some(outcome) = self.write_share(store, records)? {
                    return Ok(outcome);
                }
                Ok(Outcome::Buffered {
                    source_tx: self.share.as_ref().map_or(0, |s| s.watermark),
                })
            }
            // "quipu.asof.commit"
            _ => {
                let complete = self
                    .share
                    .as_ref()
                    .and_then(|s| s.chain.as_ref())
                    .is_some_and(|chain| chain.complete(&frame));
                if !complete {
                    self.abort_share(store)?;
                    return Ok(Outcome::Resume { from: 0 });
                }
                self.finish_share(store)
            }
        }
    }

    /// Roll back a share in progress, if any: nothing of it stays written.
    pub(super) fn abort_share(&mut self, store: &Store) -> Result<()> {
        if self.share.take().is_some() {
            store
                .conn
                .execute_batch("ROLLBACK TO replica_bootstrap; RELEASE replica_bootstrap")?;
        }
        Ok(())
    }

    /// Check that the replica can be bootstrapped from `manifest`, then open
    /// the savepoint. `Some` refuses with nothing opened.
    fn start_share(
        &mut self,
        store: &Store,
        manifest: &Json,
        chain: Option<Chunk>,
    ) -> Result<Option<Outcome>> {
        if let Some(p) = &self.progress {
            return Ok(Some(refused(&format!(
                "bootstrap needs an empty replica; this one follows {}/{} at {}",
                p.source.store_id, p.source.log_epoch, p.source_tx
            ))));
        }
        let holds_facts: bool =
            store
                .conn
                .query_row("SELECT EXISTS (SELECT 1 FROM facts)", [], |r| r.get(0))?;
        if holds_facts {
            return Ok(Some(refused(
                "bootstrap needs an empty replica; this store holds facts",
            )));
        }
        let (source, watermark, graphs) = match check_manifest(manifest) {
            Ok(m) => m,
            Err(reason) => return Ok(Some(refused(&reason))),
        };
        // The savepoint must be the outermost transaction, or "all or
        // nothing" would depend on a caller's commit.
        if !store.conn.is_autocommit() {
            return Ok(Some(refused(
                "the replica store already has a transaction open",
            )));
        }
        store.conn.execute_batch(LINES_DDL)?;
        store.conn.execute_batch("SAVEPOINT replica_bootstrap")?;
        self.share = Some(ShareStream {
            chain,
            source,
            watermark,
            graphs,
            counts: BTreeMap::new(),
        });
        Ok(None)
    }

    /// Write one batch of the share; `Some` has rolled the whole share back.
    fn write_share(&mut self, store: &mut Store, records: &[Json]) -> Result<Option<Outcome>> {
        let Some(share) = self.share.as_mut() else {
            return Ok(Some(Outcome::Resume { from: 0 }));
        };
        match write_batch(store, share, records)? {
            None => Ok(None),
            Some(reason) => {
                self.abort_share(store)?;
                Ok(Some(refused(&reason)))
            }
        }
    }

    /// Verify every graph's count and digest, then commit the facts and the
    /// cursor together.
    fn finish_share(&mut self, store: &mut Store) -> Result<Outcome> {
        let Some(share) = self.share.as_ref() else {
            return Ok(Outcome::Resume { from: 0 });
        };
        if let Some(reason) = verify(store, share)? {
            self.abort_share(store)?;
            return Ok(refused(&reason));
        }
        let Some(share) = self.share.take() else {
            return Ok(Outcome::Resume { from: 0 });
        };
        store.conn.execute(
            "INSERT INTO replica_progress (id, store_id, log_epoch, source_tx) VALUES (1, ?1, ?2, ?3) \
             ON CONFLICT(id) DO UPDATE SET store_id = ?1, log_epoch = ?2, source_tx = ?3",
            params![share.source.store_id, share.source.log_epoch, share.watermark],
        )?;
        store
            .conn
            .execute_batch("RELEASE replica_bootstrap; DELETE FROM temp.replica_share_lines")?;
        self.progress = Some(Progress {
            source: share.source,
            source_tx: share.watermark,
        });
        Ok(Outcome::Bootstrapped {
            source_tx: share.watermark,
        })
    }
}

/// Discard the savepoint of a share left open by an applier that went away
/// mid-share, so a new applier on the same store starts clean.
pub(super) fn discard_open_share(store: &Store) -> Result<()> {
    if !store.conn.is_autocommit() {
        store
            .conn
            .execute_batch("ROLLBACK TO replica_bootstrap; RELEASE replica_bootstrap")?;
    }
    Ok(())
}

/// Write one batch: every record a live fact of a listed graph, never more
/// records than the manifest counted. `Ok(Some(reason))` refuses the share.
fn write_batch(
    store: &mut Store,
    share: &mut ShareStream,
    records: &[Json],
) -> Result<Option<String>> {
    let mut by_graph: BTreeMap<&str, Vec<&Json>> = BTreeMap::new();
    for record in records {
        if record["op"].as_str() != Some("assert") {
            return Ok(Some(
                "a share carries live facts only; a record is not an assert".into(),
            ));
        }
        let graph = record["graph"].as_str().unwrap_or("ROOT");
        if !share.graphs.contains_key(graph) {
            return Ok(Some(format!(
                "record in graph {graph:?} outside the share's scope"
            )));
        }
        by_graph.entry(graph).or_default().push(record);
    }
    for (graph, records) in by_graph {
        let count = share.counts.entry(graph.to_string()).or_default();
        *count += records.len();
        if *count > share.graphs.get(graph).map_or(0, |(n, _)| *n) {
            return Ok(Some(format!(
                "graph {graph:?} has more records than the manifest counted"
            )));
        }
        {
            let mut insert = store.conn.prepare_cached(
                "INSERT INTO temp.replica_share_lines (graph, line) VALUES (?1, ?2)",
            )?;
            for record in &records {
                insert.execute(params![graph, canonical(record)])?;
            }
        }
        let timestamp = records[0]["timestamp"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let mut datums = Vec::with_capacity(records.len());
        for record in &records {
            match record_to_datum(store, record, &timestamp)? {
                Ok(d) => datums.push(d),
                Err(reason) => return Ok(Some(format!("share graph {graph:?}: {reason}"))),
            }
        }
        if graph == "ROOT" {
            let local = store.transact(&datums, &timestamp, None, None)?;
            super::map::record(store, &share.source, local, share.watermark)?;
        } else {
            let g = store.graph_create(graph)?;
            let local = store.transact_to_graph(&datums, &timestamp, None, None, g)?;
            super::map::record(store, &share.source, local, share.watermark)?;
        }
    }
    Ok(None)
}

/// `Some(reason)` when a graph's records are not exactly the ones the
/// manifest counted and digested. The digest is [`share_graph_digest`],
/// computed from the spilled lines in sorted order (SQLite's BINARY order is
/// the same bytewise order as sorting the strings).
fn verify(store: &Store, share: &ShareStream) -> Result<Option<String>> {
    let mut lines = store.conn.prepare_cached(
        "SELECT line FROM temp.replica_share_lines WHERE graph = ?1 ORDER BY line",
    )?;
    for (graph, (count, digest)) in &share.graphs {
        let mut hasher = Sha256::new();
        let mut rows = lines.query([graph])?;
        let mut first = true;
        while let Some(row) = rows.next()? {
            if !first {
                hasher.update(b"\n");
            }
            first = false;
            hasher.update(row.get_ref(0)?.as_str().unwrap_or_default().as_bytes());
        }
        let held = share.counts.get(graph).copied().unwrap_or(0);
        if held != *count || hex::encode(hasher.finalize()) != *digest {
            return Ok(Some(format!(
                "graph {graph:?} does not match the manifest's count and digest"
            )));
        }
    }
    Ok(None)
}

/// The manifest's source, watermark and per-graph `(records, digest)`, or why
/// it cannot be trusted to reproduce its source.
type Checked = (Source, i64, BTreeMap<String, (usize, String)>);

fn check_manifest(m: &Json) -> std::result::Result<Checked, String> {
    if m["schema"].as_str() != Some(SHARE_SCHEMA) {
        return Err(format!("share schema is not {SHARE_SCHEMA}"));
    }
    let source = source_of(m).ok_or("share does not name its source store and log epoch")?;
    let watermark = m["watermark_tx"]
        .as_i64()
        .ok_or("share does not name its watermark_tx")?;
    let c = &m["completeness"];
    // A share that cannot carry valid time or byte bodies cannot equal its
    // source; refuse it rather than bootstrap a replica that silently differs.
    if c["valid_time"].as_bool() != Some(true) || c["bytes"].as_str() != Some("lossless") {
        return Err("share does not claim valid time and lossless bytes".into());
    }
    let Some(listed) = c["graphs"].as_object() else {
        return Err("share does not list its graphs".into());
    };
    let mut graphs = BTreeMap::new();
    for (graph, entry) in listed {
        let (Some(records), Some(digest)) = (entry["records"].as_u64(), entry["digest"].as_str())
        else {
            return Err(format!("graph {graph:?} lacks its record count or digest"));
        };
        let digest = digest.strip_prefix("sha256:").unwrap_or(digest).to_string();
        graphs.insert(
            graph.clone(),
            (usize::try_from(records).unwrap_or(usize::MAX), digest),
        );
    }
    Ok((source, watermark, graphs))
}

#[cfg(test)]
#[path = "applier_bootstrap_tests.rs"]
mod tests;
