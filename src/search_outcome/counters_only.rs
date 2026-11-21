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

/// Scrape-time read of the cross-process aggregate (aegis-85x). Replaces the
/// scheduled textfile exporter with a pull on Prometheus's own scrape: same
/// fixed label set, same 4096-series bound, read-only, no schedule or result FD.
/// Any unreadable or out-of-contract state renders only `collector_success 0`;
/// a partial aggregate is never published.
pub(crate) fn render_aggregate(out: &mut String) {
    let body = match mode() {
        Mode::Off => return,
        Mode::Invalid => Err(()),
        #[cfg(unix)]
        Mode::Ready(conn) => conn
            .lock()
            .map_err(|_| ())
            .and_then(|conn| aggregate(&conn)),
    };
    let success = u8::from(body.is_ok());
    if let Ok(text) = body {
        out.push_str(&text);
    }
    out.push_str("# TYPE quipu_search_aggregate_collector_success gauge\n");
    out.push_str("quipu_search_aggregate_collector_success ");
    out.push_str(if success == 1 { "1\n" } else { "0\n" });
}

#[cfg(unix)]
fn aggregate(conn: &rusqlite::Connection) -> Result<String, ()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| ())?
        .as_secs();
    // One read snapshot; strict column types (a text value or integer series errors).
    let tx = conn.unchecked_transaction().map_err(|_| ())?;
    let rows = {
        let mut stmt = tx
            .prepare("SELECT series,value,last_success FROM counts ORDER BY series LIMIT 4097")
            .map_err(|_| ())?;
        stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })
        .map_err(|_| ())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| ())?
    };
    tx.finish().map_err(|_| ())?;
    render_rows(&rows, i64::try_from(now).map_err(|_| ())?)
}

fn series_contract() -> &'static regex::Regex {
    static SERIES: OnceLock<regex::Regex> = OnceLock::new();
    SERIES.get_or_init(|| {
        let prefix = r#"surface="(?:http|mcp|cli_local|library)",operation="(?:search|hybrid_search|search_nodes|search_facts|search_query|query|named_query)","#;
        regex::Regex::new(&format!(
            concat!(
                r#"^quipu_search_(?:requests_total\{{{p}outcome="(?:hits|empty|no_context|error|timeout|refused|unknown)"\}}"#,
                r#"|zero_results_total\{{{p}kind="(?:no_candidates|all_rejected|empty_rows)"\}}"#,
                r#"|miss_reasons_total\{{{p}reason="(?:no_candidates|empty_rows|no_result_above_floor|top_result_below_floor|identifier_not_covered|insufficient_term_coverage|subject_not_covered|query_too_long|query_terms_unavailable|other|unknown)"\}})$"#
            ),
            p = prefix
        ))
        .expect("fixed counter contract")
    })
}

/// Render validated rows exactly as the retired exporter did: the
/// `quipu_search_aggregate_` families plus a per-surface last-capture gauge.
fn render_rows(rows: &[(String, i64, i64)], now: i64) -> Result<String, ()> {
    use std::fmt::Write as _;
    if rows.len() > 4096 {
        return Err(());
    }
    let mut stamps = std::collections::BTreeMap::<&str, i64>::new();
    for (series, value, stamp) in rows {
        if !series_contract().is_match(series) || *value < 0 || !(0 < *stamp && *stamp <= now + 5) {
            return Err(());
        }
        if series.starts_with("quipu_search_requests_total{") {
            let surface = series["quipu_search_requests_total{surface=\"".len()..]
                .split('"')
                .next()
                .ok_or(())?;
            let entry = stamps.entry(surface).or_insert(0);
            *entry = (*entry).max(*stamp);
        }
    }
    let mut out = String::new();
    for name in ["requests_total", "zero_results_total", "miss_reasons_total"] {
        let _ = writeln!(out, "# TYPE quipu_search_aggregate_{name} counter");
        let source = format!("quipu_search_{name}{{");
        for (series, value, _) in rows.iter().filter(|row| row.0.starts_with(&source)) {
            let _ = writeln!(
                out,
                "quipu_search_aggregate_{} {value}",
                &series["quipu_search_".len()..]
            );
        }
    }
    out.push_str("# TYPE quipu_search_aggregate_last_capture_timestamp_seconds gauge\n");
    for (surface, stamp) in stamps {
        let _ = writeln!(
            out,
            "quipu_search_aggregate_last_capture_timestamp_seconds{{surface=\"{surface}\"}} {stamp}"
        );
    }
    Ok(out)
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
