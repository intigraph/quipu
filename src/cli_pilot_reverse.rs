//! `pilot reverse`: fenced reverse reconciliation of one routed graph back to
//! the legacy file (aegis-4qv.3, DESIGN v2), wrapping
//! `Store::reverse_routed_graph`.
//!
//! The operator first publishes catalog revision R+1 with `pilot catalog
//! --write`; this verb never writes a catalog. It validates R → R+1, takes the
//! owner file's write lock for the whole run (an owner still running refuses),
//! exports the owner's full graph read-only, then reports (dry run) or drives
//! (`--execute`) the journaled legacy steps. A resumed run prints the same
//! receipt as an uninterrupted one.
use super::{Flags, Out, identity, open_ro, open_rw, parse, receipt};
use quipu::graph_catalog::Catalog;
use quipu::store::{ReversePlan, ReverseReceipt, lock_owner_file, validate_reverse_catalogs};
use serde_json::json;
use std::path::Path;

fn existing(flags: &Flags, name: &str) -> Result<String, String> {
    let path = flags.get(name)?;
    if !Path::new(path).is_file() {
        return Err(format!("{name} must name an existing file"));
    }
    Ok(path.to_string())
}

fn receipt_json(r: &ReverseReceipt) -> serde_json::Value {
    json!({
        "graph": r.graph,
        "from": {"catalog_revision": r.from.0, "generation": r.from.1},
        "to": {"catalog_revision": r.to.0, "generation": r.to.1, "route": "legacy"},
        "state": r.state.as_str(),
        "owner_facts": r.owner_facts,
        "sources": r.sources,
        "multiset_sha256": r.multiset_sha256,
        "at": r.at,
        "witness_state": r.witness_state,
    })
}

pub(super) fn reverse(rest: &[String]) -> Out {
    let flags = parse(
        rest,
        &[
            "--db",
            "--owner-file",
            "--graph",
            "--revision",
            "--generation",
            "--catalog",
            "--next-catalog",
        ],
        &["--execute"],
    )?;
    let db = existing(&flags, "--db")?;
    let owner = existing(&flags, "--owner-file")?;
    if std::fs::canonicalize(&db).ok() == std::fs::canonicalize(&owner).ok() {
        return Err("--db and --owner-file must be different files".into());
    }
    let plan = ReversePlan {
        graph: flags.get("--graph")?,
        catalog_revision: flags.number("--revision")?,
        generation: flags.number("--generation")?,
    };
    let current = Catalog::load_file(Path::new(&existing(&flags, "--catalog")?))
        .map_err(|e| format!("--catalog: {e}"))?;
    let next = Catalog::load_file(Path::new(&existing(&flags, "--next-catalog")?))
        .map_err(|e| format!("--next-catalog: {e}"))?;
    validate_reverse_catalogs(&current, &next, &plan).map_err(|e| e.to_string())?;
    let execute = flags.on("--execute");
    let before = json!({"legacy": identity(Path::new(&db)), "owner": identity(Path::new(&owner))});
    // Held until the end of the run: no owner commit can land meanwhile.
    let lock = lock_owner_file(&owner).map_err(|e| e.to_string())?;
    let facts = open_ro(&owner)?
        .export_routed_graph(plan.graph)
        .map_err(|e| e.to_string())?;
    let result = if execute {
        open_rw(&db)?.reverse_routed_graph(&plan, &facts, &quipu::time::now_iso(), true)
    } else {
        open_ro(&db)?.reverse_receipt(&plan, &facts)
    }
    .map_err(|e| e.to_string())?;
    drop(lock);
    let after = json!({"legacy": identity(Path::new(&db)), "owner": identity(Path::new(&owner))});
    Ok(receipt(
        "reverse",
        execute,
        before,
        after,
        json!({"reverse": receipt_json(&result), "catalog_next_revision": next.revision()}),
    ))
}
