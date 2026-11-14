//! Explicit, bounded pending evidence in a pendant; never an origin grant.
use std::collections::BTreeMap;

use crate::share::{ShareManifest, manifest_bytes};
use crate::{Error, Result, Store};

const MAX_CARRIERS: usize = 64;
const MAX_TOTAL_BYTES: usize = 32 * 1024 * 1024;
pub(super) type Selected = BTreeMap<String, String>;

fn invalid() -> Error {
    Error::InvalidValue("pack: invalid or overbound pending fluent carriers".into())
}

/// Bound serialization before canonicalization clones any manifest fields.
struct ByteBudget(usize);
impl std::io::Write for ByteBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.0 {
            return Err(std::io::Error::other("pending carrier byte bound"));
        }
        self.0 -= bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn select(manifests: &[ShareManifest]) -> Result<Selected> {
    if manifests.len() > MAX_CARRIERS {
        return Err(invalid());
    }
    let mut selected = Selected::new();
    let mut total = 0usize;
    for manifest in manifests {
        let overhead = manifest.share_id.len().checked_add(1).ok_or_else(invalid)?;
        let remaining = MAX_TOTAL_BYTES
            .checked_sub(total)
            .and_then(|n| n.checked_sub(overhead))
            .ok_or_else(invalid)?;
        serde_json::to_writer(ByteBudget(remaining), manifest).map_err(|_| invalid())?;
        crate::share_fluent::check(manifest)?;
        if manifest.fluent_proofs.is_empty() && manifest.observable_configs.is_empty() {
            return Err(invalid());
        }
        let bytes = manifest_bytes(manifest, true)?;
        total = total
            .checked_add(bytes.len())
            .and_then(|n| n.checked_add(manifest.share_id.len()))
            .ok_or_else(invalid)?;
        if total > MAX_TOTAL_BYTES {
            return Err(invalid());
        }
        let text = String::from_utf8(bytes).map_err(|_| invalid())?;
        if let Some(previous) = selected.insert(manifest.share_id.clone(), text.clone())
            && previous != text
        {
            return Err(Error::InvalidValue(
                "pack: conflicting complete carrier manifests for one share identity".into(),
            ));
        }
    }
    Ok(selected)
}

/// Empty evidence emits nothing, so every historical content hash is unchanged.
pub(super) fn extend_canonical(canonical: &mut String, selected: &Selected) {
    if selected.is_empty() {
        return;
    }
    canonical.push_str("\n## quipu-pending-fluent-manifests v1\n");
    canonical.push_str(&format!("{}\n", selected.len()));
    for (id, text) in selected {
        canonical.push_str(&format!("{}:{}:{}:", id.len(), id, text.len()));
        canonical.push_str(text);
        canonical.push('\n');
    }
}

pub(super) fn enforce_destination(
    store: &Store,
    selected: &Selected,
    opts: &super::PackOptions,
) -> Result<()> {
    // Each complete raw manifest and its complete decoded proof projection are
    // inspected without changing the bytes retained or hashed in the artifact.
    for (id, text) in selected {
        let files = BTreeMap::from([("manifest.json".to_string(), text.clone())]);
        let inspection = crate::share_fluent::inspection_files(&files)?.ok_or_else(invalid)?;
        crate::share_scrub::enforce_destination(
            store,
            &inspection,
            opts.destination.clone(),
            &format!("pack pending carrier {id}"),
        )?;
    }
    Ok(())
}

pub(super) fn write(conn: &rusqlite::Connection, selected: &Selected) -> Result<()> {
    // This is a fresh build store. Only selected evidence is written; never
    // discover or copy source auth, permits, caches or origin receipts.
    for (id, text) in selected {
        conn.execute(
            "INSERT INTO share_fluent_carriers(share_id,manifest) VALUES(?1,?2)",
            rusqlite::params![id, text],
        )?;
    }
    Ok(())
}

pub(super) fn read(conn: &rusqlite::Connection) -> Result<Selected> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='share_fluent_carriers')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(Selected::new()); // legacy artifacts genuinely lack the table
    }
    // Census bytes before retrieving a body. Malformed schema/read errors are
    // propagated, never interpreted as an empty legacy extension.
    let (count, bytes): (i64, i64) = conn.query_row(
        "SELECT count(*),coalesce(sum(length(CAST(share_id AS BLOB))+length(CAST(manifest AS BLOB))),0) FROM share_fluent_carriers",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if count < 0 || count > MAX_CARRIERS as i64 || bytes < 0 || bytes > MAX_TOTAL_BYTES as i64 {
        return Err(invalid());
    }
    let mut statement =
        conn.prepare("SELECT share_id,manifest FROM share_fluent_carriers ORDER BY share_id")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut selected = Selected::new();
    for row in rows {
        let (id, text) = row?;
        let manifest: ShareManifest = serde_json::from_str(&text).map_err(|_| invalid())?;
        if manifest.share_id != id {
            return Err(invalid());
        }
        let checked = select(&[manifest])?;
        if checked.get(&id) != Some(&text) || selected.insert(id, text).is_some() {
            return Err(invalid());
        }
    }
    Ok(selected)
}
