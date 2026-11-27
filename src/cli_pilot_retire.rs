//! `pilot retire` (aegis-4qv.4): wraps `quipu::store::retire_pilot_owner`.
//!
//! Retires the reversed pilot owner's witness authority to the legacy ROOT
//! placement: witness reserve, journal Retiring, witness retire, journal
//! Retired. No activation. Dry run (the default) reports the planned request
//! and writes nothing. The retirement is unsigned; the report says so.
use super::{Flags, Out, identity, parse, receipt};
use quipu::graph_catalog::Catalog;
use serde_json::json;
use std::path::Path;

fn catalog(flags: &Flags, name: &str) -> Result<Catalog, String> {
    Catalog::load_file(Path::new(flags.get(name)?)).map_err(|e| format!("{name}: {e}"))
}

pub(super) fn retire(rest: &[String]) -> Out {
    let flags: Flags = parse(
        rest,
        &[
            "--journal",
            "--legacy-db",
            "--root-principal",
            "--operator",
            "--owner-file",
            "--catalog",
            "--next-catalog",
            "--witness-config",
        ],
        &["--execute"],
    )?;
    let absolute = |name: &str| {
        std::path::absolute(flags.get(name)?).map_err(|_| format!("{name} is not a usable path"))
    };
    let (journal, legacy) = (absolute("--journal")?, absolute("--legacy-db")?);
    let owner = absolute("--owner-file")?;
    let (current, next) = (
        catalog(&flags, "--catalog")?,
        catalog(&flags, "--next-catalog")?,
    );
    let config = super::enroll::load_config(Path::new(flags.get("--witness-config")?))?;
    let execute = flags.on("--execute");
    let request = quipu::store::PilotRetire {
        journal: &journal,
        legacy_db: &legacy,
        root_principal: flags.get("--root-principal")?,
        operator: flags.get("--operator")?,
        owner_db: &owner,
        current: &current,
        next: &next,
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
