//! Immutable Git transport and guarded output for DataBook decisions.
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use crate::error::Result;
use crate::git_merge_repo::{file, from_env, git, refs, text};
use super::{File, Plan, SCHEMA, document::Document};
use super::super::{invalid, io};

pub fn is_document(path: &str) -> bool { path.ends_with(".databook.md") }
fn safe(repo: &Path, path: &str) -> Result<PathBuf> {
    if path.is_empty() || Path::new(path).components().any(|part| !matches!(part, Component::Normal(_)) || part.as_os_str() == ".git") {
        return Err(invalid("DataBook path must be repository-relative"));
    }
    let mut result = repo.to_path_buf();
    for part in Path::new(path).components() {
        result.push(part);
        if result.is_symlink() { return Err(invalid("DataBook path traverses a symlink")); }
    }
    Ok(result)
}
fn sidecar(path: &str) -> String { format!("{path}.decisions.json") }
fn build(repo: &Path, commits: &[String; 3], path: &str) -> Result<Plan> {
    if !is_document(path) { return Err(invalid("DataBook driver requires an explicit .databook.md path")); }
    safe(repo, path)?; safe(repo, &sidecar(path))?;
    let mut sources = Vec::new();
    for commit in commits {
        sources.push(file(repo, commit, path)?.ok_or_else(|| invalid("DataBook merge needs a regular document in all three Git inputs"))?);
    }
    Plan::new(sources.try_into().map_err(|_| invalid("three DataBook inputs required"))?, path, commits.clone())
}
fn replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().ok_or_else(|| invalid("DataBook output has no parent"))?).map_err(io)?;
    std::io::Write::write_all(&mut temp, bytes).map_err(io)?;
    temp.persist(path).map_err(io)?;
    Ok(())
}
fn write(repo: &Path, path: &str, output: &str, file: &File) -> Result<()> {
    let document = safe(repo, path)?;
    let decisions = safe(repo, &sidecar(path))?;
    // A crash-separated pair cannot pass immutable CI. Publish evidence first.
    replace(&decisions, &serde_json::to_vec_pretty(file).map_err(io)?)?;
    replace(&document, output.as_bytes())
}

pub fn load_path(path: &Path) -> Result<File> {
    let expected = path.to_str().and_then(|path| path.strip_suffix(".decisions.json")).ok_or_else(|| invalid("DataBook decisions need their document sidecar path"))?.trim_start_matches("./");
    let file: File = serde_json::from_slice(&std::fs::read(path).map_err(io)?).map_err(io)?;
    if file.schema != SCHEMA || file.document != expected || !is_document(expected) {
        return Err(invalid("DataBook decision schema/document path mismatch"));
    }
    safe(Path::new("."), &file.document)?;
    Ok(file)
}

/// Verify Git temporary inputs before writing a DataBook result. No store is opened.
pub fn driver(repo: &Path, base: &Path, ours: &Path, theirs: &Path, path: &str) -> Result<bool> {
    let commits = from_env(repo)?;
    for (commit, temporary) in commits.iter().zip([base, ours, theirs]) {
        if !std::fs::symlink_metadata(temporary).map_err(io)?.file_type().is_file()
            || Some(std::fs::read(temporary).map_err(io)?) != file(repo, commit, path)?.map(String::into_bytes) {
            return Err(invalid("DataBook temporary input does not match its immutable Git snapshot"));
        }
    }
    let mut plan = build(repo, &commits, path)?;
    let (output, ready) = plan.apply(None)?;
    let decisions = safe(repo, &sidecar(path))?;
    replace(&decisions, &serde_json::to_vec_pretty(&plan.file).map_err(io)?)?;
    replace(ours, output.as_bytes())?;
    Ok(ready)
}

/// Revalidate the current decision/result pair without changing it.
pub fn reconcile(repo: &Path, path: &str) -> Result<bool> {
    let commits = crate::git_merge_repo::merge_refs(repo)?;
    let recorded = load_path(&safe(repo, &sidecar(path))?)?;
    let mut plan = build(repo, &commits, path)?;
    let (expected, ready) = plan.apply(Some(&recorded))?;
    if std::fs::read(safe(repo, path)?).map_err(io)? != expected.as_bytes() {
        return Err(invalid("DataBook working result differs from its recorded decisions; preserve manual edits on a branch"));
    }
    Ok(ready)
}

