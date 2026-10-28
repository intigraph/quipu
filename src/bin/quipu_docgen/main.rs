//! `quipu-docgen`: generate the CLI's man pages, shell completions and the
//! book's CLI and MCP indexes from the command catalogue and the MCP tool
//! definitions, or check that the committed copies are current. Both modes
//! also check agent-surface coverage (see `coverage`).
//!
//! ```text
//! quipu-docgen [--check] [--root DIR]
//! ```
//!
//! Writes `packaging/man/*.1`, `packaging/completions/*`,
//! `docs/book/src/reference/cli-commands.md` and
//! `docs/book/src/reference/mcp-tool-index.md` under DIR (default the current
//! directory). With `--check` nothing is written: the command exits
//! 1 and names every file that is stale, missing or no longer generated. The
//! release workflow copies both directories into each archive, and packagers
//! (Homebrew, .deb) install them from there.
//!
//! Output carries no version or date, so a release bump never makes it stale;
//! only a change to the catalogue or the command descriptions does.

#[path = "../../cli_input/about.rs"]
mod about;
mod book;
#[path = "../../cli_input/catalogue.rs"]
#[allow(dead_code)]
mod catalogue;
mod coverage;
mod output;
mod tree;

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check = args.iter().any(|a| a == "--check");
    let root = args
        .windows(2)
        .find(|w| w[0] == "--root")
        .map_or_else(|| PathBuf::from("."), |w| PathBuf::from(&w[1]));
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("usage: quipu-docgen [--check] [--root DIR]");
        return;
    }
    let files = output::generate();
    let result = if check {
        output::check(&root, &files)
    } else {
        output::write(&root, &files)
    };
    let mut failed = false;
    match result {
        Ok(summary) => println!("{summary}"),
        Err(problems) => {
            eprintln!("{problems}");
            eprintln!(
                "Regenerate with `cargo run --features docgen --bin quipu-docgen` and commit the result."
            );
            failed = true;
        }
    }
    let uncovered = coverage::check(&root, &quipu::mcp::tool_definitions());
    if uncovered.is_empty() {
        println!("every CLI verb and MCP tool is in the primer, the skill or the exclusions");
    } else {
        eprintln!("{}", uncovered.join("\n"));
        failed = true;
    }
    if failed {
        std::process::exit(1);
    }
}
