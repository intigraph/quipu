//! `quipu merge` decision flags: emit, propose and apply (aegis-yavo9c).

use crate::cli::{chrono_now, flag_value};

pub(crate) const MERGE_USAGE: &str = "usage: quipu merge <share-dir> [--actor <id>] [--db <path>]\n\
       quipu merge show <decisions.json> [--format text|md|json]\n\
       quipu merge resolve <decisions.json> <row> --choose ours|theirs|base|both|value|defer [--value <term>]... --by <who>\n\
       quipu merge <share-dir> --emit-decisions <file.json> [--slot-scoped] [--propose] [--db <path>]\n\
       quipu merge <share-dir> --decisions <file.json> --reviewer <who> [--dry-run] [--actor <id>] [--db <path>]";

/// Handle `--emit-decisions` / `--decisions`; false when neither was given.
pub fn cmd_merge_decisions(args: &[String], db_path: &str, dir: &str) -> bool {
    if let Some(out) = flag_value(args, "--emit-decisions") {
        let store = crate::cli_open::open_store(db_path);
        let emit = if args.iter().any(|a| a == "--slot-scoped") {
            quipu::share_merge_decisions::emit_scoped
        } else {
            quipu::share_merge_decisions::emit
        };
        let emitted = emit(&store, std::path::Path::new(dir)).map(|file| {
            if args.iter().any(|a| a == "--propose") {
                quipu::share_merge_decisions::propose(file)
            } else {
                file
            }
        });
        match emitted.and_then(|file| {
            let body = serde_json::to_string_pretty(&file)
                .map_err(|e| quipu::Error::Serialization(e.to_string()))?;
            std::fs::write(out, body + "\n")
                .map_err(|e| quipu::Error::Store(format!("write {out}: {e}")))?;
            Ok(file.rows.len())
        }) {
            Ok(rows) => println!(
                "wrote {rows} decision row(s) to {out}; nothing was merged. Show them with: quipu merge show {out}\nDecide each row with: quipu merge resolve {out} <row> --choose <choice> --by <who>\nThen: quipu merge {dir} --decisions {out} --reviewer <who>"
            ),
            Err(error) => {
                eprintln!("merge error: {error}");
                std::process::exit(1);
            }
        }
        return true;
    }
    if let Some(path) = flag_value(args, "--decisions") {
        let Some(reviewer) = flag_value(args, "--reviewer") else {
            eprintln!("merge: --decisions requires --reviewer <who>\n{MERGE_USAGE}");
            std::process::exit(1);
        };
        let mut store = crate::cli_open::open_store(db_path);
        let applied = std::fs::read(path)
            .map_err(|e| quipu::Error::Store(format!("read {path}: {e}")))
            .and_then(|bytes| {
                let file = serde_json::from_slice(&bytes).map_err(|e| {
                    quipu::Error::InvalidValue(format!("{path} is not a decisions file: {e}"))
                })?;
                if args.iter().any(|a| a == "--dry-run") {
                    quipu::share_merge_decisions::dry_run(
                        &mut store,
                        std::path::Path::new(dir),
                        &file,
                        &bytes,
                        reviewer,
                    )
                } else {
                    quipu::share_merge_decisions::apply(
                        &mut store,
                        std::path::Path::new(dir),
                        &file,
                        &bytes,
                        reviewer,
                        &chrono_now(),
                        flag_value(args, "--actor"),
                    )
                }
            });
        match applied {
            Ok(result) => println!("{}", serde_json::to_string_pretty(&result).unwrap()),
            Err(error) => {
                eprintln!("merge error: {error}");
                std::process::exit(1);
            }
        }
        return true;
    }
    false
}

/// Every `--value` given, in order.
fn flag_values(args: &[String], name: &str) -> Vec<String> {
    args.windows(2)
        .filter(|w| w[0] == name)
        .map(|w| w[1].clone())
        .collect()
}

fn fail(error: impl std::fmt::Display) -> ! {
    eprintln!("merge error: {error}");
    std::process::exit(1);
}

fn read_file(path: &str) -> quipu::merge_reconcile::ReconcileFile {
    quipu::merge_reconcile::ReconcileFile::load_path(std::path::Path::new(path))
        .unwrap_or_else(|e| fail(e))
}

/// Print a decisions file in the standard layout.
pub(crate) fn print_view(file: &quipu::merge_reconcile::ReconcileFile, path: &str, format: &str) {
    use quipu::merge_reconcile::{Surface, render_markdown, render_text};
    let view = file.view(path, Surface::Cli);
    match format {
        "text" => print!("{}", render_text(&view)),
        "md" | "markdown" => print!("{}", render_markdown(&view)),
        "json" => println!("{}", serde_json::to_string_pretty(&view).unwrap()),
        other => fail(format!("unknown --format {other:?}: use text, md or json")),
    }
}

/// `quipu merge show|resolve`: the shared reconcile verbs (aegis-k7l).
/// Store-free: they read and edit a decisions file of any merge kind.
pub fn cmd_merge_reconcile(args: &[String]) -> bool {
    match args.get(2).map(String::as_str) {
        Some("show") => {
            let Some(path) = args.get(3).filter(|a| !a.starts_with("--")) else {
                fail(MERGE_USAGE)
            };
            let file = read_file(path);
            print_view(&file, path, flag_value(args, "--format").unwrap_or("text"));
            true
        }
        Some("resolve") => {
            let (Some(path), Some(row)) = (
                args.get(3).filter(|a| !a.starts_with("--")),
                args.get(4).filter(|a| !a.starts_with("--")),
            ) else {
                fail(MERGE_USAGE)
            };
            let Some(by) = flag_value(args, "--by") else {
                fail("resolve needs --by <who>: the person who chose, or your agent id")
            };
            let mut file = read_file(path);
            if let quipu::merge_reconcile::ReconcileFile::Git { dir, .. } = &file {
                // A Git choice rewrites the qpack: same verb, the Git applier.
                let Some(choice) = flag_value(args, "--choose") else {
                    fail("resolve needs --choose <choice>")
                };
                crate::cli_git_merge::resolve_in_merge(dir, row, choice, by);
                return true;
            }
            let choice = quipu::merge_reconcile::Choice::parse(
                flag_value(args, "--choose"),
                flag_values(args, "--value"),
            )
            .unwrap_or_else(|e| fail(e));
            file.resolve(row, choice, by, &chrono_now())
                .unwrap_or_else(|e| fail(e));
            let bytes = file.to_bytes().unwrap_or_else(|e| fail(e));
            write_atomic(path, &bytes).unwrap_or_else(|e| fail(e));
            print_view(&file, path, "text");
            true
        }
        _ => false,
    }
}

/// Replace `path` with `bytes` via a temp file in the same directory.
pub(crate) fn write_atomic(path: &str, bytes: &[u8]) -> quipu::Result<()> {
    let target = std::path::Path::new(path);
    let dir = target
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(dir)
        .map_err(|e| quipu::Error::Store(format!("write {path}: {e}")))?;
    std::io::Write::write_all(&mut temp, bytes)
        .map_err(|e| quipu::Error::Store(format!("write {path}: {e}")))?;
    temp.persist(target)
        .map_err(|e| quipu::Error::Store(format!("write {path}: {e}")))?;
    Ok(())
}
