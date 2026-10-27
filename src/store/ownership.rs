//! Explicit disk-store ownership. A copy is inspectable, never silently adopted.
use super::Store;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// Stage A distinguishes legacy compatibility from verified native ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OwnershipState {
    /// Legacy startup and streams work, but no owned capability is granted.
    Inactive,
    /// Sticky native placement has been explicitly adopted and verified.
    Enforced,
}
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

/// Host identity is stable for a process; resolve it once so the per-write
/// fence on an ENFORCED store does not spawn a platform query every time.
/// Only a successful lookup is kept; a failure is retried on the next check.
fn host() -> Result<String> {
    static HOST: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    if let Some(known) = HOST.get() {
        return Ok(known.clone());
    }
    let resolved = host_uncached()?;
    Ok(HOST.get_or_init(|| resolved).clone())
}

fn host_uncached() -> Result<String> {
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

/// The ownership check on any connection to the store: the writer's own, or
/// the separate read-only connection the commit fence uses (see
/// `ownership_fence`), which may query while the writer holds its lock.
pub(super) fn state_on(conn: &rusqlite::Connection) -> Result<OwnershipState> {
    let marked: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='ownership_enforcement')",
        [],
        |r| r.get(0),
    )?;
    if !marked {
        // Browser VFS has no native sidecar namespace. This is explicitly
        // INACTIVE, never a successful ownership/provider check. A sticky
        // SQLite marker below still refuses there, without a cfg fallback.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = conn.path().filter(|p| !p.is_empty()) {
            match fs::symlink_metadata(sidecar(Path::new(path))) {
                Ok(_) => {
                    return Err(failure(
                        "partial ownership record without enforcement marker",
                    ));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(failure(e)),
            }
        }
        return Ok(OwnershipState::Inactive);
    }
    let version: Vec<i64> = conn
        .prepare("SELECT version FROM ownership_enforcement WHERE id=1 AND phase='ready'")?
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    let rows: i64 = conn.query_row("SELECT count(*) FROM ownership_enforcement", [], |r| {
        r.get(0)
    })?;
    if version != [1] || rows != 1 {
        return Err(failure("malformed enforcement marker"));
    }
    verify_binding_on(conn, host)?;
    Ok(OwnershipState::Enforced)
}

fn verify_binding_on(
    conn: &rusqlite::Connection,
    provider: impl FnOnce() -> Result<String>,
) -> Result<()> {
    let path = conn
        .path()
        .filter(|path| !path.is_empty())
        .ok_or_else(|| failure("native disk placement unavailable"))?;
    let canonical = Path::new(path).canonicalize().map_err(failure)?;
    let binding = record(&canonical)?;
    let adopted: String = conn.query_row(
        "SELECT adopted_epoch FROM ownership_enforcement WHERE id=1 AND phase='ready'",
        [],
        |r| r.get(0),
    )?;
    let store_id: String = conn.query_row(
        "SELECT store_id FROM store_identity WHERE id = 1",
        [],
        |r| r.get(0),
    )?;
    if binding.adopted_epoch != adopted
        || binding.version != 1
        || binding.host != provider()?
        || binding.path != canonical
        || binding.store_id != store_id
    {
        return Err(failure("host, canonical path, or store identity differs"));
    }
    Ok(())
}

impl Store {
    pub(super) fn open_owned(
        path: &str,
        attachments: &[super::attach::Attachment],
    ) -> Result<Self> {
        let conn = rusqlite::Connection::open(path)?;
        let inspected = Self::with_connection(conn);
        // Check before any schema migration. Invalid sticky/partial state is
        // inspectable only; selecting a default constructor cannot downgrade it.
        if inspected.ownership_state().is_err() {
            if !attachments.is_empty() {
                inspected.ownership_state()?;
            }
            drop(inspected);
            return Self::open_read_only(path);
        }
        Self::init_with_attachments(inspected.conn, attachments)
    }

    /// Explicit observable state. INACTIVE is not ownership authorization.
    /// The database marker travels with raw copies; removing only the sidecar
    /// cannot turn an enforced copy into an ordinary legacy store.
    pub fn ownership_state(&self) -> Result<OwnershipState> {
        state_on(&self.conn)
    }

    /// Write fence for handles that are ALREADY open. Opening re-checks
    /// ownership, but a writable handle opened earlier keeps its connection:
    /// a later failed adoption (sticky pending marker) or a lost/invalid
    /// binding must refuse its ordinary writes exactly as a fresh open would.
    /// Every committing write entrypoint calls this before it writes anything.
    /// INACTIVE legacy stores stay writable; this grants no ownership capability.
    pub fn ensure_ownership_writable(&self) -> Result<()> {
        if self
            .ownership_fence_tripped
            .swap(false, std::sync::atomic::Ordering::SeqCst)
            && !self.conn.is_autocommit()
        {
            // The commit fence refused an outermost RELEASE; its data is
            // already rolled back, only the open-transaction state remains.
            self.conn.execute_batch("ROLLBACK")?;
        }
        self.ownership_state().map(|_| ())
    }

    /// Ownership-authorized capability check. Legacy/in-memory/browser INACTIVE
    /// operation must never pass this check merely because it can serve reads.
    pub fn require_ownership(&self) -> Result<()> {
        match self.ownership_state()? {
            OwnershipState::Enforced => Ok(()),
            OwnershipState::Inactive => Err(failure("ownership enforcement is INACTIVE")),
        }
    }

    /// Provider injection for tests; production uses the cached host.
    #[cfg(test)]
    pub(super) fn verify_binding_with(
        &self,
        provider: impl FnOnce() -> Result<String>,
    ) -> Result<()> {
        verify_binding_on(&self.conn, provider)
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
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS ownership_enforcement (
            id INTEGER PRIMARY KEY CHECK(id=1), version INTEGER NOT NULL CHECK(version=1),
            phase TEXT NOT NULL CHECK(phase IN ('pending','ready')), adopted_epoch TEXT NOT NULL);",
        )?;
        let phases: Vec<(i64, String)> = tx
            .prepare("SELECT version,phase FROM ownership_enforcement")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        if !phases.is_empty() && phases != [(1, "ready".to_owned())] {
            return Err(failure(
                "incomplete or malformed adoption; operator recovery/readback required, refusing repeat",
            ));
        }
        Self::ensure_store_identity(&store.conn)?;
        Self::ensure_log_epoch(&store.conn)?;
        let epoch = store.mint_log_epoch("explicit store adopt")?;
        tx.execute("INSERT INTO ownership_enforcement(id,version,phase,adopted_epoch) VALUES(1,1,'pending',?1)
            ON CONFLICT(id) DO UPDATE SET phase='pending',adopted_epoch=excluded.adopted_epoch", [&epoch])?;
        tx.commit()?;
        store.bind_creation()?;
        // A failed file install leaves a sticky pending operation. An identical
        // adopt command cannot blindly mint another epoch after that failure.
        store.conn.execute("UPDATE ownership_enforcement SET phase='ready' WHERE id=1 AND phase='pending' AND adopted_epoch=?1", [&epoch])?;
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
