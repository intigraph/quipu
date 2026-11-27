//! `quipu pilot …` — file-level operator verbs for the per-graph ingest pilot
//! (aegis-4qv). Built only with `ownership-witness-s3`, like the server pilot;
//! the default build never sees the word `pilot`.
//!
//! Every verb wraps an existing library API and adds no semantics:
//! `fence` → `Store::fence_routed_graph`, `fence --verify` →
//! `Store::verify_routed_fence`, `export-slice` → `Store::export_routed_slice`, `import-slice` →
//! `Store::import_routed_slice`,
//! `cutover` → `Store::cut_over_routed_source`, `catalog` →
//! `Catalog::from_json` / `Catalog::load_file`, `enroll` →
//! `quipu::store::enroll_pilot_owner`, `reverse` →
//! `Store::reverse_routed_graph`, `retire` → `quipu::store::retire_pilot_owner`.
//! Write verbs default to a dry
//! run that opens the file read-only; `--execute` writes. Every verb prints one
//! JSON receipt with the target file identity before and after.
//!
//! No verb contacts a server or reads an environment variable. Flags are an
//! exact allow-list per verb, so a secret passed on argv refuses before any
//! file is opened.
use quipu::Store;
use quipu::graph_catalog::Catalog;
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

#[path = "cli_pilot_enroll.rs"]
mod enroll;
#[path = "cli_pilot_slice.rs"]
mod slice;

#[path = "cli_pilot_retire.rs"]
mod retire;
#[path = "cli_pilot_reverse.rs"]
mod reverse;

type Out = Result<Json, String>;

pub fn run(args: &[String]) {
    let verb = args.get(2).map_or("", String::as_str);
    let rest = args.get(3..).unwrap_or(&[]);
    let result = match verb {
        "fence" => fence(rest),
        "export-slice" => slice::export_slice(rest),
        "import-slice" => slice::import_slice(rest),
        "cutover" => cutover(rest),
        "catalog" => catalog(rest),
        "enroll" => enroll::enroll(rest),
        "reverse" => reverse::reverse(rest),
        "retire" => retire::retire(rest),
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(if verb == "--help" || verb == "-h" {
                0
            } else {
                2
            });
        }
    };
    match result {
        Ok(receipt) => println!("{}", serde_json::to_string_pretty(&receipt).unwrap()),
        Err(why) => {
            eprintln!("error: pilot {verb} refused: {why}");
            std::process::exit(1);
        }
    }
}

const USAGE: &str = "\
usage: quipu pilot <verb> [--execute]   (dry run unless --execute)
  pilot fence --db LEGACY --graph IRI --revision N --generation N [--execute]
  pilot fence --verify --db LEGACY
  pilot export-slice --db LEGACY --tag snapshot:KEY --graph IRI --out FILE [--execute]
  pilot import-slice --owner-file OWNER --in FILE [--into IRI --legacy-db LEGACY] [--at RFC3339] [--execute]
  pilot cutover --db LEGACY --tag snapshot:KEY --graph IRI --revision N --generation N [--at RFC3339] [--execute]
  pilot catalog --validate FILE
  pilot catalog --write FILE --from INPUT [--execute]
  pilot enroll --db DB --journal J --unit full-store --principal P --witness-config F [--execute]
  pilot reverse --db LEGACY --owner-file OWNER --graph IRI --revision R --generation G --catalog CAT_R --next-catalog CAT_R1 [--execute]
  pilot retire --journal J --legacy-db LEGACY --root-principal P --operator O --transaction N --facts HEX --artifact HEX --witness-config F [--execute]";