/// Resolve one graph-qualified row through the shared operator and rebuild the document.
pub fn resolve(repo: &Path, path: &str, key: &str, choice: &str, by: &str) -> Result<bool> {
    if by.trim().is_empty() { return Err(invalid("DataBook resolution needs --by")); }
    reconcile(repo, path)?;
    let commits = crate::git_merge_repo::merge_refs(repo)?;
    let mut recorded = load_path(&safe(repo, &sidecar(path))?)?;
    let view = recorded.view();
    let choices = view.conflicts.iter().find(|row| row.id == key).map(|row| &row.choices)
        .or_else(|| view.aliases.iter().find(|row| row.id == key).map(|row| &row.choices));
    if !choices.is_some_and(|choices| choices.iter().any(|offered| offered == choice)) {
        return Err(invalid("DataBook row/choice is not offered by its decision layout"));
    }
    let (group, row) = key.split_once('/').ok_or_else(|| invalid("DataBook row needs graph qualification"))?;
    let index: usize = group.strip_prefix("graph:").ok_or_else(|| invalid("invalid DataBook graph row"))?.parse().map_err(io)?;
    let decisions = &mut recorded.groups.get_mut(index).ok_or_else(|| invalid("unknown DataBook graph"))?.decisions;
    if choice == "defer" { decisions.resolutions.remove(row); }
    else { decisions.resolutions.insert(row.into(), choice.into()); }
    decisions.resolved_by.insert(row.into(), by.into());
    let mut plan = build(repo, &commits, path)?;
    let (output, ready) = plan.apply(Some(&recorded))?;
    write(repo, path, &output, &plan.file)?;
    Ok(ready)
}

/// Byte-for-byte CI replay of an immutable DataBook merge (including sidecar bindings).
pub fn check(repo: &Path, base: &str, ours: &str, theirs: &str, result: &str, path: &str) -> Result<()> {
    let commits = refs(repo, base, ours, theirs)?;
    let result = text(repo, &["rev-parse", "--verify", "--end-of-options", &format!("{result}^{{commit}}")])?;
    let mut plan = build(repo, &commits, path)?;
    let recorded: File = serde_json::from_str(&file(repo, result.trim(), &sidecar(path))?.ok_or_else(|| invalid("merged DataBook needs its decisions sidecar"))?).map_err(io)?;
    let (expected, ready) = plan.apply(Some(&recorded))?;
    if !ready || file(repo, result.trim(), path)?.as_deref() != Some(&expected) {
        return Err(invalid("DataBook CI result is unresolved or differs from the immutable merge"));
    }
    Ok(())
}

fn declared(repo: &Path, commit: &str, path: &str) -> Result<bool> {
    let directory = tempfile::tempdir().map_err(io)?;
    let index = directory.path().join("index");
    for args in [vec!["read-tree", commit], vec!["check-attr", "--cached", "-z", "merge", "--", path]] {
        let output = Command::new("git").arg("-C").arg(repo).args(&args).env("GIT_INDEX_FILE", &index).output().map_err(|e| super::super::spawn_git(&e))?;
        if !output.status.success() { return Err(invalid(String::from_utf8_lossy(&output.stderr))); }
        if args[0] == "check-attr" {
            let values: Vec<_> = output.stdout.split(|byte| *byte == 0).collect();
            return Ok(values.get(2).copied() == Some(b"quipu-databook"));
        }
    }
    Ok(false)
}

pub(crate) fn prepare(repo: &Path, commits: &[String; 3]) -> Result<Vec<(String, Plan)>> {
    let mut paths = BTreeSet::new();
    for commit in commits {
        for path in String::from_utf8(git(repo, &["ls-tree", "-rz", "--name-only", commit])?).map_err(io)?.split('\0') {
            if is_document(path) { paths.insert(path.to_string()); }
        }
    }
    let mut plans = Vec::new();
    for path in paths {
        let declared: Vec<_> = commits.iter().map(|commit| declared(repo, commit, &path)).collect::<Result<_>>()?;
        if declared.iter().all(|value| !value) { continue; }
        if declared.iter().any(|value| !value) { return Err(invalid("declare merge=quipu-databook in every immutable input before branching")); }
        let sources = commits.iter().map(|commit| file(repo, commit, &path)).collect::<Result<Vec<_>>>()?;
        if let [Some(base), Some(ours), Some(theirs)] = sources.as_slice()
            && base != ours && base != theirs {
            let mut plan = build(repo, commits, &path)?;
            plan.apply(None)?; // Validate the complete preflight before Git mutates anything.
            plans.push((path, plan));
        }
    }
    Ok(plans)
}

pub(crate) fn finish(repo: &Path, path: &str, plan: &mut Plan) -> Result<bool> {
    let (output, ready) = plan.apply(None)?;
    write(repo, path, &output, &plan.file)?;
    Ok(ready)
}
