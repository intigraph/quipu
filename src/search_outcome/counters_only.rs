//! Opt-in text-free, cross-process counts. Separate from query evidence journals.
use super::observation::Event;
#[cfg(unix)]
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

enum Mode {
    Off,
    Invalid,
    #[cfg(unix)]
    Ready(Mutex<rusqlite::Connection>),
}

static MODE: OnceLock<Mode> = OnceLock::new();
static WRITTEN: AtomicU64 = AtomicU64::new(0);
static FAILED: AtomicU64 = AtomicU64::new(0);

fn mode() -> &'static Mode {
    MODE.get_or_init(
        || match std::env::var("QUIPU_SEARCH_COUNTERS_ONLY_ENABLED").as_deref() {
            Err(std::env::VarError::NotPresent) | Ok("false") => Mode::Off,
            Ok("true") => {
                #[cfg(unix)]
                if let Some(path) = std::env::var_os("QUIPU_SEARCH_COUNTERS_ONLY_DIR") {
                    return open(std::path::Path::new(&path))
                        .map_or(Mode::Invalid, |conn| Mode::Ready(Mutex::new(conn)));
                }
                Mode::Invalid
            }
            _ => Mode::Invalid,
        },
    )
}

pub(super) fn selected() -> bool {
    !matches!(mode(), Mode::Off)
}

pub(super) fn append(_event: &Event) {
    #[cfg(unix)]
    if let Mode::Ready(conn) = mode() {
        let result = conn.lock().map_err(|_| ()).and_then(|mut conn| {
            record(&mut conn, &crate::metrics::search_counter_delta(_event)).map_err(|_| ())
        });
        if result.is_ok() {
            WRITTEN.fetch_add(1, Ordering::Relaxed);
            return;
        }
    }
    // Static diagnostic only: no path, query, logical ID or caller declaration.
    // An uncertain write is never retried; no query is failed for telemetry.
    FAILED.fetch_add(1, Ordering::Relaxed);
    eprintln!("quipu search counters-only capture UNKNOWN; aggregate not confirmed");
}

pub(crate) fn render_health(out: &mut String) {
    use std::fmt::Write as _;
    if MODE.get().is_some_and(|mode| !matches!(mode, Mode::Off)) {
        out.push_str("# TYPE quipu_search_counters_only_capture_total counter\n");
        let _ = writeln!(
            out,
            "quipu_search_counters_only_capture_total{{outcome=\"written\"}} {}",
            WRITTEN.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            out,
            "quipu_search_counters_only_capture_total{{outcome=\"error\"}} {}",
            FAILED.load(Ordering::Relaxed)
        );
    }
}

#[cfg(unix)]
fn open(directory: &std::path::Path) -> Result<rusqlite::Connection, Box<dyn std::error::Error>> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    if !directory.is_absolute() {
        return Err("counter directory must be absolute".into());
    }
    let dir = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(directory)?;
    let meta = dir.metadata()?;
    let uid = rustix::process::geteuid().as_raw();
    if meta.uid() != uid || meta.mode() & 0o077 != 0 {
        return Err("counter directory must be private and owned".into());
    }
    let path = directory.join("counts.sqlite");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&path)?;
    let before = file.metadata()?;
    if !before.is_file() || before.uid() != uid || before.mode() & 0o077 != 0 || before.nlink() != 1
    {
        return Err("counter database must be a private owned regular file".into());
    }
    let conn = rusqlite::Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    let after = std::fs::symlink_metadata(&path)?;
    let current_dir = std::fs::symlink_metadata(directory)?;
    if before.ino() != after.ino()
        || before.dev() != after.dev()
        || meta.ino() != current_dir.ino()
        || meta.dev() != current_dir.dev()
    {
        return Err("counter identity changed".into());
    }
    conn.busy_timeout(std::time::Duration::from_millis(5))?;
    conn.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA synchronous=FULL;")?;
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version == 0 {
        let tables: i64 = conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )?;
        if tables != 0 {
            return Err("unknown existing counter state".into());
        }
        conn.execute_batch("BEGIN IMMEDIATE;
            CREATE TABLE counts (series TEXT PRIMARY KEY, value INTEGER NOT NULL CHECK(typeof(value)='integer' AND value>=0), last_success INTEGER NOT NULL);
            CREATE TABLE health (singleton INTEGER PRIMARY KEY CHECK(singleton=1), last_success INTEGER NOT NULL);
            PRAGMA user_version=1; COMMIT;")?;
    } else if version != 1 {
        return Err("unknown counter schema".into());
    }
    Ok(conn)
}

#[cfg(unix)]
fn record(conn: &mut rusqlite::Connection, delta: &str) -> rusqlite::Result<()> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    for line in delta.lines().filter(|line| !line.starts_with('#')) {
        if let Some(series) = line.strip_suffix(" 1") {
            tx.execute(
                "INSERT INTO counts(series,value,last_success) VALUES(?1,1,unixepoch())
                ON CONFLICT(series) DO UPDATE SET value=value+1,last_success=excluded.last_success",
                [series],
            )?;
        }
    }
    let n: i64 = tx.query_row("SELECT count(*) FROM counts", [], |r| r.get(0))?;
    if n > 4096 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    tx.execute(
        "INSERT INTO health VALUES(1,unixepoch())
        ON CONFLICT(singleton) DO UPDATE SET last_success=excluded.last_success",
        [],
    )?;
    tx.commit()
}

#[cfg(all(test, unix))]
mod tests;