/// Parse `rest` against an exact allow-list. Values never echo into errors.
pub(super) struct Flags {
    values: Vec<(&'static str, String)>,
    switches: Vec<&'static str>,
}

const SECRET_FLAGS: &[&str] = &[
    "--access-key-id",
    "--secret-access-key",
    "--access-key",
    "--secret",
    "--credentials",
    "--password",
    "--token",
];

fn parse(
    rest: &[String],
    values: &[&'static str],
    switches: &[&'static str],
) -> Result<Flags, String> {
    let mut flags = Flags {
        values: Vec::new(),
        switches: Vec::new(),
    };
    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        let name = arg.split('=').next().unwrap_or(arg);
        if SECRET_FLAGS.contains(&name) {
            return Err("secrets are never accepted on argv; use a 0600 credentials_file".into());
        }
        if let Some(s) = switches.iter().find(|s| **s == arg) {
            flags.switches.push(s);
            i += 1;
        } else if let Some(v) = values.iter().find(|v| **v == arg) {
            let value = rest
                .get(i + 1)
                .ok_or_else(|| format!("{v} needs a value"))?;
            if flags.values.iter().any(|(n, _)| n == v) {
                return Err(format!("{v} given twice"));
            }
            flags.values.push((v, value.clone()));
            i += 2;
        } else {
            return Err(format!("unknown argument `{name}` for this verb"));
        }
    }
    Ok(flags)
}

impl Flags {
    fn get(&self, name: &str) -> Result<&str, String> {
        self.opt(name).ok_or_else(|| format!("{name} is required"))
    }
    fn opt(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }
    fn number(&self, name: &str) -> Result<u64, String> {
        self.get(name)?
            .parse()
            .map_err(|_| format!("{name} must be a non-negative integer"))
    }
    fn on(&self, name: &str) -> bool {
        self.switches.contains(&name)
    }
}

fn sha256_file(path: &Path) -> Option<String> {
    std::fs::read(path)
        .ok()
        .map(|b| hex::encode(Sha256::digest(b)))
}

/// Identity of a file and its SQLite WAL sidecar, for before/after receipts.
fn identity(path: &Path) -> Json {
    let wal = Path::new(&format!("{}-wal", path.display())).to_path_buf();
    match std::fs::metadata(path) {
        Ok(meta) => json!({
            "path": path.display().to_string(),
            "exists": true,
            "device": meta.dev(),
            "inode": meta.ino(),
            "len": meta.len(),
            "mode": format!("{:o}", meta.permissions().mode() & 0o7777),
            "sha256": sha256_file(path),
            // A reader may leave an EMPTY `-wal` behind; it holds no frames,
            // so it is the same database state as no sidecar at all.
            "wal_sha256": std::fs::metadata(&wal)
                .is_ok_and(|m| m.len() > 0)
                .then(|| sha256_file(&wal))
                .flatten(),
        }),
        Err(_) => json!({"path": path.display().to_string(), "exists": false}),
    }
}

fn existing_db(flags: &Flags) -> Result<&str, String> {
    let db = flags.get("--db")?;
    if !Path::new(db).is_file() {
        return Err("--db must name an existing database file".into());
    }
    Ok(db)
}

fn open_ro(db: &str) -> Result<Store, String> {
    Store::open_read_only(db).map_err(|e| e.to_string())
}

fn open_rw(db: &str) -> Result<Store, String> {
    Store::open(db).map_err(|e| e.to_string())
}

fn receipt(verb: &str, execute: bool, before: Json, after: Json, body: Json) -> Json {
    let mut out = serde_json::Map::new();
    out.insert("verb".into(), json!(verb));
    out.insert(
        "mode".into(),
        json!(if execute { "execute" } else { "dry-run" }),
    );
    out.insert("before".into(), before);
    out.insert("after".into(), after);
    if let Json::Object(b) = body {
        out.extend(b);
    }
    Json::Object(out)
}

fn fence_state(store: &Store) -> Result<Json, String> {
    let graphs = store
        .routed_graph_fence_rows()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|(iri, revision, generation)| {
            json!({"graph": iri, "catalog_revision": revision, "generation": generation})
        })
        .collect::<Vec<_>>();
    let sources = store
        .routed_source_fence_rows()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|(tag, graph, revision, generation)| {
            json!({"tag": tag, "graph": graph, "catalog_revision": revision, "generation": generation})
        })
        .collect::<Vec<_>>();
    Ok(json!({"graphs": graphs, "sources": sources}))
}

fn fence(rest: &[String]) -> Out {
    let flags = parse(
        rest,
        &["--db", "--graph", "--revision", "--generation"],
        &["--verify", "--execute"],
    )?;
    let db = existing_db(&flags)?;
    let path = Path::new(db);
    let before = identity(path);
    if flags.on("--verify") {
        if flags.on("--execute") || flags.opt("--graph").is_some() {
            return Err("--verify is read-only and takes only --db".into());
        }
        let store = open_ro(db)?;
        let columns = store.verify_routed_fence().map_err(|e| e.to_string())?;
        let state = fence_state(&store)?;
        drop(store);
        return Ok(receipt(
            "fence --verify",
            false,
            before,
            identity(path),
            json!({
                "verified": true,
                "fenced_columns": columns.len(),
                "fence": state,
            }),
        ));
    }
    let graph = flags.get("--graph")?;
    let revision = flags.number("--revision")?;
    let generation = flags.number("--generation")?;
    let execute = flags.on("--execute");
    let plan = json!({"graph": graph, "catalog_revision": revision, "generation": generation});
    if !execute {
        let store = open_ro(db)?;
        let inventory = store.routed_fence_inventory().map_err(|e| e.to_string())?;
        let state = fence_state(&store)?;
        drop(store);
        return Ok(receipt(
            "fence",
            false,
            before,
            identity(path),
            json!({
                "plan": plan, "fence_columns": inventory.len(), "fence": state,
            }),
        ));
    }
    let store = open_rw(db)?;
    store
        .fence_routed_graph(graph, revision, generation)
        .map_err(|e| e.to_string())?;
    store.verify_routed_fence().map_err(|e| e.to_string())?;
    let state = fence_state(&store)?;
    drop(store);
    Ok(receipt(
        "fence",
        true,
        before,
        identity(path),
        json!({"plan": plan, "fence": state}),
    ))
}

