//! Explicit disk-store ownership. A copy is inspectable, never silently adopted.
use super::Store;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    version: u32,
    host: String,
    path: PathBuf,
    store_id: String,
    /// Epoch minted when ownership was established; freeze may advance it later.
    adopted_epoch: String,
}

fn failure(e: impl std::fmt::Display) -> Error {
    Error::PolicyDenied(format!(
        "store ownership: {e}; run explicit store adopt before writes or replication"
    ))
}

fn host() -> Result<String> {
    #[cfg(target_os = "linux")]
    {
        let id = fs::read_to_string("/etc/machine-id").map_err(failure)?;
        let id = id.trim();
        if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(failure("machine identity is unavailable"));
        }
        Ok(format!("machine-id:{id}"))
    }
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("/usr/sbin/ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
            .map_err(failure)?;
        if !output.status.success() {
            return Err(failure("platform identity query failed"));
        }
        let text = String::from_utf8(output.stdout).map_err(failure)?;
        let value = text
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once('=')?;
                (key.trim() == "\"IOPlatformUUID\"").then(|| value.trim().trim_matches('"'))
            })
            .ok_or_else(|| failure("platform UUID unavailable"))?;
        if value.len() != 36 || !value.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
            return Err(failure("invalid platform UUID"));
        }
        Ok(format!("platform-uuid:{value}"))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Err(failure(
            "disk ownership requires a supported stable host identity",
        ))
    }
}

fn sidecar(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".ownership.json");
    PathBuf::from(name)
}

fn record(path: &Path) -> Result<Binding> {
    let file = sidecar(path);
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(file).map_err(failure)?;
    let meta = file.metadata().map_err(failure)?;
    if !meta.is_file() || meta.len() > 16384 {
        return Err(failure("ownership record is not a bounded regular file"));
    }
    let mut bytes = Vec::new();
    file.take(16385).read_to_end(&mut bytes).map_err(failure)?;
    if bytes.len() > 16384 {
        return Err(failure("ownership record exceeds its bound"));
    }
    serde_json::from_slice(&bytes).map_err(failure)
}

impl Store {
    pub(super) fn open_owned(
        path: &str,
        attachments: &[super::attach::Attachment],
    ) -> Result<Self> {
        if path == ":memory:" {
            return Self::init_with_attachments(
                rusqlite::Connection::open_in_memory()?,
                attachments,
            );
        }
        let existing = fs::metadata(path).is_ok_and(|m| m.len() > 0);
        if existing {
            let inspected = Self::open_read_only(path)?;
            if inspected.require_ownership().is_err() {
                if !attachments.is_empty() {
                    inspected.require_ownership()?;
                }
                return Ok(inspected);
            }
        }
        let conn = rusqlite::Connection::open(path)?;
        let store = Self::init_with_attachments(conn, attachments)?;
        if !existing {
            store.bind_creation()?;
        }
        Ok(store)
    }

    /// Refuse replication from an unowned or relocated disk store. Memory-only
    /// stores have no persistent copy to bind. Ordinary read inspection remains possible.
    pub fn require_ownership(&self) -> Result<()> {
        let Some(path) = self.conn.path().filter(|path| !path.is_empty()) else {
            return Ok(());
        };
        let canonical = Path::new(path).canonicalize().map_err(failure)?;
        let binding = record(&canonical)?;
        if binding.version != 1
            || binding.host != host()?
            || binding.path != canonical
            || binding.store_id != self.store_id()?
        {
            return Err(failure("host, canonical path, or store identity differs"));
        }
        Ok(())
    }

    pub(super) fn bind_creation(&self) -> Result<()> {
        let Some(path) = self.conn.path().filter(|path| !path.is_empty()) else {
            return Ok(());
        };
        let path = Path::new(path).canonicalize().map_err(failure)?;
        let binding = Binding {
            version: 1,
            host: host()?,
            path: path.clone(),
            store_id: self.store_id()?,
            adopted_epoch: self.log_epoch()?,
        };
        let bytes = serde_json::to_vec_pretty(&binding).map_err(failure)?;
        let destination = sidecar(&path);
        if fs::symlink_metadata(&destination).is_ok_and(|m| !m.file_type().is_file()) {
            return Err(failure("refusing non-regular ownership destination"));
        }
        let suffix = super::open::new_urn_uuid("ownership temporary")?.replace(':', "-");
        let temporary = destination.with_extension(suffix);
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = (|| {
            let mut file = options.open(&temporary).map_err(failure)?;
            file.write_all(&bytes).map_err(failure)?;
            file.sync_all().map_err(failure)?;
            fs::rename(&temporary, &destination).map_err(failure)?;
            fs::File::open(path.parent().ok_or_else(|| failure("missing parent"))?)
                .and_then(|dir| dir.sync_all())
                .map_err(failure)
        })();
        if temporary.exists() {
            let _ = fs::remove_file(temporary);
        }
        result
    }

    /// Explicitly take ownership of an EXISTING database. Never creates one.
    /// A fresh epoch invalidates every prior stream cursor, including on the
    /// same host/path. Sidecar failure is reported; no successful adoption is claimed.
    pub fn adopt(path: &str) -> Result<String> {
        let path = Path::new(path).canonicalize().map_err(failure)?;
        let conn = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
        )?;
        host()?;
        let recognizable: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name IN ('facts', 'attestation_bindings'))",
            [], |row| row.get(0))?;
        if !recognizable {
            return Err(failure("not a recognized store schema"));
        }
        let store = Self::with_connection(conn);
        let tx = store.conn.unchecked_transaction()?;
        // Ownership establishes only lineage metadata. Do not populate other
        // excluded/local authority rows while restoring a verified full pack.
        // A subsequent owned open performs the ordinary legacy migrations.
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS store_identity (
                id INTEGER PRIMARY KEY CHECK (id = 1), store_id TEXT NOT NULL UNIQUE);
             CREATE TABLE IF NOT EXISTS log_epoch (
                id INTEGER PRIMARY KEY CHECK (id = 1), epoch TEXT NOT NULL,
                minted_at TEXT NOT NULL, reason TEXT NOT NULL);",
        )?;
        Self::ensure_store_identity(&store.conn)?;
        Self::ensure_log_epoch(&store.conn)?;
        let epoch = store.mint_log_epoch("explicit store adopt")?;
        tx.commit()?;
        store.bind_creation()?;
        store.require_ownership()?;
        Ok(epoch)
    }

    /// Restore a consistent SQLite snapshot to a new destination, then adopt it.
    /// Existing destinations are refused; the source is never modified.
    pub fn restore(source: &str, destination: &str) -> Result<String> {
        let target = Path::new(destination);
        let reserved = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)
            .map_err(failure)?;
        drop(reserved);
        (|| {
            let conn = rusqlite::Connection::open_with_flags(
                source,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?;
            conn.execute("VACUUM INTO ?1", [destination])?;
            Self::adopt(destination)
        })()
    }
}
