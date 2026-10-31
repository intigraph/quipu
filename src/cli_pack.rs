//! `quipu pack` / `quipu unpack` — the knowledge-pack CLI (quipu #81/#82).
//!
//! Split from `cli_commands.rs` for the file-size ratchet; dispatch in
//! `main.rs` is unchanged apart from the module path.

use crate::cli::{chrono_now, flag_value};

mod archive;

pub use archive::{cmd_pack, cmd_restore, cmd_unpack};

/// `quipu share --output <dir>` — write a deterministic, git-native share.
pub fn cmd_share(args: &[String], db_path: &str) {
    // `--project [<id>]` (aegis-w3k75d.11): share THIS repository's project
    // graph to the committed location, writing .quipu/.gitignore first so the
    // store and signing key beside it can never be staged by `git add .quipu`.
    let project = project_flag(args).map(|id| {
        let root = std::path::Path::new(".");
        let id = quipu::project_graph::scaffold(root, id).unwrap_or_else(|e| {
            eprintln!("share --project: {e}");
            std::process::exit(1);
        });
        let next = quipu::project_graph::clear_next_bundle(root).unwrap_or_else(|e| {
            eprintln!("share --project: {e}");
            std::process::exit(1);
        });
        (quipu::project_graph::project_iri(&id), next)
    });
    if project.is_some()
        && (flag_value(args, "--output").is_some() || flag_value(args, "--since").is_some())
    {
        eprintln!("share --project writes .quipu/graph itself; --output and --since do not apply");
        std::process::exit(1);
    }
    let project_output = project
        .as_ref()
        .map(|(_, dir)| dir.to_string_lossy().into_owned());
    let output = flag_value(args, "--output")
        .or(project_output.as_deref())
        .unwrap_or_else(|| {
            eprintln!(
                "usage: quipu share --output <dir> [--graph <iri> | --group-id <id> | \
             --construct <query>] [--shapes <name>]... [--no-shapes] \
             [--queries <name>]... [--no-queries] [--alignments <file>] [--parent-share <sha256:id>] \
             [--since <parent-reference>] [--turtle] [--destination internal]"
            );
            std::process::exit(1);
        });
    let graph = flag_value(args, "--graph").or(project.as_ref().map(|(iri, _)| iri.as_str()));
    if project.is_some() && flag_value(args, "--graph").is_some() {
        eprintln!("share accepts --project or --graph, not both: --project names the graph");
        std::process::exit(1);
    }
    let group = flag_value(args, "--group-id");
    let construct = flag_value(args, "--construct");
    if [graph.is_some(), group.is_some(), construct.is_some()]
        .into_iter()
        .filter(|selected| *selected)
        .count()
        > 1
    {
        eprintln!("share accepts only one of --graph, --group-id, or --construct");
        std::process::exit(1);
    }
    let scope = match (graph, group, construct) {
        (Some(iri), None, None) => quipu::share::ShareScope::Graph(iri.into()),
        (None, Some(id), None) => quipu::share::ShareScope::Group(id.into()),
        (None, None, Some(query)) => quipu::share::ShareScope::Construct(query.into()),
        (None, None, None) => quipu::share::ShareScope::Root,
        _ => unreachable!("mutually exclusive share scopes checked above"),
    };
    let shapes = repeated(args, "--shapes");
    let no_shapes = args.iter().any(|arg| arg == "--no-shapes");
    if no_shapes && args.iter().any(|arg| arg == "--shapes") {
        eprintln!("share accepts either --shapes or --no-shapes, not both");
        std::process::exit(1);
    }
    // aegis-fxpbys.2: named queries, none, or (absent) those registered
    // against the scope.
    let queries = repeated(args, "--queries");
    let no_queries = args.iter().any(|arg| arg == "--no-queries");
    if no_queries && !queries.is_empty() {
        eprintln!("share accepts either --queries or --no-queries, not both");
        std::process::exit(1);
    }
    let alignments_turtle = flag_value(args, "--alignments").map(|path| {
        quipu::share_alignments::read_file(std::path::Path::new(path)).unwrap_or_else(|error| {
            eprintln!("share alignments: {error}");
            std::process::exit(1)
        })
    });
    let opts = quipu::share::ShareOptions {
        alignments_turtle,
        scope,
        shapes,
        no_shapes,
        queries: (no_queries || !queries.is_empty()).then_some(queries),
        parent_share: flag_value(args, "--parent-share").map(String::from),
        turtle_view: args.iter().any(|arg| arg == "--turtle"),
        // Stamp the new local default; explicit repository layouts still win.
        // Legacy manifests without this field keep their old qpack fallback.
        pack_dir: Some(
            flag_value(args, "--pack-dir")
                .unwrap_or(quipu::share::PENDANT_PACK_DIR)
                .into(),
        ),
        attest: attest_options(args),
        destination: destination_flag(args),
        pointers: pointers_flag(args),
    };
    let store = crate::cli_open::open_store(db_path);
    if let Some(parent) = flag_value(args, "--since") {
        match quipu::share_delta::write_delta(&store, parent, output, &opts) {
            Ok(manifest) => println!(
                "shared delta {} from {}",
                manifest.delta_id, manifest.parent_share
            ),
            Err(error) => {
                eprintln!("share delta error: {error}");
                std::process::exit(if matches!(error, quipu::Error::CannotVerify(_)) {
                    2
                } else {
                    1
                });
            }
        }
        return;
    }
    match quipu::share::share(&store, output, &opts).and_then(|manifest| {
        if project.is_some() {
            quipu::project_graph::install_next_bundle(std::path::Path::new("."))?;
        }
        Ok(manifest)
    }) {
        Ok(manifest) => {
            let output = if project.is_some() {
                "./.quipu/graph"
            } else {
                output
            };
            println!("shared {}", manifest.share_id);
            println!("  graph_hash: {}", manifest.graph_hash);
            println!("  tx_anchor:  {}", manifest.tx_anchor);
            println!("  output:     {output}");
        }
        Err(error) => {
            eprintln!("share error: {error}");
            std::process::exit(if matches!(error, quipu::Error::CannotVerify(_)) {
                2
            } else {
                1
            });
        }
    }
}

