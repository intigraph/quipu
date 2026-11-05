//! Cooperative process ownership for servers and local language bindings.
//!
//! Lock the database inode itself: symlinks and hard links must not create a
//! second lock domain. This is separate from SQLite's transaction locks.

use std::fs::{File, OpenOptions};

use crate::{Error, Result};

/// Retain this guard until every writable store handle has been dropped.
/// Nonparticipating writers and replacing a live database file are unsupported.
pub struct WriterLock {
    _file: File,
}

impl WriterLock {
    /// Refuse before SQLite initialization when another participant owns the file.
    /// The file may be created, but is never truncated or removed by this guard.
    pub fn acquire(path: &str) -> Result<Self> {
        if path.is_empty() || path == ":memory:" || path.starts_with("file:") {
            return Err(Error::InvalidValue(
                "writer locks require a plain database filesystem path, not a SQLite URI".into(),
            ));
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|e| Error::Store(format!("cannot open writer lock for {path}: {e}")))?;
        file.try_lock().map_err(|e| {
            Error::Store(format!(
                "cannot acquire single-writer lock for {path}: {e}; use the REST client for a served database"
            ))
        })?;
        Ok(Self { _file: file })
    }
}
