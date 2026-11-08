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
        try_lock(&file).map_err(|e| {
            Error::Store(format!(
                "cannot acquire single-writer lock for {path}: {e}; use the REST client for a served database"
            ))
        })?;
        Ok(Self { _file: file })
    }
}

#[cfg(not(target_os = "macos"))]
fn try_lock(file: &File) -> std::result::Result<(), std::fs::TryLockError> {
    file.try_lock()
}

#[cfg(target_os = "macos")]
fn try_lock(file: &File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;

    // Darwin flock conflicts with SQLite's POSIX transaction locks, including
    // in this process. Reserve byte zero with an open-file-description lock;
    // SQLite locks its database at PENDING_BYTE (0x40000000).
    // OFD ownership survives unrelated SQLite descriptors closing, unlike a
    // process-associated F_SETLK lock. Closing this guard releases ownership.
    // Unsupported filesystems must refuse rather than open without a lock.
    let lock = libc::flock {
        l_start: 0,
        l_len: 1,
        l_pid: -1,
        l_type: libc::F_WRLCK,
        l_whence: libc::SEEK_SET as _,
    };
    // SAFETY: file owns a live descriptor and lock is initialized for this
    // nonblocking call. The kernel does not retain the Rust reference.
    #[allow(unsafe_code)] // Narrow FFI boundary; the safety argument is above.
    let status = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_OFD_SETLK, &lock) };
    if status == -1 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