/// `quipu status <share-dir>` — report divergence from the share's parent.
pub fn cmd_status(args: &[String], db_path: &str) {
    let dir = args
        .get(2)
        .filter(|s| !s.starts_with("--"))
        .unwrap_or_else(|| {
            eprintln!("usage: quipu status <share-dir> [--db <path>]");
            std::process::exit(1);
        });
    let store = crate::cli_open::open_store(db_path);
    match quipu::share_merge::status(&store, std::path::Path::new(dir)) {
        Ok(result) => println!("{}", serde_json::to_string_pretty(&result).unwrap()),
        Err(error) => {
            eprintln!("status error: {error}");
            std::process::exit(1);
        }
    }
}

/// `quipu merge <share-dir>` — shape-aware three-way reconnect into ROOT.
///
/// With `--emit-decisions` it writes the conflicts to resolve (and, with
/// `--propose`, a mechanical proposal per row) and changes nothing; with
/// `--decisions` it finishes the merge from an operator's decided file
/// (aegis-yavo9c).
pub fn cmd_merge(args: &[String], db_path: &str) {
    if crate::cli_merge::cmd_merge_reconcile(args) {
        return;
    }
    let dir = args
        .get(2)
        .filter(|s| !s.starts_with("--"))
        .unwrap_or_else(|| {
            eprintln!("{}", crate::cli_merge::MERGE_USAGE);
            std::process::exit(1);
        });
    if crate::cli_merge::cmd_merge_decisions(args, db_path, dir) {
        return;
    }
    let mut store = crate::cli_open::open_store(db_path);
    match quipu::share_merge::merge(
        &mut store,
        std::path::Path::new(dir),
        &chrono_now(),
        flag_value(args, "--actor"),
    ) {
        Ok(result) => {
            println!("{}", serde_json::to_string_pretty(&result).unwrap());
            if result.outcome == "conflicts" {
                eprintln!(
                    "merge held: {} conflict(s), nothing was written. Reconcile with: quipu merge {dir} --emit-decisions <file.json> --propose, then quipu merge show <file.json>",
                    result.conflicts.len()
                );
                std::process::exit(2);
            }
        }
        Err(error) => {
            eprintln!("merge error: {error}");
            std::process::exit(1);
        }
    }
}

