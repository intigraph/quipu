//! Private named scratch custody for server restart reconciliation.
use super::{Snapshot, pages};
use crate::{Error, Result, Store};
use std::path::Path;

impl Snapshot {
    /// Capture or resume this store's private disk job. The caller must hold
    /// single-flight materialization admission. File stores retain one job next
    /// to the database; in-memory library stores use anonymous scratch instead.
    /// A completed job is replaced on the next invocation. Failed/incompatible
    /// jobs are retained and refused, never silently recaptured or called fresh.
    pub fn capture_or_resume(source: &Store, timestamp: &str) -> Result<Self> {
        let Some(path) = source.conn.path().filter(|p| !p.is_empty()) else {
            return Self::capture(source, Self::temporary_store()?, timestamp);
        };
        let database = Path::new(path);
        let parent = database
            .parent()
            .ok_or_else(|| Error::InvalidValue("OWL database parent missing".into()))?;
        use sha2::Digest;
        let identity = format!("{:x}", sha2::Sha256::digest(source.store_id()?.as_bytes()));
        let directory = parent.join(format!(".owl-job-{identity}"));
        private_directory(&directory)?;
        let job_path = directory.join("snapshot.sqlite");
        if job_path.exists() {
            private_file(&job_path)?;
            let scratch = Store::open(
                job_path
                    .to_str()
                    .ok_or_else(|| Error::InvalidValue("OWL scratch path is not UTF-8".into()))?,
            )?;
            pages::limits(&scratch)?;
            let job = pages::load(&scratch)?;
            if job.phase != "COMPLETE" {
                let snapshot = Self::resume(scratch)?;
                snapshot.validate(source)?;
                return Ok(snapshot);
            }
            drop(scratch);
            std::fs::remove_file(&job_path).map_err(|error| io_error(&error))?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&job_path).map_err(|error| io_error(&error))?;
        file.sync_all().map_err(|error| io_error(&error))?;
        drop(file);
        // A crash before CAPTURED is durable leaves an explicit invalid job;
        // it never permits publication or silently resets an admitted budget.
        Self::capture(
            source,
            Store::open(
                job_path
                    .to_str()
                    .ok_or_else(|| Error::InvalidValue("OWL scratch path is not UTF-8".into()))?,
            )?,
            timestamp,
        )
    }

    /// Continue private derivation or retain an already established fixpoint.
    pub fn derive_or_resume(&mut self) -> Result<()> {
        if self.derived {
            return Ok(());
        }
        self.derive()
    }
}

fn io_error(error: &std::io::Error) -> Error {
    Error::InvalidValue(format!("OWL scratch custody: {error}"))
}
fn private_directory(path: &Path) -> Result<()> {
    if !path.exists() {
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(path).map_err(|error| io_error(&error))?;
    }
    let meta = std::fs::symlink_metadata(path).map_err(|error| io_error(&error))?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(Error::InvalidValue(
            "OWL scratch directory is not private".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err(Error::InvalidValue(
                "OWL scratch directory permits other users".into(),
            ));
        }
    }
    Ok(())
}
fn private_file(path: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(path).map_err(|error| io_error(&error))?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(Error::InvalidValue(
            "OWL scratch file is not private".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err(Error::InvalidValue(
                "OWL scratch file permits other users".into(),
            ));
        }
    }
    Ok(())
}
