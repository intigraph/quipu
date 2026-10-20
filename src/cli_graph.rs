//! `quipu graph <import|freeze|thaw|list>` — the graph-registry CLI.
//!
//! `import` predates this module (quipu #85, moved here from `cli_commands`
//! for the file-size ratchet); `freeze`/`thaw`/`list` are the deep-freeze
//! surface (`docs/design/graph-kinds-and-deep-freeze.md`).

use crate::cli::{chrono_now, flag_value};

pub fn cmd_graph(args: &[String], db_path: &str) {
    match args.get(2).map(String::as_str) {
        Some("import") => cmd_import(args, db_path),
        Some("create") => cmd_register(args, db_path, false),
        Some("label") => cmd_register(args, db_path, true),
        Some("overlay-create" | "overlay-write" | "overlay-compose") => cmd_overlay(args, db_path),
        Some("freeze") => cmd_freeze(args, db_path),
        Some("thaw") => cmd_thaw(args, db_path),
        Some("list") => cmd_list(args, db_path),
        _ => {
            eprintln!(
                "usage: quipu graph create <iri> [--actor <who>] [--timestamp <utc>] [--db <path>]\n       \
                 quipu graph label <iri> --label '<JSON label fields>' [--actor <who>] [--timestamp <utc>] [--db <path>]\n       \
                 quipu graph overlay-create|overlay-write|overlay-compose --input '<JSON tool input>' [--db <path>]\n       \
                 quipu graph import <db> --as <iri> [--db <path>]\n       \
                 quipu graph freeze <iri> [--out <dir>] [--actor <who>] [--db <path>]\n       \
                 quipu graph thaw <iri> [--actor <who>] [--db <path>]\n       \
                 quipu graph list [--graph <iri>] [--kind <token>] [--frozen] [--json] [--db <path>]"
            );
            std::process::exit(1);
        }
    }
}

fn cmd_overlay(args: &[String], db_path: &str) {
    let input = flag_value(args, "--input")
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| {
            eprintln!("graph overlay verbs require --input '<JSON object>'; fields match the corresponding MCP/REST tool");
            std::process::exit(1);
        });
    let mut store = open_store(db_path);
    let result = match args[2].as_str() {
        "overlay-create" => quipu::tool_overlay_create(&store, &input),
        "overlay-write" => quipu::tool_overlay_write(&mut store, &input),
        "overlay-compose" => quipu::tool_overlay_compose(&store, &input),
        _ => unreachable!(),
    };
    match result {
        Ok(value) => println!("{value}"),
        Err(error) => {
            eprintln!("{} error: {error}", args[2]);
            std::process::exit(1);
        }
    }
}

/// Both CLI verbs enter the exact library handlers used by REST and MCP.
fn cmd_register(args: &[String], db_path: &str, label: bool) {
    let Some(graph) = args.get(3).filter(|s| !s.starts_with("--")) else {
        eprintln!("graph create/label requires a graph IRI; see quipu graph --help");
        std::process::exit(1);
    };
    let mut input = if label {
        let raw = flag_value(args, "--label").unwrap_or_else(|| {
            eprintln!(
                "graph label requires --label '<JSON object>', e.g. '{{\"kind\":\"operational\"}}'"
            );
            std::process::exit(1);
        });
        match serde_json::from_str::<serde_json::Value>(raw) {
            Ok(value) if value.is_object() => value,
            _ => {
                eprintln!("--label must be a JSON object");
                std::process::exit(1);
            }
        }
    } else {
        serde_json::json!({})
    };
    input["graph"] = serde_json::json!(graph);
    input["timestamp"] =
        serde_json::json!(flag_value(args, "--timestamp").map_or_else(chrono_now, str::to_owned));
    if let Some(actor) = flag_value(args, "--actor") {
        input["actor"] = serde_json::json!(actor);
    }
    let mut store = open_store(db_path);
    let result = if label {
        quipu::tool_graph_label(&mut store, &input)
    } else {
        quipu::tool_graph_create(&store, &input)
    };
    match result {
        Ok(value) => println!("{value}"),
        Err(error) => {
            eprintln!(
                "graph {} error: {error}",
                if label { "label" } else { "create" }
            );
            std::process::exit(1);
        }
    }
}

