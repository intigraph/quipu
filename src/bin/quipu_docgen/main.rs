//! `quipu-docgen`: generate the CLI's man pages and shell completions from the
//! command catalogue, or check that the committed copies are current.
//!
//! ```text
//! quipu-docgen [--check] [--root DIR]
//! ```
//!
//! Writes `packaging/man/*.1` and `packaging/completions/*` under DIR (default
//! the current directory). With `--check` nothing is written: the command exits
//! 1 and names every file that is stale, missing or no longer generated. The
//! release workflow copies both directories into each archive, and packagers
//! (Homebrew, .deb) install them from there.
//!
//! Output carries no version or date, so a release bump never makes it stale;
//! only a change to the catalogue or the command descriptions does.

#[path = "../../cli_input/about.rs"]
mod about;
#[path = "../../cli_input/catalogue.rs"]
#[allow(dead_code)]
mod catalogue;
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
    match result {
        Ok(summary) => println!("{summary}"),
        Err(problems) => {
            eprintln!("{problems}");
            eprintln!(
                "Regenerate with `cargo run --features docgen --bin quipu-docgen` and commit the result."
            );
            std::process::exit(1);
        }
    }
}