fn write_new_0600(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| format!("cannot create {}: {e}", path.display()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn cutover(rest: &[String]) -> Out {
    let flags = parse(
        rest,
        &[
            "--db",
            "--tag",
            "--graph",
            "--revision",
            "--generation",
            "--at",
        ],
        &["--execute"],
    )?;
    let db = existing_db(&flags)?;
    let path = Path::new(db);
    let (tag, graph) = (flags.get("--tag")?, flags.get("--graph")?);
    let revision = flags.number("--revision")?;
    let generation = flags.number("--generation")?;
    let execute = flags.on("--execute");
    let before = identity(path);
    // The library refuses an unfenced or mismatched cutover itself; the dry
    // run reads the same fence rows so its plan refuses the same inputs.
    let store = open_ro(db)?;
    let fenced = store
        .routed_graph_fence_rows()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|(iri, _, _)| iri == graph);
    let Some((_, have_revision, have_generation)) = fenced else {
        return Err("graph is not fenced in this file; run `pilot fence` first".into());
    };
    if (have_revision, have_generation) != (revision, generation) {
        return Err("fence revision/generation differ from --revision/--generation".into());
    }
    let planned = store
        .export_routed_slice(tag, quipu::schema::ROOT_GRAPH_IRI)
        .map_err(|e| e.to_string())?
        .len();
    drop(store);
    let plan = json!({
        "tag": tag, "graph": graph, "catalog_revision": revision,
        "generation": generation, "root_facts_to_retract": planned,
    });
    if !execute {
        return Ok(receipt(
            "cutover",
            false,
            before,
            identity(path),
            json!({"plan": plan}),
        ));
    }
    let at = flags
        .opt("--at")
        .map_or_else(quipu::time::now_iso, str::to_string);
    let mut store = open_rw(db)?;
    let retracted = store
        .cut_over_routed_source(tag, graph, revision, generation, &at)
        .map_err(|e| e.to_string())?;
    let state = fence_state(&store)?;
    drop(store);
    Ok(receipt(
        "cutover",
        true,
        before,
        identity(path),
        json!({
            "plan": plan, "retracted": retracted, "at": at, "fence": state,
        }),
    ))
}

fn catalog_summary(catalog: &Catalog) -> Json {
    json!({
        "revision": catalog.revision(),
        "routes": catalog.routes().map(|r| json!({
            "graph": r.graph, "file_id": r.file_id, "generation": r.generation, "policy": r.policy,
        })).collect::<Vec<_>>(),
    })
}

fn catalog(rest: &[String]) -> Out {
    let flags = parse(rest, &["--validate", "--write", "--from"], &["--execute"])?;
    match (flags.opt("--validate"), flags.opt("--write")) {
        (Some(file), None) => {
            if flags.opt("--from").is_some() || flags.on("--execute") {
                return Err("--validate is read-only and takes only the file".into());
            }
            let path = Path::new(file);
            let before = identity(path);
            let catalog = Catalog::load_file(path).map_err(|e| e.to_string())?;
            Ok(receipt(
                "catalog --validate",
                false,
                before,
                identity(path),
                json!({
                    "valid": true, "catalog": catalog_summary(&catalog),
                }),
            ))
        }
        (None, Some(file)) => {
            let path = Path::new(file);
            if path.exists() {
                return Err(
                    "--write target already exists; publish a new file per revision".into(),
                );
            }
            let input = flags.get("--from")?;
            let bytes = std::fs::read(input).map_err(|e| format!("cannot read --from: {e}"))?;
            let catalog = Catalog::from_json(&bytes).map_err(|e| e.to_string())?;
            let execute = flags.on("--execute");
            let before = identity(path);
            if execute {
                write_new_0600(path, &bytes)?;
                // Read back through the same loader the server uses.
                Catalog::load_file(path).map_err(|e| e.to_string())?;
            }
            Ok(receipt(
                "catalog --write",
                execute,
                before,
                identity(path),
                json!({
                    "catalog": catalog_summary(&catalog),
                }),
            ))
        }
        _ => Err("give exactly one of --validate FILE or --write FILE".into()),
    }
}
