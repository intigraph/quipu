//! Cross-process publication/mutation serialization on the existing policy inode.
//! No new grant store or credential lane. A busy writer refuses before mutation;
//! successful revocation therefore follows release of every published response.
use crate::{Error, Result};
use std::fs::File;

fn unavailable() -> Error {
    Error::CannotVerify("read policy publication barrier unavailable or busy".into())
}

/// Keep this permit with the actual buffered response body until EOF/drop.
/// Construct BEFORE the final policy/source/identity recheck, not afterwards.
pub struct PublicationPermit {
    _file: File,
}

pub(crate) struct PolicyMutationPermit {
    _file: File,
}

#[cfg(unix)]
fn open(path: &str) -> Result<File> {
    use std::fs::OpenOptions;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| unavailable())?;
    let held = file.metadata().map_err(|_| unavailable())?;
    let current = std::fs::symlink_metadata(path).map_err(|_| unavailable())?;
    if !held.is_file()
        || !current.is_file()
        || held.dev() != current.dev()
        || held.ino() != current.ino()
    {
        return Err(unavailable());
    }
    Ok(file)
}

#[cfg(not(unix))]
fn open(_: &str) -> Result<File> {
    Err(unavailable())
}

impl PublicationPermit {
    pub fn acquire(path: &str) -> Result<Self> {
        let file = open(path)?;
        file.try_lock_shared().map_err(|_| unavailable())?;
        Ok(Self { _file: file })
    }
}

impl PolicyMutationPermit {
    pub(crate) fn acquire(path: &str) -> Result<Self> {
        let file = open(path)?;
        file.try_lock().map_err(|_| unavailable())?;
        Ok(Self { _file: file })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn held_response_blocks_mutation_until_release_and_mutation_blocks_new_response() {
        let policy = tempfile::NamedTempFile::new().unwrap();
        let path = policy.path().to_str().unwrap();
        let first = PublicationPermit::acquire(path).unwrap();
        let second = PublicationPermit::acquire(path).unwrap();
        assert!(PolicyMutationPermit::acquire(path).is_err());
        drop(first);
        assert!(PolicyMutationPermit::acquire(path).is_err());
        drop(second);
        let mutation = PolicyMutationPermit::acquire(path).unwrap();
        assert!(PublicationPermit::acquire(path).is_err());
        drop(mutation);
        assert!(PublicationPermit::acquire(path).is_ok());
    }

    #[test]
    fn missing_or_symlink_policy_never_creates_or_locks_new_authority() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        assert!(PublicationPermit::acquire(missing.to_str().unwrap()).is_err());
        assert!(!missing.exists());
        let target = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        let alias = dir.path().join("alias");
        std::os::unix::fs::symlink(target.path(), &alias).unwrap();
        assert!(PublicationPermit::acquire(alias.to_str().unwrap()).is_err());
    }
}