/// `quipu import <share-dir>` stages a verified share; promotion is separate.
/// `--project` with an optional id: `Some(Some(id))`, `Some(None)` (use the
/// committed id), or `None` when the flag is absent.
fn project_flag(args: &[String]) -> Option<Option<&str>> {
    let at = args.iter().position(|a| a == "--project")?;
    Some(
        args.get(at + 1)
            .map(String::as_str)
            .filter(|v| !v.starts_with("--")),
    )
}

/// `quipu load <bundle-dir>`: import a project bundle and promote it into its
/// own graph (aegis-w3k75d.11). One command for a fresh clone.
///
/// The import is the ordinary `import_share`, with every gate it has; only the
/// promotion differs. It targets the manifest's scope graph (never ROOT) and is
/// a diff, so a re-load after `git pull` changes only what changed.
pub fn cmd_load_bundle(args: &[String], db_path: &str) {
    let timestamp = chrono_now();
    let dir = &args[2];
    let actor = flag_value(args, "--actor");
    let result = quipu::share_transport::read_reference(dir).and_then(|mut request| {
        let target = match &request.manifest.scope {
            quipu::share::ShareScope::Graph(iri) => iri.clone(),
            other => {
                return Err(quipu::Error::InvalidValue(format!(
                    "{dir} is a share of scope {other:?}; `quipu load` loads a single-graph \
                     bundle (quipu share --project). Use `quipu import` for other shares."
                )));
            }
        };
        request.actor = actor.map(String::from);
        request.destination = destination_flag(args);
        request.source = flag_value(args, "--source").unwrap_or(dir).to_string();
        let mut store = crate::cli_open::open_store(db_path);
        let imported = quipu::share_import::import_share(&mut store, &request, &timestamp, actor)?;
        if !imported.promotion.eligible {
            return Err(quipu::Error::InvalidValue(format!(
                "the bundle imported into quarantine ({:?}); nothing was loaded into {target}. \
                 Inspect {} before promoting.",
                imported.promotion.blockers, imported.staging_graph
            )));
        }
        quipu::project_graph::promote_into_graph(
            &mut store,
            &imported.share_id,
            &imported.staging_graph,
            &target,
            &timestamp,
            actor,
        )
    });
    match result {
        Ok(loaded) => println!("{}", serde_json::to_string_pretty(&loaded).unwrap()),
        Err(error) => {
            eprintln!("load error: {error}");
            std::process::exit(1);
        }
    }
}

