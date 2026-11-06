//! `quipu databook export|import`: graphs to and from DataBook documents.

use quipu::databook_store::{self, ExportOptions, ImportOptions};
use quipu::store::AsOf;

use crate::cli::{flag_value, resolve_timestamp};

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("error: {message}");
    std::process::exit(1);
}

pub fn cmd_databook(args: &[String], db_path: &str) {
    match args.get(2).map(String::as_str) {
        Some("export") => export(args, db_path),
        Some("import") => import(args, db_path),
        _ => {
            eprintln!(
                "usage: quipu databook export [--graph <iri>] [--tx N] [--valid-at <date>] \
                 [--with-shapes] [--id <iri>] [--title <t>] [--version <semver>] [--out <file>]\n       \
                 quipu databook import <file.databook.md> [--graph <iri>] [--with-shapes] \
                 [--actor <a>] [--source <s>] [--timestamp <t>]"
            );
            std::process::exit(1);
        }
    }
}

fn export(args: &[String], db_path: &str) {
    let tx = flag_value(args, "--tx").map(|v| {
        v.parse::<i64>()
            .unwrap_or_else(|_| fail(format!("--tx must be a transaction number, got {v}")))
    });
    let valid_at = flag_value(args, "--valid-at").map(str::to_string);
    let as_of = (tx.is_some() || valid_at.is_some()).then_some(AsOf { tx, valid_at });
    let timestamp = resolve_timestamp(args);
    let store = crate::cli_open::open_store(db_path);
    let opts = ExportOptions {
        graph: flag_value(args, "--graph"),
        as_of,
        id: flag_value(args, "--id"),
        title: flag_value(args, "--title"),
        version: flag_value(args, "--version").unwrap_or("1.0.0"),
        with_shapes: args.iter().any(|a| a == "--with-shapes"),
        timestamp: &timestamp,
    };
    let text = databook_store::export(&store, &opts).unwrap_or_else(|e| fail(e));
    match flag_value(args, "--out") {
        Some(path) => {
            std::fs::write(path, &text).unwrap_or_else(|e| fail(format!("writing {path}: {e}")));
            eprintln!("wrote {path}");
        }
        None => print!("{text}"),
    }
}

/// The first word after `databook import` that is neither a flag nor a flag's value.
fn operand(args: &[String]) -> Option<&str> {
    let mut words = args.iter().skip(3);
    while let Some(word) = words.next() {
        if word == "--with-shapes" {
            continue;
        }
        if word.starts_with("--") {
            words.next();
            continue;
        }
        return Some(word);
    }
    None
}

fn import(args: &[String], db_path: &str) {
    let path = operand(args).unwrap_or_else(|| fail("databook import needs a file"));
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|e| fail(format!("reading {path}: {e}")));
    let timestamp = resolve_timestamp(args);
    let mut store = crate::cli_open::open_store(db_path);
    let opts = ImportOptions {
        graph: flag_value(args, "--graph"),
        with_shapes: args.iter().any(|a| a == "--with-shapes"),
        timestamp: &timestamp,
        actor: flag_value(args, "--actor"),
        source: flag_value(args, "--source"),
    };
    let report = databook_store::import(&mut store, &text, &opts).unwrap_or_else(|e| fail(e));
    for warning in &report.warnings {
        eprintln!("warning: {warning}");
    }
    let graphs: Vec<_> = report
        .graphs
        .iter()
        .map(|(graph, tx, facts)| serde_json::json!({"graph": graph, "tx": tx, "facts": facts}))
        .collect();
    println!(
        "{}",
        serde_json::json!({
            "id": report.id,
            "version": report.version,
            "graphs": graphs,
            "shapes": report.shapes,
            "skipped": report.skipped,
            "warnings": report.warnings,
        })
    );
}
