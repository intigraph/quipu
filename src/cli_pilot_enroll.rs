//! `pilot enroll` (aegis-4qv.2): wraps `quipu::store::enroll_pilot_owner`.
//!
//! `--witness-config F` names a TOML file holding the server's
//! `[quipu.ingest_pilot]` table (or that table's keys at top level), so the
//! operator enrolls with exactly the pins the server will start with. The file
//! carries no secret: witness credentials come only from the owner-only
//! `credentials_file` it names, which the library reads and checks. The config
//! file itself must not be writable by group or other, because it pins the
//! witness endpoint, bucket and prefix.
//!
//! Dry run (the default) is the library's read-only preflight plus the planned
//! authority: no local file and no witness object is written. `--execute`
//! performs or resumes enrollment and proves it through enrolled startup.
use super::{Flags, Out, identity, parse, receipt};
use quipu::config::IngestPilotConfig;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// Upper bound on the witness configuration file.
const MAX_CONFIG_BYTES: u64 = 64 * 1024;

fn load_config(path: &Path) -> Result<IngestPilotConfig, String> {
    let meta = std::fs::symlink_metadata(path)
        .map_err(|_| "--witness-config cannot be inspected".to_string())?;
    if !meta.file_type().is_file() {
        return Err("--witness-config is a symlink or not a regular file".into());
    }
    if meta.permissions().mode() & 0o022 != 0 {
        return Err("--witness-config is writable by group or other".into());
    }
    if meta.len() > MAX_CONFIG_BYTES {
        return Err("--witness-config exceeds its size bound".into());
    }
    let text =
        std::fs::read_to_string(path).map_err(|_| "--witness-config cannot be read".to_string())?;
    let doc: toml::Table =
        toml::from_str(&text).map_err(|_| "--witness-config is not valid TOML".to_string())?;
    let table = match doc.get("quipu").and_then(|q| q.get("ingest_pilot")) {
        Some(pilot) => pilot.clone(),
        None if doc.contains_key("quipu") => {
            return Err("--witness-config has no [quipu.ingest_pilot] table".into());
        }
        None => toml::Value::Table(doc),
    };
    // Parse errors name the problem (missing or unknown key), never a value.
    table
        .try_into()
        .map_err(|e: toml::de::Error| format!("--witness-config: {}", e.message()))
}

pub(super) fn enroll(rest: &[String]) -> Out {
    let flags: Flags = parse(
        rest,
        &[
            "--db",
            "--journal",
            "--unit",
            "--principal",
            "--witness-config",
        ],
        &["--execute"],
    )?;
    // Absolute without resolving symlinks, so a bare `owner.journal` has a
    // parent directory and a symlinked `--db` is still refused as one.
    let absolute = |name: &str| {
        std::path::absolute(flags.get(name)?).map_err(|_| format!("{name} is not a usable path"))
    };
    let (db, journal) = (absolute("--db")?, absolute("--journal")?);
    let (db, journal) = (db.as_path(), journal.as_path());
    let unit = flags.get("--unit")?;
    let principal = flags.get("--principal")?;
    let config = load_config(Path::new(flags.get("--witness-config")?))?;
    let execute = flags.on("--execute");
    let inventory = journal.with_extension("inventory");
    let before = json!({
        "database": identity(db),
        "journal": identity(journal),
        "inventory": identity(&inventory),
    });
    let request = quipu::store::PilotEnroll {
        db,
        journal,
        unit,
        principal,
        execute,
    };
    let report = quipu::store::enroll_pilot_owner(&config, &request).map_err(|e| e.to_string())?;
    let after = json!({
        "database": identity(db),
        "journal": identity(journal),
        "inventory": identity(&inventory),
    });
    Ok(receipt(
        "enroll",
        execute,
        before,
        after,
        json!({"enrollment": report}),
    ))
}
