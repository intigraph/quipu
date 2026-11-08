//! The actual CLI and Git mergetool, using disposable repositories only.
#![cfg(feature = "shacl")]
#[path = "support/git_pendant_fixture.rs"]
mod fixture;
use fixture::*;
use std::io::Write;
use std::process::{Command, Output, Stdio};

fn tool(f: &Fixture, input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quipu"))
        .current_dir(f.path())
        .args(["git-mergetool", "qpack/export.nt", "--by", "reviewer"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn explicit_choice_uses_standard_layout_and_passes_immutable_ci_check() {
    let f = Fixture::new("functional");
    assert_eq!(f.merge().status.code(), Some(2));
    let o = tool(&f, "conflict:0 theirs\n");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).contains("[conflict:0]"));
    assert_eq!(f.decisions()["resolutions"]["conflict:0"], "theirs");
    assert!(
        f.decisions()["resolved_by"]["conflict:0"]
            .as_str()
            .unwrap()
            .starts_with("reviewer at ")
    );
    f.commit();
    assert!(f.check().status.success());
}

#[test]
fn eof_quit_and_unoffered_choices_never_claim_resolution() {
    for input in [
        "",
        "quit\n",
        "conflict:0 both\n",
        "conflict:999 ours\n",
        "conflict:0 ours extra\n",
    ] {
        let f = Fixture::new("functional");
        f.merge();
        let before = f.graph();
        assert_eq!(tool(&f, input).status.code(), Some(2));
        assert_eq!(f.graph(), before);
        assert!(f.decisions()["resolutions"].as_object().unwrap().is_empty());
    }
}

#[test]
fn aliases_use_the_existing_resolver() {
    let f = Fixture::new("alias");
    f.merge();
    assert!(tool(&f, "alias:0 reject\n").status.success());
    f.commit();
    assert!(f.check().status.success());
}

#[test]
fn stale_decisions_and_missing_merge_refuse_even_without_input() {
    let f = Fixture::new("functional");
    assert!(!tool(&f, "").status.success());
    f.merge();
    let path = f.path().join("qpack/decisions.json");
    let mut decisions = f.decisions();
    decisions["inputs"][0] = "not-the-reviewed-input".into();
    std::fs::write(&path, serde_json::to_vec(&decisions).unwrap()).unwrap();
    let before = f.graph();
    assert_eq!(tool(&f, "conflict:0 ours\n").status.code(), Some(1));
    assert_eq!(f.graph(), before);
}

#[test]
fn real_git_mergetool_stages_only_after_success() {
    let f = Fixture::new("functional");
    f.merge();
    let exe = env!("CARGO_BIN_EXE_quipu").replace('\'', "'\\''");
    let command = format!("'{exe}' git-mergetool \"$MERGED\" --by reviewer");
    let mut child = Command::new("git")
        .current_dir(f.path())
        .args([
            "-c",
            &format!("mergetool.quipu.cmd={command}"),
            "-c",
            "mergetool.quipu.trustExitCode=true",
            "-c",
            "mergetool.keepBackup=false",
            "mergetool",
            "--no-prompt",
            "--tool=quipu",
            "--",
            "qpack/export.nt",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"conflict:0 ours\n")
        .unwrap();
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(f.decisions()["resolutions"]["conflict:0"], "ours");
    assert!(g(f.path(), &["ls-files", "-u", "--", "qpack/export.nt"]).is_empty());
    f.commit();
    assert!(f.check().status.success());
}
