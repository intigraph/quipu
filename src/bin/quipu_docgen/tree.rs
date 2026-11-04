//! A clap `Command` tree built from the CLI catalogue.
//!
//! The `quipu` binary parses with `src/cli_input` (store-free admission over
//! `SPECS`), not clap. This tree exists only so `clap_mangen` and `clap_complete`
//! can render that same catalogue; it is never used to parse arguments.

use clap::{Arg, ArgAction, Command};

use crate::about::about;
use crate::catalogue::{SPECS, Spec, is_group, option_help};

/// Root-level text shared by the man page and the completions.
const ROOT_ABOUT: &str = "AI-native knowledge graph with strict ontology enforcement";
const ROOT_LONG: &str = "Quipu stores RDF facts in one SQLite file, queries them with SPARQL, \
validates writes against SHACL shapes and keeps every fact's transaction and valid time. \
`qp` is the same binary under its short name. Run `quipu prime` for an agent primer.\n\n\
Store commands operate on a local database (--db, default .bobbin/quipu/quipu.db; \
configuration may override it), not the HTTP service. Help never reads configuration \
or opens a store.";

/// The whole tree, rooted at `bin` (`quipu` or `qp`).
pub fn build(bin: &str) -> Command {
    let mut root = Command::new(bin.to_string())
        .about(ROOT_ABOUT)
        .long_about(ROOT_LONG)
        .subcommand_required(true)
        .disable_help_subcommand(true)
        .arg(db_arg());
    for spec in SPECS {
        let words: Vec<&str> = spec.path.split_whitespace().collect();
        root = insert(root, &words, 1, spec);
    }
    root
}

/// `--db`, which admission accepts on every command. Added to each command
/// rather than marked global, so every man page lists it.
fn db_arg() -> Arg {
    Arg::new("db")
        .long("db")
        .value_name("PATH")
        .help("Store file (default: .bobbin/quipu/quipu.db; configuration may override it)")
}

/// Place `spec` under `cmd` at `spec.path`'s words from `depth` on, creating
/// intermediate commands, each described by its own path's ABOUT row.
fn insert(mut cmd: Command, words: &[&str], depth: usize, spec: &Spec) -> Command {
    let head = words[depth - 1];
    if !cmd.get_subcommands().any(|c| c.get_name() == head) {
        let mut child = Command::new(head.to_string()).arg(db_arg());
        if let Some(text) = about(&words[..depth].join(" ")) {
            child = child.about(text);
        }
        cmd = cmd.subcommand(child);
    }
    cmd.mut_subcommand(head, |child| {
        if depth == words.len() {
            apply(child, spec)
        } else {
            insert(child, words, depth + 1, spec)
        }
    })
}

/// Operands, value options and switches of one catalogue row.
fn apply(mut cmd: Command, spec: &Spec) -> Command {
    if is_group(spec) {
        return cmd.subcommand_required(true);
    }
    let mut taken: Vec<String> = Vec::new();
    // Alternatives (`<pack>... | <key=pack>...`) document the first form; the
    // others are spellings of the same operands.
    let first_form = spec.operands.split('|').next().unwrap_or("");
    for (i, token) in first_form.split_whitespace().enumerate() {
        let many = token.ends_with("...");
        let inner = token
            .trim_end_matches("...")
            .trim_matches(|c| matches!(c, '<' | '>' | '[' | ']'));
        let id = format!("arg-{}", sanitize(inner));
        if taken.contains(&id) {
            continue;
        }
        taken.push(id.clone());
        let mut arg = Arg::new(id)
            .value_name(inner.to_string())
            .required(i < spec.min);
        if many {
            arg = arg.num_args(1..).action(ArgAction::Append);
        }
        cmd = cmd.arg(arg);
    }
    for name in spec.values.split_whitespace() {
        if name == "db" || taken.iter().any(|t| t == name) {
            continue;
        }
        taken.push(name.to_string());
        cmd = cmd.arg(
            Arg::new(name.to_string())
                .long(name.to_string())
                .value_name("VALUE")
                .help(option_help(spec.path, name).expect("catalogue option description"))
                .action(ArgAction::Append),
        );
    }
    for name in spec.switches.split_whitespace() {
        if taken.iter().any(|t| t == name) {
            continue;
        }
        taken.push(name.to_string());
        cmd = cmd.arg(
            Arg::new(name.to_string())
                .long(name.to_string())
                .help(option_help(spec.path, name).expect("catalogue switch description"))
                .action(ArgAction::SetTrue),
        );
    }
    cmd
}

fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_is_a_valid_clap_command_with_every_catalogue_path() {
        let cmd = build("quipu");
        cmd.clone().debug_assert();
        for spec in SPECS {
            let mut node = &cmd;
            for word in spec.path.split_whitespace() {
                node = node
                    .find_subcommand(word)
                    .unwrap_or_else(|| panic!("missing {}", spec.path));
            }
            for value in spec
                .values
                .split_whitespace()
                .chain(spec.switches.split_whitespace())
                .filter(|v| *v != "db")
            {
                assert!(
                    node.get_arguments().any(|a| a.get_long() == Some(value)
                        && a.get_help()
                            .is_some_and(|h| !h.to_string().trim().is_empty())),
                    "{}: --{value}",
                    spec.path
                );
            }
        }
    }

    #[test]
    fn commands_carry_their_about_and_db() {
        let cmd = build("quipu");
        let knot = cmd.find_subcommand("knot").unwrap();
        assert_eq!(
            knot.get_about().unwrap().to_string(),
            about("knot").unwrap()
        );
        let load = cmd
            .find_subcommand("shapes")
            .and_then(|c| c.find_subcommand("load"))
            .unwrap();
        assert_eq!(
            load.get_about().unwrap().to_string(),
            about("shapes load").unwrap()
        );
        assert!(load.get_arguments().any(|a| a.get_long() == Some("db")));
    }
}