pub fn cmd_import(args: &[String], db_path: &str) {
    if args.get(2).map(String::as_str) == Some("review") {
        return crate::cli_share_review::run(args, db_path);
    }
    let timestamp = chrono_now();
    if args.get(2).map(String::as_str) == Some("delta") {
        let parent = args.get(3).unwrap_or_else(|| {
            eprintln!("usage: quipu import delta <parent-share> <delta-share>");
            std::process::exit(1);
        });
        let delta = args.get(4).unwrap_or_else(|| {
            eprintln!("usage: quipu import delta <parent-share> <delta-share>");
            std::process::exit(1);
        });
        warn_deprecated_extension(parent);
        warn_deprecated_extension(delta);
        let actor = flag_value(args, "--actor");
        let imported = quipu::share_delta::materialize(parent, delta).and_then(|mut request| {
            request.actor = actor.map(String::from);
            request.query_namespace = flag_value(args, "--query-namespace").map(String::from);
            let mut store = quipu::Store::open_in_memory()?;
            crate::cli_open::configure_embeddings(&mut store);
            quipu::share_import::import_share(&mut store, &request, &timestamp, actor)
        });
        match imported {
            Ok(result) => println!("{}", serde_json::to_string_pretty(&result).unwrap()),
            Err(error) => {
                eprintln!("delta import error: {error}");
                std::process::exit(1);
            }
        }
        return;
    }
    if args.get(2).map(String::as_str) == Some("promote") {
        let mut store = crate::cli_open::open_store(db_path);
        let share_id = args
            .get(3)
            .filter(|s| !s.starts_with("--"))
            .unwrap_or_else(|| {
                eprintln!(
                    "usage: quipu import promote <share-id> [--actor <id>] \
                     [--into <graph-iri> [--retire-from <graph-iri>] [--dry-run]] [--db <path>]"
                );
                std::process::exit(1);
            });
        let request = quipu::share_import::PromoteImportRequest {
            share_id: share_id.clone(),
            actor: flag_value(args, "--actor").map(String::from),
            target_graph: flag_value(args, "--into").map(String::from),
            retire_from: flag_value(args, "--retire-from").map(String::from),
            dry_run: args.iter().any(|a| a == "--dry-run"),
        };
        match quipu::share_import::promote_import(&mut store, &request, &timestamp, None) {
            Ok(result) => println!("{}", serde_json::to_string_pretty(&result).unwrap()),
            Err(error) => {
                eprintln!("import promotion error: {error}");
                std::process::exit(1);
            }
        }
        return;
    }
    let reference = args
        .get(2)
        .filter(|s| !s.starts_with("--"))
        .unwrap_or_else(|| {
            eprintln!(
                "usage: quipu import <share-dir|archive|URL> [--actor <id>] \
                 [--destination <audience-json>] [--query-namespace <ns>] [--replace-queries] \
                 [--db <path>]\n\
                 Stages and validates; ROOT is untouched. Next step: \
                 quipu import promote <share-id> [--actor <id>] [--db <path>]"
            );
            std::process::exit(1);
        });
    warn_deprecated_extension(reference);
    // aegis-zev: an `internal` stamp is read from the verified manifest, so
    // `--destination internal` on import is no longer needed. It is still
    // accepted, so existing scripts keep working. An audience declaration
    // (a JSON binding) is still the receiver's to make.
    if flag_value(args, "--destination") == Some("internal") {
        eprintln!(
            "note: --destination internal is not needed on import; \
             the share's destination stamp is read from its verified manifest"
        );
    }
    let actor = flag_value(args, "--actor");
    // Keep no-file archive/URL verification as the default, but an explicit
    // database selects the same local shapes, bindings and staging as a directory.
    let transient = flag_value(args, "--db").is_none()
        && (reference.starts_with("https://")
            || reference.starts_with("http://")
            || !std::path::Path::new(reference).is_dir());
    let imported = quipu::share_transport::read_reference(reference).and_then(|mut request| {
        request.actor = actor.map(String::from);
        request.destination = destination_flag(args);
        request.query_namespace = flag_value(args, "--query-namespace").map(String::from);
        request.replace_queries = args.iter().any(|arg| arg == "--replace-queries");
        request.source = flag_value(args, "--source")
            .unwrap_or(reference)
            .to_string();
        let mut store = if transient {
            quipu::Store::open_in_memory()?
        } else {
            crate::cli_open::open_store(db_path)
        };
        if transient {
            crate::cli_open::configure_embeddings(&mut store);
        }
        quipu::share_import::import_share(&mut store, &request, &timestamp, actor)
    });
    match imported {
        Ok(result) => {
            if result
                .destination
                .as_ref()
                .is_some_and(quipu::share::ShareDestination::is_internal)
            {
                eprintln!("internal share: unscrubbed identifiers, kept internal");
            }
            println!("{}", serde_json::to_string_pretty(&result).unwrap());
        }
        Err(error) => {
            eprintln!("import error: {error}");
            std::process::exit(1);
        }
    }
}

/// Every value of a repeatable `--flag <value>`.
fn repeated(args: &[String], flag: &str) -> Vec<String> {
    args.windows(2)
        .filter(|w| w[0] == flag)
        .map(|w| w[1].clone())
        .collect()
}

