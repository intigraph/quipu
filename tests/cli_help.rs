//! Help/usage must be a pure operation of the actual CLI binary.
#![cfg(feature = "shacl")]
use std::{
    collections::BTreeSet,
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quipu"))
        .env_clear()
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("home/config"))
        .env("XDG_CACHE_HOME", root.join("home/cache"))
        .env("PATH", "/usr/bin:/bin")
        .current_dir(root.join("cwd"))
        .args(args)
        .output()
        .unwrap()
}
fn sandbox() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("home")).unwrap();
    fs::create_dir(dir.path().join("cwd")).unwrap();
    dir
}
fn snapshot(root: &Path) -> Vec<(std::path::PathBuf, Vec<u8>, std::time::SystemTime)> {
    let mut files = Vec::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(snapshot(&path));
        } else {
            files.push((
                path.clone(),
                fs::read(&path).unwrap(),
                fs::metadata(path).unwrap().modified().unwrap(),
            ));
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}
fn paths() -> BTreeSet<String> {
    // Enumeration is syntax data only: every assertion below executes the
    // real binary and examines filesystem/process effects, never a mock parser.
    include_str!("../src/cli_input/catalogue.rs")
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("spec!(\"")?
                .split('"')
                .next()
                .map(str::to_owned)
        })
        .collect()
}
#[test]
fn every_command_and_nested_help_is_store_and_config_free() {
    let paths = paths();
    assert!(paths.len() > 100);
    let dir = sandbox();
    let root = dir.path();
    // Invalid config would emit a warning if the help path ever loaded it.
    fs::create_dir(root.join("cwd/.bobbin")).unwrap();
    fs::write(root.join("cwd/.bobbin/config.toml"), "invalid TOML [").unwrap();
    let before = snapshot(root);
    for path in paths {
        for flag in ["-h", "--help"] {
            let mut args: Vec<_> = path.split_whitespace().collect();
            args.push(flag);
            let output = run(root, &args);
            assert!(output.status.success(), "{args:?}: {output:?}");
            assert!(output.stderr.is_empty(), "{args:?}: {output:?}");
            let help = String::from_utf8(output.stdout).unwrap();
            assert!(help.contains(&format!("qp {path}")), "{args:?}: {help}");
            assert!(help.contains(".bobbin/quipu/quipu.db"));
            assert_eq!(snapshot(root), before, "{args:?} changed files");
        }
        let mut invalid: Vec<_> = path.split_whitespace().collect();
        invalid.push("--this-option-does-not-exist");
        let output = run(root, &invalid);
        assert_eq!(output.status.code(), Some(2), "{invalid:?}: {output:?}");
        assert_eq!(snapshot(root), before, "{invalid:?} changed files");
        let args: Vec<_> = std::iter::once("help")
            .chain(path.split_whitespace())
            .collect();
        let output = run(root, &args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert!(output.stderr.is_empty(), "{args:?}: {output:?}");
        assert_eq!(snapshot(root), before, "{args:?} changed files");
    }
    for args in [
        vec![],
        vec!["-h"],
        vec!["--help"],
        vec!["--version", "--help"],
        vec!["-V", "-h"],
        vec!["help"],
        vec!["help", "--help"],
        vec!["help", "help"],
        vec!["shapes", "help", "load"],
        vec!["import", "review", "help", "notify"],
        vec!["shapes", "-h", "load"],
        vec!["help", "shapes", "load"],
        vec!["share", "--project", "--help"],
    ] {
        let output = run(root, &args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert!(output.stderr.is_empty(), "{args:?}: {output:?}");
        assert_eq!(snapshot(root), before);
    }
    let output = run(root, &["shapes", "load", "--help"]);
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("qp shapes load <name> <file.ttl>"));
    assert!(help.contains("local database"));
    assert!(help.contains("no --remote flag"));
}
#[test]
fn invalid_syntax_refuses_before_any_store_or_config_access() {
    let dir = sandbox();
    let root = dir.path();
    let before = snapshot(root);
    for args in [
        vec!["not-a-command"],
        vec!["--help", "--bogus"],
        vec!["version", "--bogus"],
        vec!["--version", "--bogus"],
        vec!["-V", "--bogus"],
        vec!["shapes", "bogus"],
        vec!["shapes", "bogus", "--help"],
        vec!["shapes", "load", "s", "s.ttl", "--bogus"],
        vec!["shapes", "list", "extra"],
        vec!["stats", "--remote", "http://localhost"],
        vec!["stats", "--db"],
        vec!["graph", "bogus"],
        vec!["db", "bogus"],
        vec!["ontology", "bogus"],
        vec!["import", "review", "bogus"],
        vec!["audit", "quarantine", "bogus"],
        vec!["policy", "bogus"],
        vec!["fork", "bogus"],
        vec!["hook", "bogus"],
        vec!["hooks", "install", "--bogus"],
        vec!["help", "shapes", "bogus"],
        vec!["read", "SELECT * WHERE {}", "--limit", "1"],
    ] {
        let output = run(root, &args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("help"));
        assert_eq!(snapshot(root), before, "{args:?} changed files");
    }
}
#[test]
fn real_local_write_read_and_shapes_lifecycle_still_work() {
    let dir = sandbox();
    let root = dir.path();
    fs::write(
        root.join("cwd/data.ttl"),
        "<urn:subject> <urn:predicate> \"value\" .",
    )
    .unwrap();
    fs::write(root.join("cwd/shapes.ttl"), "@prefix sh: <http://www.w3.org/ns/shacl#> . <urn:shape> a sh:NodeShape ; sh:targetClass <urn:Type> .").unwrap();
    for args in [
        vec!["knot", "data.ttl", "--db", "data.db"],
        vec!["shapes", "load", "s", "shapes.ttl", "--db", "data.db"],
        vec!["shapes", "list", "--db", "data.db"],
    ] {
        let output = run(root, &args);
        assert!(output.status.success(), "{args:?}: {output:?}");
    }
    let output = run(
        root,
        &[
            "read",
            "SELECT ?v WHERE { <urn:subject> <urn:predicate> ?v }",
            "--db",
            "data.db",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("value"));
    let before = snapshot(root);
    for args in [
        vec!["shapes", "remove", "--help", "--db", "data.db"],
        vec!["shapes", "--help"],
        vec!["stats", "--bogus", "--db", "data.db"],
    ] {
        let output = run(root, &args);
        assert_eq!(
            output.status.success(),
            !args.contains(&"--bogus"),
            "{output:?}"
        );
        assert_eq!(
            snapshot(root),
            before,
            "{args:?} changed existing store bytes/mtime"
        );
    }
    let output = run(root, &["shapes", "remove", "s", "--db", "data.db"]);
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("removed"));
    // A consumed option value is not a help request, even if named --help.
    let output = run(root, &["stats", "--db", "--help"]);
    assert!(output.status.success(), "{output:?}");
    assert!(root.join("cwd/--help").exists());
}
#[test]
fn catalogue_covers_actual_top_level_dispatch() {
    let roots: BTreeSet<_> = paths()
        .into_iter()
        .map(|p| p.split_whitespace().next().unwrap().to_owned())
        .collect();
    let mut checked = 0;
    for line in include_str!("../src/main.rs").lines() {
        let Some(line) = line.strip_prefix("        ") else {
            continue;
        };
        if !line.starts_with('"') || !line.contains("=>") {
            continue;
        }
        for token in line.split("=>").next().unwrap().split('|') {
            let token = token.trim().trim_matches('"');
            if token.chars().all(|c| c.is_ascii_lowercase() || c == '-')
                && !token.starts_with('-')
                && token != "help"
            {
                assert!(
                    roots.contains(token),
                    "dispatch missing from help/admission: {token}"
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 40);
}
