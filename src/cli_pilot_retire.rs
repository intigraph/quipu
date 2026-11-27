//! `pilot retire` (aegis-4qv.4): wraps `quipu::store::retire_pilot_owner`.
//!
//! Retires the reversed pilot owner's witness authority to the legacy ROOT
//! placement: witness reserve, journal Retiring, witness retire, journal
//! Retired. No activation. Dry run (the default) reports the planned request
//! and writes nothing. The retirement is unsigned; the report says so.
use super::{Flags, Out, identity, parse, receipt};
use serde_json::json;
use std::path::Path;

fn digest(flags: &Flags, name: &str) -> Result<[u8; 32], String> {
    let bytes = hex::decode(flags.get(name)?).map_err(|_| format!("{name} must be hex"))?;
    bytes
        .try_into()
        .map_err(|_| format!("{name} must be a 32-byte SHA-256 digest"))
}

pub(super) fn retire(rest: &[String]) -> Out {
    let flags: Flags = parse(
        rest,
        &[
            "--journal",
            "--legacy-db",
            "--root-principal",
            "--operator",
            "--transaction",
            "--facts",
            "--artifact",
            "--witness-config",
        ],
        &["--execute"],
    )?;
    let absolute = |name: &str| {
        std::path::absolute(flags.get(name)?).map_err(|_| format!("{name} is not a usable path"))
    };
    let (journal, legacy) = (absolute("--journal")?, absolute("--legacy-db")?);
    let config = super::enroll::load_config(Path::new(flags.get("--witness-config")?))?;
    let execute = flags.on("--execute");
    let request = quipu::store::PilotRetire {
        journal: &journal,
        legacy_db: &legacy,
        root_principal: flags.get("--root-principal")?,
        operator: flags.get("--operator")?,
        transaction: flags.number("--transaction")?,
        facts: digest(&flags, "--facts")?,
        artifact: digest(&flags, "--artifact")?,
        execute,
    };
    let before = json!({"journal": identity(&journal), "legacy": identity(&legacy)});
    let report = quipu::store::retire_pilot_owner(&config, &request).map_err(|e| e.to_string())?;
    let after = json!({"journal": identity(&journal), "legacy": identity(&legacy)});
    Ok(receipt(
        "retire",
        execute,
        before,
        after,
        json!({"retirement": report}),
    ))
}
