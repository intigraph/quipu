//! Pure argument admission, before any configuration, credentials or store.
mod catalogue;
use catalogue::{SPECS, Spec, is_group};

pub(crate) enum Action {
    Run,
    Help(String),
}

fn words(list: &str, value: &str) -> bool {
    list.split_whitespace().any(|item| item == value)
}

fn family(spec: &Spec, command: &str) -> bool {
    spec.path.split_whitespace().next() == Some(command)
}

fn matches_path(spec: &Spec, positionals: &[&str]) -> bool {
    let path: Vec<_> = spec.path.split_whitespace().collect();
    positionals.starts_with(&path)
}

/// Only syntax is inspected here. In particular, selecting help never loads
/// config, stats a database, reads an input file or starts the MCP companion.
pub(crate) fn check(args: &[String]) -> Result<Action, String> {
    let Some(first) = args.get(1).map(String::as_str) else {
        return Ok(Action::Help(String::new()));
    };
    if matches!(first, "-h" | "--help") {
        if args[2..]
            .iter()
            .any(|a| !matches!(a.as_str(), "-h" | "--help"))
        {
            return Err("unexpected argument after top-level help; use help <command>".into());
        }
        return Ok(Action::Help(String::new()));
    }
    let help_command = first == "help";
    let start = if help_command { 2 } else { 1 };
    let Some(command) = args.get(start).map(String::as_str) else {
        return Ok(Action::Help(String::new()));
    };
    let command = match command {
        "--version" | "-V" => "version",
        other => other,
    };
    if help_command && matches!(command, "help" | "-h" | "--help") && args.len() == 3 {
        return Ok(Action::Help(String::new()));
    }
    if !SPECS.iter().any(|spec| family(spec, command)) {
        return Err(format!("unknown command: {command}"));
    }
    let mut positionals = vec![command];
    let mut options = Vec::new();
    let mut help = help_command;
    let mut i = start + 1;
    while i < args.len() {
        let arg = args[i].as_str();
        if matches!(arg, "--help" | "-h") {
            help = true;
        } else if let Some(flag) = arg.strip_prefix("--") {
            // The MCP companion also accepts --db=PATH. Other legacy parsers
            // use exact flag tokens and must not silently accept equals syntax.
            if command == "mcp" && flag.starts_with("db=") {
                options.push("db");
            } else if flag == "db"
                || SPECS
                    .iter()
                    .any(|s| family(s, command) && words(s.values, flag))
            {
                options.push(flag);
                i += 1;
                if args.get(i).is_none() {
                    return Err(format!("--{flag} requires a value"));
                }
            } else if SPECS
                .iter()
                .any(|s| family(s, command) && words(s.switches, flag))
            {
                options.push(flag);
                // This is the sole optional-value flag in the local CLI.
                if command == "share"
                    && flag == "project"
                    && args.get(i + 1).is_some_and(|s| !s.starts_with('-'))
                {
                    i += 1;
                }
            } else {
                return Err(format!("unknown option for {command}: {arg}"));
            }
        } else if arg == "help"
            && SPECS.iter().any(|s| {
                s.path == positionals.join(" ") && (s.max == 0 || is_group(s) || s.path == "fork")
            })
        {
            help = true;
        } else if arg.starts_with('-') && arg != "-" {
            return Err(format!("unknown option for {command}: {arg}"));
        } else {
            positionals.push(arg);
        }
        i += 1;
    }
    let spec = SPECS
        .iter()
        .filter(|s| matches_path(s, &positionals))
        .max_by_key(|s| s.path.split_whitespace().count())
        .expect("known root");
    let path_len = spec.path.split_whitespace().count();
    let operands = &positionals[path_len..];
    for flag in options {
        if flag != "db" && !words(spec.values, flag) && !words(spec.switches, flag) {
            return Err(format!("unknown option for {}: --{flag}", spec.path));
        }
    }
    if operands.len() > spec.max || (is_group(spec) && !operands.is_empty()) {
        return Err(format!(
            "unknown subcommand or unexpected argument for {}",
            spec.path
        ));
    }
    if help {
        // Extra words are not silently discarded. `help shapes typo` is an
        // error, whereas `shapes load --help` need not supply required inputs.
        if help_command && !operands.is_empty() {
            return Err(format!("unknown help topic: {}", positionals.join(" ")));
        }
        return Ok(Action::Help(spec.path.to_string()));
    }
    if is_group(spec) {
        return Err(format!("{} requires a supported subcommand", spec.path));
    }
    if operands.len() < spec.min || operands.len() > spec.max {
        return Err(format!(
            "unexpected or missing argument; usage: qp {} {}",
            spec.path, spec.operands
        ));
    }
    if spec.path == "fork" && operands.first().is_none_or(|s| s.parse::<i64>().is_err()) {
        return Err("fork requires a transaction number or supported subcommand".into());
    }
    Ok(Action::Run)
}

pub(crate) fn print_help(path: &str) {
    println!("Usage: qp {path} [OPTIONS]");
    for spec in SPECS.iter().filter(|s| {
        s.path == path
            || s.path
                .strip_prefix(path)
                .is_some_and(|tail| tail.starts_with(' '))
    }) {
        if is_group(spec) {
            continue;
        }
        let operands = if spec.operands.is_empty() {
            String::new()
        } else {
            format!(" {}", spec.operands)
        };
        println!("    qp {}{operands}", spec.path);
        if !spec.values.is_empty() {
            println!(
                "      Value options: {}",
                spec.values
                    .split_whitespace()
                    .map(|v| format!("--{v} <VALUE>"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        if !spec.switches.is_empty() {
            println!(
                "      Switches: {}",
                spec.switches
                    .split_whitespace()
                    .map(|v| format!("--{v}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
    }
    print_scope();
}

pub(crate) fn print_scope() {
    println!(
        "\nLocal store: --db <path> (default: .bobbin/quipu/quipu.db; configuration may override it)."
    );
    println!("Help (-h/--help or help <command>) never reads configuration or opens a store.");
    println!(
        "Store commands operate on a local database, not the HTTP service; there is no --remote flag."
    );
    println!(
        "import can fetch a share URL; read --federation uses configured remotes; mcp starts the local stdio companion."
    );
    println!(
        "Git transport, hook(s) and install-alias operate on local files/configuration rather than a graph store."
    );
    println!(
        "share --project [ID] takes an optional project ID. ontology/explain require the owl feature; reason --reactive requires reactive-reasoner."
    );
    println!(
        "-h, --help  Print help and exit. Value options consume the following argument literally."
    );
}
