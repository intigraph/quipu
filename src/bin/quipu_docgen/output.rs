//! The generated file set, and writing or checking it against a directory.

use std::collections::BTreeMap;
use std::path::Path;

use clap::Command;
use clap_complete::Shell;

use crate::tree;

/// Directories this generator owns outright: anything else inside them is
/// stale output and is removed on write, reported on check.
pub const OWNED_DIRS: &[&str] = &["packaging/man", "packaging/completions"];

/// Every generated file, keyed by its path relative to the repository root.
pub fn generate() -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let root = tree::build("quipu");
    man_pages(&root, "quipu", "quipu", &mut files);
    for bin in ["quipu", "qp"] {
        for (shell, name) in [
            (Shell::Bash, format!("{bin}.bash")),
            (Shell::Zsh, format!("_{bin}")),
            (Shell::Fish, format!("{bin}.fish")),
            (Shell::Elvish, format!("{bin}.elv")),
            (Shell::PowerShell, format!("_{bin}.ps1")),
        ] {
            let mut cmd = tree::build(bin);
            let mut buf = Vec::new();
            clap_complete::generate(shell, &mut cmd, bin, &mut buf);
            files.insert(format!("packaging/completions/{name}"), buf);
        }
    }
    files
}

/// One page per command at every depth: `quipu.1`, `quipu-shapes.1`,
/// `quipu-shapes-load.1`, as git and cargo name theirs.
fn man_pages(cmd: &Command, page: &str, usage: &str, files: &mut BTreeMap<String, Vec<u8>>) {
    let rendered = cmd
        .clone()
        .name(page.to_string())
        .bin_name(usage.to_string())
        .display_name(page.to_string());
    let mut buf = Vec::new();
    clap_mangen::Man::new(rendered)
        .title(page.to_uppercase())
        .section("1")
        .source("quipu")
        .manual("Quipu Manual")
        .render(&mut buf)
        .expect("render man page into memory");
    files.insert(format!("packaging/man/{page}.1"), quote_empty_date(buf));
    for sub in cmd.get_subcommands() {
        man_pages(
            sub,
            &format!("{page}-{}", sub.get_name()),
            &format!("{usage} {}", sub.get_name()),
            files,
        );
    }
}

/// clap_mangen writes an empty date as nothing, so `.TH NAME 1  quipu` shifts
/// the source into the date field. Pages carry no date on purpose (a release
/// must not make them stale), so quote the empty field instead.
fn quote_empty_date(page: Vec<u8>) -> Vec<u8> {
    let text = String::from_utf8(page).expect("man page is UTF-8");
    text.lines()
        .map(|l| {
            if l.starts_with(".TH ") {
                l.replacen(" 1  ", " 1 \"\" ", 1)
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .into_bytes()
        .into_iter()
        .chain(text.ends_with('\n').then_some(b'\n'))
        .collect()
}

/// Files under the owned directories that exist on disk.
fn on_disk(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for dir in OWNED_DIRS {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.path().is_file() {
                found.push(format!("{dir}/{}", entry.file_name().to_string_lossy()));
            }
        }
    }
    found.sort();
    found
}

/// Compare `files` with what is committed under `root`.
pub fn check(root: &Path, files: &BTreeMap<String, Vec<u8>>) -> Result<String, String> {
    let mut problems = Vec::new();
    for (path, want) in files {
        match std::fs::read(root.join(path)) {
            Ok(have) if &have == want => {}
            Ok(_) => problems.push(format!("stale: {path}")),
            Err(_) => problems.push(format!("missing: {path}")),
        }
    }
    for path in on_disk(root) {
        if !files.contains_key(&path) {
            problems.push(format!("no longer generated: {path}"));
        }
    }
    if problems.is_empty() {
        Ok(format!("{} generated files are current", files.len()))
    } else {
        Err(problems.join("\n"))
    }
}

/// Write `files` under `root`, removing stale files from the owned directories.
pub fn write(root: &Path, files: &BTreeMap<String, Vec<u8>>) -> Result<String, String> {
    let mut removed = 0;
    for path in on_disk(root) {
        if !files.contains_key(&path) {
            std::fs::remove_file(root.join(&path)).map_err(|e| format!("{path}: {e}"))?;
            removed += 1;
        }
    }
    let mut changed = 0;
    for (path, bytes) in files {
        let full = root.join(path);
        if std::fs::read(&full).ok().as_ref() == Some(bytes) {
            continue;
        }
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{path}: {e}"))?;
        }
        std::fs::write(&full, bytes).map_err(|e| format!("{path}: {e}"))?;
        changed += 1;
    }
    Ok(format!(
        "{} generated files: {changed} written, {removed} removed",
        files.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_deterministic_and_versionless() {
        let a = generate();
        assert_eq!(a, generate());
        let page = String::from_utf8(a["packaging/man/quipu-knot.1"].clone()).unwrap();
        assert!(page.contains("quipu knot"), "{page}");
        assert!(!page.contains(env!("CARGO_PKG_VERSION")));
        assert!(
            page.contains(".TH QUIPU-KNOT 1 \"\" quipu \"Quipu Manual\""),
            "{page}"
        );
        assert!(a.contains_key("packaging/man/quipu-shapes-load.1"));
        assert!(a.contains_key("packaging/completions/_qp"));
    }

    #[test]
    fn check_reports_stale_missing_and_extra_files() {
        let dir = tempfile::tempdir().unwrap();
        let files = generate();
        assert!(check(dir.path(), &files).is_err());
        write(dir.path(), &files).unwrap();
        assert!(check(dir.path(), &files).is_ok());
        std::fs::write(dir.path().join("packaging/man/quipu-knot.1"), "edited").unwrap();
        std::fs::write(dir.path().join("packaging/man/quipu-gone.1"), "old").unwrap();
        let err = check(dir.path(), &files).unwrap_err();
        assert!(err.contains("stale: packaging/man/quipu-knot.1"), "{err}");
        assert!(err.contains("no longer generated: packaging/man/quipu-gone.1"));
        write(dir.path(), &files).unwrap();
        assert!(check(dir.path(), &files).is_ok());
    }
}
