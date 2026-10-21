//! Optional, sealed alignment evidence. Transport never applies identity mappings.
use crate::Store;
use crate::error::{Error, Result};
use crate::share::{ShareManifest, sha256};
use oxrdfio::{RdfFormat, RdfParser};
use rusqlite::OptionalExtension;

/// Fixed ancillary member name; no caller-selected archive path.
pub const ALIGNMENTS_FILE: &str = "alignments.ttl";
/// Bound parser and retained evidence independently of the archive limit.
pub const MAX_ALIGNMENT_BYTES: usize = 2 * 1024 * 1024;

/// Validate syntax only. Neither declarations nor records grant authority.
pub fn validate(text: &str) -> Result<()> {
    if text.len() > MAX_ALIGNMENT_BYTES {
        return Err(Error::InvalidValue(
            "alignment member exceeds byte limit".into(),
        ));
    }
    for quad in RdfParser::from_format(RdfFormat::Turtle).for_reader(text.as_bytes()) {
        quad.map_err(|error| Error::InvalidValue(format!("alignment Turtle: {error}")))?;
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn read_optional(path: &std::path::Path) -> Result<Option<String>> {
    use std::io::Read;
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Error::InvalidValue(format!(
                "alignment member metadata: {error}"
            )));
        }
    };
    if !metadata.is_file() || metadata.len() > MAX_ALIGNMENT_BYTES as u64 {
        return Err(Error::InvalidValue(
            "alignment member must be a bounded regular file".into(),
        ));
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|error| Error::InvalidValue(format!("alignment member open: {error}")))?;
    let metadata = file
        .metadata()
        .map_err(|error| Error::InvalidValue(format!("alignment member stat: {error}")))?;
    if !metadata.is_file() || metadata.len() > MAX_ALIGNMENT_BYTES as u64 {
        return Err(Error::InvalidValue(
            "alignment member must be a bounded regular file".into(),
        ));
    }
    let mut bytes = Vec::new();
    file.take((MAX_ALIGNMENT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| Error::InvalidValue(format!("alignment member read: {error}")))?;
    if bytes.len() > MAX_ALIGNMENT_BYTES {
        return Err(Error::InvalidValue(
            "alignment member exceeds byte limit".into(),
        ));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| Error::InvalidValue(format!("alignment member UTF-8: {error}")))
}

/// Read bounded regular UTF-8 producer input. Unix opens refuse symlinks and FIFO races.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_file(path: &std::path::Path) -> Result<String> {
    read_optional(path)?
        .ok_or_else(|| Error::InvalidValue("alignment input file is missing".into()))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn read_directory(folder: &std::path::Path) -> Result<Option<String>> {
    read_optional(&folder.join(ALIGNMENTS_FILE))
}

pub(crate) fn verify_member(manifest: &ShareManifest, member: Option<&str>) -> Result<()> {
    match (
        manifest.alignments_hash.as_deref(),
        manifest.files.alignments.as_deref(),
        member,
    ) {
        (None, None, None) => Ok(()),
        (Some(hash), Some(ALIGNMENTS_FILE), Some(text)) => {
            if sha256(text.as_bytes()) != hash {
                return Err(Error::InvalidValue("share alignments hash mismatch".into()));
            }
            validate(text)
        }
        _ => Err(Error::InvalidValue(
            "alignments.ttl must be declared, correctly named, sealed and present together".into(),
        )),
    }
}

pub(crate) fn retain(store: &Store, share_id: &str, member: Option<&str>) -> Result<()> {
    if let Some(text) = member {
        store.conn.execute_batch("CREATE TABLE IF NOT EXISTS share_alignment_members(share_id TEXT PRIMARY KEY, member TEXT NOT NULL)")?;
        store.conn.execute("INSERT INTO share_alignment_members(share_id,member) VALUES(?1,?2) ON CONFLICT(share_id) DO NOTHING", rusqlite::params![share_id, text])?;
    }
    Ok(())
}

/// Read exact verified import evidence for explicit curation, never graph adoption.
pub fn imported_member(store: &Store, share_id: &str) -> Result<Option<String>> {
    let exists: bool = store.conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='share_alignment_members')", [], |row| row.get(0),
    )?;
    if !exists {
        return Ok(None);
    }
    Ok(store
        .conn
        .query_row(
            "SELECT member FROM share_alignment_members WHERE share_id=?1",
            [share_id],
            |row| row.get(0),
        )
        .optional()?)
}

#[cfg(test)]
#[path = "share_alignments_tests.rs"]
mod tests;