/// Read `--destination`, defaulting to outward (aegis-auw0o7).
///
/// `outward` is accepted explicitly as well as by omission, so a script can say
/// what it means; anything else EXITS rather than falling back. A typo such as
/// `--destination interal` silently defaulting to outward would be the benign
/// direction, but `--destination internal-only` silently defaulting to outward
/// on a payload the operator believed exempt is a refusal they will read as the
/// guard misfiring — and the fix they reach for is to look for a way round it.
/// Print the one-release `.qpack` -> `.pendant` rename notice to stderr, if
/// `reference` still uses the old name (aegis-fxpbys.3). Never fails the
/// command: the old name is an alias, not an error.
pub(crate) fn warn_deprecated_extension(reference: &str) {
    if let Some(notice) = quipu::share_transport::deprecated_extension_notice(reference) {
        eprintln!("{notice}");
    }
}

fn destination_flag(args: &[String]) -> quipu::share::ShareDestination {
    match flag_value(args, "--destination") {
        None | Some("outward") => quipu::share::ShareDestination::Outward,
        Some("internal") => quipu::share::ShareDestination::Internal,
        Some(other) => serde_json::from_str(other).unwrap_or_else(|_| {
            eprintln!("--destination needs outward, internal or an audience/store_id JSON object");
            std::process::exit(2);
        }),
    }
}

/// Build `--attest` options, or `None` when the flag is absent (aegis-tadzdf).
///
/// EVERY identity flag is required once `--attest` is given. A default agent or
/// session would put a name in a signed statement that the operator never chose,
/// and the whole value of the envelope is that it says who signed.
fn attest_options(args: &[String]) -> Option<quipu::share::AttestOptions> {
    if !args.iter().any(|a| a == "--attest") {
        return None;
    }
    let need = |name: &str| match flag_value(args, name) {
        Some(v) => v.to_string(),
        None => {
            eprintln!("--attest requires {name}");
            std::process::exit(2);
        }
    };
    let key_path = flag_value(args, "--attest-key")
        .map_or_else(quipu::signing::default_key_path, std::path::PathBuf::from);
    let issued: u64 = match flag_value(args, "--attest-issued-at") {
        Some(v) => v.parse().unwrap_or_else(|_| {
            eprintln!("--attest-issued-at must be seconds since the epoch");
            std::process::exit(2);
        }),
        None => {
            eprintln!(
                "--attest requires --attest-issued-at: a wall clock here would make two runs \
                 over one pinned dataset produce different signed bytes, and the share would \
                 not be re-derivable"
            );
            std::process::exit(2);
        }
    };
    let ttl: u64 = flag_value(args, "--attest-ttl")
        .and_then(|v| v.parse().ok())
        .unwrap_or(3600);
    Some(quipu::share::AttestOptions {
        key_path,
        agent: need("--attest-agent"),
        session: need("--attest-session"),
        introducer: need("--attest-introducer"),
        issued_at_epoch: issued,
        expires_at_epoch: issued + ttl,
        nonce: need("--attest-nonce"),
    })
}

/// `--pointers <file.toml>`: graphs this share names without carrying (aegis-3fy1kp).
///
/// The file holds `[[pointer]]` tables, as in the pendant autoload design:
///
/// ```toml
/// [[pointer]]
/// graph = "https://example.org/homelab"
/// kind = "endpoint"
/// endpoint = "https://quipu.example.org/sparql"
///
/// [[pointer]]
/// graph = "https://example.org/reference"
/// kind = "pack"
/// url = "https://example.org/reference.pendant.tar.gz"
/// sha256 = "<64 hex>"
/// ```
fn pointers_flag(args: &[String]) -> Vec<quipu::share::SharePointer> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct PointerFile {
        #[serde(default)]
        pointer: Vec<quipu::share::SharePointer>,
    }
    let Some(path) = flag_value(args, "--pointers") else {
        return Vec::new();
    };
    let parsed = std::fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|body| toml::from_str::<PointerFile>(&body).map_err(|e| e.to_string()));
    match parsed {
        Ok(file) if file.pointer.is_empty() => {
            eprintln!("--pointers {path}: no [[pointer]] entries");
            std::process::exit(1);
        }
        Ok(file) => file.pointer,
        Err(error) => {
            eprintln!("--pointers {path}: {error}");
            std::process::exit(1);
        }
    }
}
