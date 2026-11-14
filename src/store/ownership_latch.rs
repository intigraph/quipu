//! Source-only per-lineage latch. Commit permits are shared; journal
//! transitions are exclusive. Neither waits while SQLite owns a writer lock.
//! The parent directory is an assumed protected native namespace, not supplied
//! by a transferred database. No production enrollment selects it yet.
#![allow(dead_code)]

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

fn denied(why: impl std::fmt::Display) -> Error {
    Error::PolicyDenied(format!("ownership latch: {why}"))
}

pub(super) fn sibling(path: &Path, suffix: &str) -> Result<PathBuf> {
    let name = path.file_name().ok_or_else(|| denied("no journal name"))?;
    let mut name = name.to_os_string();
    name.push(suffix);
    Ok(path.with_file_name(name))
}

/// Refuse links/nonregular and group/world-accessible authority files. This
/// does not authenticate a directory or defend against its privileged owner.
pub(super) fn regular(file: &File) -> Result<()> {
    let metadata = file.metadata().map_err(denied)?;
    if !metadata.is_file() {
        return Err(denied("not a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.nlink() != 1 || metadata.permissions().mode() & 0o077 != 0 {
            return Err(denied("authority file is linked or not owner-only"));
        }
    }
    Ok(())
}

pub(super) fn no_follow(options: &mut OpenOptions) -> &mut OpenOptions {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // NONBLOCK also prevents opening a substituted FIFO from hanging.
        options
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .mode(0o600);
    }
    options
}

fn open(path: &Path) -> Result<File> {
    let file = no_follow(
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false),
    )
    .open(sibling(path, ".lock")?)
    .map_err(denied)?;
    regular(&file)?;
    Ok(file)
}

/// Held until physical SQL completion, not merely until its commit callback.
pub(super) struct Shared(File);

pub(super) fn shared(path: &Path) -> Result<Shared> {
    let file = open(path)?;
    file.try_lock_shared().map_err(denied)?;
    Ok(Shared(file))
}

/// A narrow internal transition token. Its path cannot be swapped after the
/// exclusive latch has been acquired. There is no public token constructor.
pub(super) struct Transition {
    path: PathBuf,
    _file: File,
}

impl Transition {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

pub(super) fn exclusive(path: &Path) -> Result<Transition> {
    let file = open(path)?;
    file.try_lock().map_err(denied)?;
    Ok(Transition {
        path: path.to_path_buf(),
        _file: file,
    })
}