fn cmd_import(args: &[String], db_path: &str) {
    let Some(source) = args.get(3).filter(|s| !s.starts_with("--")) else {
        eprintln!("quipu graph import requires a source database");
        std::process::exit(1);
    };
    let Some(graph) = flag_value(args, "--as") else {
        eprintln!("quipu graph import requires --as <iri>");
        std::process::exit(1);
    };
    match quipu::store::import::import_graph(
        std::path::Path::new(db_path),
        std::path::Path::new(source),
        graph,
    ) {
        Ok(r) => println!(
            "imported {source} as {graph}\n  graph id: {}\n  terms: {}\n  transactions: {}\n  facts: {}\n  vectors: {}",
            r.graph, r.terms, r.transactions, r.facts, r.vectors
        ),
        Err(e) => {
            eprintln!("graph import error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_freeze(args: &[String], db_path: &str) {
    let Some(iri) = args.get(3).filter(|s| !s.starts_with("--")) else {
        eprintln!("usage: quipu graph freeze <iri> [--out <dir>] [--actor <who>] [--db <path>]");
        std::process::exit(1);
    };
    let out_dir = flag_value(args, "--out").map_or_else(
        || {
            std::path::Path::new(db_path)
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| std::path::Path::new("."))
                .to_string_lossy()
                .to_string()
        },
        std::string::ToString::to_string,
    );
    let mut store = open_store(db_path);
    match store.freeze_graph(iri, &out_dir, &chrono_now(), flag_value(args, "--actor")) {
        Ok(r) => {
            println!(
                "froze {iri}\n  pack: {}\n  alias: {}\n  hash: {}\n  facts: {} (full history)\n  transactions: {}\n  vectors: {}\n  note: as_of_tx time travel is refused while archives are attached; \
                 valid-time queries and thaw remain available",
                r.path, r.alias, r.content_hash, r.facts, r.transactions, r.vectors
            );
            // Printed on stderr, not folded into the counts above: an
            // archive that could not carry its embeddings is incomplete in a
            // way the operator has to see.
            if let Some(why) = &r.vectors_omitted {
                eprintln!("warning: no entity embeddings were carried into the archive: {why}");
            }
        }
        Err(e) => {
            eprintln!("freeze error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_thaw(args: &[String], db_path: &str) {
    let Some(iri) = args.get(3).filter(|s| !s.starts_with("--")) else {
        eprintln!("usage: quipu graph thaw <iri> [--actor <who>] [--db <path>]");
        std::process::exit(1);
    };
    let mut store = open_store(db_path);
    match store.thaw_graph(iri, &chrono_now(), flag_value(args, "--actor")) {
        Ok((facts, vectors)) => println!(
            "thawed {iri}\n  facts restored: {facts}\n  vectors restored: {vectors}\n  pack file kept on disk"
        ),
        Err(e) => {
            eprintln!("thaw error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_list(args: &[String], db_path: &str) {
    let store = if args.iter().any(|a| a == "--graph") {
        quipu::config::resolve_attachments(&crate::cli_open::config().attachments)
            .and_then(|attachments| quipu::Store::open_graph_metadata_reader(db_path, &attachments))
            .unwrap_or_else(|error| {
                eprintln!("graph metadata reader: {error}");
                std::process::exit(1)
            })
    } else {
        open_store(db_path)
    };
    let lifecycle = args.iter().any(|a| a == "--frozen").then_some("frozen");
    if args.iter().any(|a| a == "--json" || a == "--graph") {
        let mut input = serde_json::json!({});
        if args.iter().any(|a| a == "--graph") {
            let Some(graph) = flag_value(args, "--graph") else {
                eprintln!("graph list requires a value for --graph");
                std::process::exit(1);
            };
            input["graph"] = serde_json::json!(graph);
        }
        if let Some(kind) = flag_value(args, "--kind") {
            input["kind"] = serde_json::json!(kind);
        }
        if let Some(lifecycle) = lifecycle {
            input["lifecycle"] = serde_json::json!(lifecycle);
        }
        match quipu::tool_graph_list(&store, &input) {
            Ok(value) => println!("{value}"),
            Err(error) => {
                eprintln!("graph list error: {error}");
                std::process::exit(1);
            }
        }
        return;
    }
    match store.list_graphs(flag_value(args, "--kind"), lifecycle) {
        Ok(graphs) => {
            for g in graphs {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    g.iri,
                    g.class,
                    g.kind.as_deref().unwrap_or("-"),
                    g.lifecycle.as_deref().unwrap_or("-"),
                    g.source.as_deref().unwrap_or("local"),
                );
            }
        }
        Err(e) => {
            eprintln!("graph list error: {e}");
            std::process::exit(1);
        }
    }
}

fn open_store(db_path: &str) -> quipu::Store {
    crate::cli_open::open_store(db_path)
}
