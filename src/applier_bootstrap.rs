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
//! Bootstrap is all or nothing. Every check below refuses with nothing
//! written; on success the facts and the cursor `(store_id, log_epoch,
//! watermark_tx)` commit in one savepoint.

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

impl Applier {
    /// One frame of an as-of share. Only an empty replica can be bootstrapped.
    pub(super) fn bootstrap_frame(
        &mut self,
        store: &mut Store,
        event: &str,
        data: &str,
    ) -> Result<Outcome> {
        let Ok(frame) = serde_json::from_str::<Json>(data) else {
            self.share = None;
            return Ok(refused("share frame is not JSON"));
        };
        match event {
            "quipu.asof" => {
                let records = frame["records"].as_array().cloned().unwrap_or_default();
                self.bootstrap(store, &frame["manifest"], &records)
            }
            "quipu.asof.begin" => {
                let manifest = frame["manifest"].clone();
                let (Some(watermark), Some(parts), Some(records), Some(digest)) = (
                    manifest["watermark_tx"].as_i64(),
                    frame["parts"].as_u64(),
                    frame["records"].as_u64(),
                    frame["digest"].as_str(),
                ) else {
                    return Ok(refused("share begin frame is incomplete"));
                };
                // A new begin replaces a share in progress: a reconnect
                // restarts the cut, it never resumes half-way.
                self.share = Some((
                    Chunk {
                        source_tx: watermark,
                        parts: usize::try_from(parts).unwrap_or(usize::MAX),
                        records: usize::try_from(records).unwrap_or(usize::MAX),
                        seen: 0,
                        digest: digest.to_string(),
                        hasher: Sha256::new(),
                        buffered: Vec::new(),
                    },
                    manifest,
                ));
                Ok(Outcome::Buffered {
                    source_tx: watermark,
                })
            }
            "quipu.asof.part" => {
                let Some((chunk, _)) = self.share.as_mut() else {
                    return Ok(Outcome::Resume { from: 0 });
                };
                if !chunk.accept(&frame, data) {
                    self.share = None;
                    return Ok(Outcome::Resume { from: 0 });
                }
                Ok(Outcome::Buffered {
                    source_tx: chunk.source_tx,
                })
            }
            // "quipu.asof.commit"
            _ => {
                let Some((chunk, manifest)) = self.share.take() else {
                    return Ok(Outcome::Resume { from: 0 });
                };
                if !chunk.complete(&frame) {
                    return Ok(Outcome::Resume { from: 0 });
                }
                self.bootstrap(store, &manifest, &chunk.buffered)
            }
        }
    }

    /// Validate a whole share and apply it in one savepoint.
    fn bootstrap(
        &mut self,
        store: &mut Store,
        manifest: &Json,
        records: &[Json],
    ) -> Result<Outcome> {
        if let Some(p) = &self.progress {
            return Ok(refused(&format!(
                "bootstrap needs an empty replica; this one follows {}/{} at {}",
                p.source.store_id, p.source.log_epoch, p.source_tx
            )));
        }
        let holds_facts: bool =
            store
                .conn
                .query_row("SELECT EXISTS (SELECT 1 FROM facts)", [], |r| r.get(0))?;
        if holds_facts {
            return Ok(refused(
                "bootstrap needs an empty replica; this store holds facts",
            ));
        }
        let (source, watermark, graphs) = match check_manifest(manifest) {
            Ok(m) => m,
            Err(reason) => return Ok(refused(&reason)),
        };
        // Every record is a live fact of a graph the manifest lists, and each
        // graph's records are exactly the ones it counted and digested.
        let mut by_graph: BTreeMap<String, Vec<&Json>> = BTreeMap::new();
        for record in records {
            if record["op"].as_str() != Some("assert") {
                return Ok(refused(
                    "a share carries live facts only; a record is not an assert",
                ));
            }
            let graph = record["graph"].as_str().unwrap_or("ROOT").to_string();
            if !graphs.contains_key(&graph) {
                return Ok(refused(&format!(
                    "record in graph {graph:?} outside the share's scope"
                )));
            }
            by_graph.entry(graph).or_default().push(record);
        }
        for (graph, (count, digest)) in &graphs {
            let held: &[&Json] = by_graph.get(graph).map_or(&[], Vec::as_slice);
            if held.len() != *count || share_graph_digest(held) != *digest {
                return Ok(refused(&format!(
                    "graph {graph:?} does not match the manifest's count and digest"
                )));
            }
        }
        if let Some(reason) = apply_share(store, &source, watermark, &by_graph)? {
            return Ok(refused(&reason));
        }
        self.progress = Some(Progress {
            source,
            source_tx: watermark,
        });
        Ok(Outcome::Bootstrapped {
            source_tx: watermark,
        })
    }
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

/// Write every graph's facts and the cursor in one savepoint.
/// `Ok(Some(reason))` refuses with nothing written.
fn apply_share(
    store: &mut Store,
    source: &Source,
    watermark: i64,
    by_graph: &BTreeMap<String, Vec<&Json>>,
) -> Result<Option<String>> {
    store.conn.execute_batch("SAVEPOINT replica_bootstrap")?;
    let result = (|| -> Result<Option<String>> {
        for (graph, records) in by_graph {
            let timestamp = records[0]["timestamp"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let mut datums = Vec::with_capacity(records.len());
            for record in records {
                match record_to_datum(store, record, &timestamp)? {
                    Ok(d) => datums.push(d),
                    Err(reason) => return Ok(Some(format!("share graph {graph:?}: {reason}"))),
                }
            }
            if graph == "ROOT" {
                store.transact(&datums, &timestamp, None, None)?;
            } else {
                let g = store.graph_create(graph)?;
                store.transact_to_graph(&datums, &timestamp, None, None, g)?;
            }
        }
        store.conn.execute(
            "INSERT INTO replica_progress (id, store_id, log_epoch, source_tx) VALUES (1, ?1, ?2, ?3) \
             ON CONFLICT(id) DO UPDATE SET store_id = ?1, log_epoch = ?2, source_tx = ?3",
            params![source.store_id, source.log_epoch, watermark],
        )?;
        Ok(None)
    })();
    match result {
        Ok(None) => {
            store.conn.execute_batch("RELEASE replica_bootstrap")?;
            Ok(None)
        }
        other => {
            store
                .conn
                .execute_batch("ROLLBACK TO replica_bootstrap; RELEASE replica_bootstrap")?;
            other
        }
    }
}

#[cfg(test)]
#[path = "applier_bootstrap_tests.rs"]
mod tests;
