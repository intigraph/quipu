//! Optional persistent native cache; all reads are reverified by the resolver.
use super::{ArchiveCache, Error, Result};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// A receiver-selected cache directory. No directory is created until a
/// verified archive is stored, and an interrupted write never publishes a file.
pub struct DirectoryCache(pub PathBuf);

impl DirectoryCache {
    fn path(&self, digest: &str) -> Result<PathBuf> {
        let hex = digest.strip_prefix("sha256:").unwrap_or("");
        if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::InvalidValue("invalid archive cache key".into()));
        }
        Ok(self.0.join(format!("{hex}.pendant")))
    }
}

impl ArchiveCache for DirectoryCache {
    fn get(&mut self, digest: &str) -> Result<Option<Vec<u8>>> {
        let file = match std::fs::File::open(self.path(digest)?) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(Error::InvalidValue(format!("archive cache read: {e}"))),
        };
        let mut bytes = Vec::new();
        file.take((crate::share_transport::MAX_SHARE_DOWNLOAD_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| Error::InvalidValue(format!("archive cache read: {e}")))?;
        Ok(Some(bytes))
    }

    fn put(&mut self, digest: &str, bytes: &[u8]) -> Result<()> {
        let path = self.path(digest)?;
        let write = || -> std::io::Result<()> {
            std::fs::create_dir_all(&self.0)?;
            let mut temp = tempfile::NamedTempFile::new_in(Path::new(&self.0))?;
            temp.write_all(bytes)?;
            temp.as_file().sync_all()?;
            temp.persist(path).map_err(|e| e.error)?;
            Ok(())
        };
        write().map_err(|e| Error::InvalidValue(format!("archive cache write: {e}")))
    }
}
