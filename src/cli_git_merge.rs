//! Store-free Git transport commands; dispatched before loading store configuration.
use std::path::Path;

use crate::cli::{chrono_now, flag_value};

const USAGE: &str = "usage: quipu git-merge REF | git-merge show QPACK_DIR [--format text|md|json] | git-merge resolve QPACK_DIR KEY --choose CHOICE --by WHO | merge-driver BASE OURS THEIRS PATH | pendant-resolve BASE_REF OURS_REF THEIRS_REF DIR KEY CHOICE | pendant-check [--history] BASE_REF OURS_REF THEIRS_REF RESULT_REF";

fn fail(error: impl std::fmt::Display) -> ! {
    eprintln!("qpack merge refused: {error}");
    std::process::exit(1);
}

/// A qpack directory given as the directory or as its `decisions.json`.
fn qpack_dir(arg: &str) -> &str {
    let arg = arg.trim_start_matches("./");
    arg.strip_suffix("/decisions.json")
        .or_else(|| arg.strip_suffix("decisions.json"))
        .map(|d| d.trim_end_matches('/'))
        .filter(|d| !d.is_empty())
        .unwrap_or(arg)
}

/// Print one qpack's decisions in the standard layout (aegis-k7l).
fn show(dir: &str, format: &str) {
    let path = Path::new(dir).join("decisions.json");
    let file = quipu::merge_reconcile::ReconcileFile::load_path(&path).unwrap_or_else(|e| fail(e));
    let view = file.view(
        &path.to_string_lossy(),
        quipu::merge_reconcile::Surface::Cli,
    );
    match format {
        "text" => print!("{}", quipu::merge_reconcile::render_text(&view)),
        "md" | "markdown" => print!("{}", quipu::merge_reconcile::render_markdown(&view)),
        "json" => println!("{}", serde_json::to_string_pretty(&view).unwrap()),
        other => fail(format!("unknown --format {other:?}: use text, md or json")),
    }
}

/// Record one choice in the merge in progress, with refs from `MERGE_HEAD`,
/// and print the qpack's decisions again. The shared resolve verb for Git
/// (`quipu merge resolve <qpack>/decisions.json` lands here too).
pub(crate) fn resolve_in_merge(dir: &str, key: &str, choice: &str, by: &str) {
    let dir = qpack_dir(dir);
    if choice == "value" {
        fail(
            "Git pendant merges take ours, theirs, base, both or defer (accept/reject for alias proposals), not typed values",
        );
    }
    let repo = Path::new(".");
    let [base, ours, theirs] = quipu::git_merge::merge_refs(repo).unwrap_or_else(|e| fail(e));
    let by = format!("{by} at {}", chrono_now());
    quipu::git_merge::resolve_by(repo, &base, &ours, &theirs, dir, key, choice, Some(&by))
        .unwrap_or_else(|e| fail(e));
    show(dir, "text");
}

pub fn run(args: &[String]) {
    let repo = Path::new(".");
    if args[1] == "git-merge" {
        match args.get(2).map(String::as_str) {
            Some("show") => {
                let Some(dir) = args.get(3).filter(|a| !a.starts_with("--")) else {
                    fail(USAGE)
                };
                show(
                    qpack_dir(dir),
                    flag_value(args, "--format").unwrap_or("text"),
                );
                return;
            }
            Some("resolve") => {
                let (Some(dir), Some(key)) = (
                    args.get(3).filter(|a| !a.starts_with("--")),
                    args.get(4).filter(|a| !a.starts_with("--")),
                ) else {
                    fail(USAGE)
                };
                let (Some(choice), Some(by)) =
                    (flag_value(args, "--choose"), flag_value(args, "--by"))
                else {
                    fail("git-merge resolve needs --choose <choice> and --by <who>")
                };
                resolve_in_merge(qpack_dir(dir), key, choice, by);
                return;
            }
            _ => {}
        }
    }
    let mut pending = Vec::new();
    let result = match (args[1].as_str(), &args[2..]) {
        ("merge-driver", [base, ours, theirs, path]) => quipu::git_merge::driver(
            repo,
            Path::new(base),
            Path::new(ours),
            Path::new(theirs),
            path,
        )
        .inspect(|ready| {
            if !ready && let Some(dir) = Path::new(path).parent() {
                pending.push(dir.to_string_lossy().into_owned());
            }
        }),
        ("git-merge", [incoming]) => {
            quipu::git_merge::merge_with_pending(repo, incoming).map(|(ready, dirs)| {
                pending = dirs;
                ready
            })
        }
        ("pendant-resolve", [base, ours, theirs, dir, key, choice]) => {
            quipu::git_merge::resolve(repo, base, ours, theirs, dir, key, choice)
        }
        ("pendant-check", [mode, base, ours, theirs, result]) if mode == "--history" => {
            quipu::git_merge::check_history(repo, base, ours, theirs, result).map(|n| {
                println!("pendant-check: {n} historical packs verified");
                true
            })
        }
        ("pendant-check", [base, ours, theirs, result]) => {
            quipu::git_merge::check(repo, base, ours, theirs, result).map(|n| {
                println!("pendant-check: {n} packs verified");
                true
            })
        }
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(1);
        }
    };
    match result {
        Ok(true) => {}
        Ok(false) => {
            if args[1] == "git-merge" {
                // The full standard layout, once per qpack left undecided.
                for dir in &pending {
                    show(dir, "text");
                }
            }
            eprintln!(
                "qpack decisions pending; nothing is committed. Show them with: quipu merge show <qpack>/decisions.json{}",
                pending
                    .first()
                    .map(|d| format!(" (here: {d}/decisions.json)"))
                    .unwrap_or_default()
            );
            std::process::exit(2);
        }
        Err(e) => {
            eprintln!("qpack merge refused: {e}");
            std::process::exit(1);
        }
    }
}
