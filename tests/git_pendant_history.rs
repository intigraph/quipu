//! Non-pack history is distinct from malformed final or declared packs.
#![cfg(feature = "shacl")]
use std::{
    path::Path,
    process::{Command, Output},
};
fn git(p: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(p)
        .args(args)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8(o.stdout).unwrap().trim().into()
}
fn check(p: &Path, base: &str, left: &str, right: &str, result: &str, history: bool) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_quipu"));
    cmd.current_dir(p).arg("pendant-check");
    if history {
        cmd.arg("--history");
    }
    cmd.args([base, left, right, result]).output().unwrap()
}
#[test]
fn historical_nonpack_passes_but_final_and_declared_pack_remain_strict() {
    let temp = tempfile::tempdir().unwrap();
    let p = temp.path();
    git(p, &["init", "-b", "main"]);
    git(p, &["config", "user.name", "Merge test"]);
    git(p, &["config", "user.email", "test@example.org"]);
    std::fs::write(p.join("README"), "base").unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-m", "base"]);
    let base = git(p, &["rev-parse", "HEAD"]);
    git(p, &["checkout", "-b", "left"]);
    std::fs::create_dir(p.join("fixture")).unwrap();
    std::fs::write(p.join("fixture/export.nt"), "<urn:a> <urn:b> <urn:c> .\n").unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-m", "nonpack fixture"]);
    let left = git(p, &["rev-parse", "HEAD"]);
    git(p, &["checkout", "-b", "right", &base]);
    std::fs::write(p.join("other"), "right").unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-m", "right"]);
    let right = git(p, &["rev-parse", "HEAD"]);
    git(p, &["checkout", "left"]);
    git(p, &["merge", "--no-ff", "right", "-m", "merge"]);
    let merge = git(p, &["rev-parse", "HEAD"]);
    assert!(
        check(p, &base, &left, &right, &merge, true)
            .status
            .success()
    );
    let strict = check(p, &base, &left, &right, &merge, false);
    assert!(!strict.status.success());
    assert!(String::from_utf8_lossy(&strict.stderr).contains("missing manifest"));
    git(p, &["mv", "fixture/export.nt", "fixture/payload.nq"]);
    git(p, &["commit", "-am", "repair fixture"]);
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/ci/pendant-merge-check.py");
    let event = p.join("event.json");
    std::fs::write(&event, serde_json::json!({"before":base}).to_string()).unwrap();
    let ci = Command::new("python3")
        .arg(&script)
        .args(["--binary", env!("CARGO_BIN_EXE_quipu")])
        .current_dir(p)
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_EVENT_PATH", &event)
        .output()
        .unwrap();
    assert!(
        ci.status.success(),
        "{}",
        String::from_utf8_lossy(&ci.stderr)
    );
    git(p, &["mv", "fixture/payload.nq", "fixture/export.nt"]);
    git(p, &["commit", "-am", "malformed final output"]);
    let final_ci = Command::new("python3")
        .arg(&script)
        .args(["--binary", env!("CARGO_BIN_EXE_quipu")])
        .current_dir(p)
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_EVENT_PATH", &event)
        .output()
        .unwrap();
    assert!(!final_ci.status.success());
    assert!(String::from_utf8_lossy(&final_ci.stderr).contains("missing manifest"));
    // A manifest declared only in the result also prevents the history exception.
    git(p, &["checkout", "--detach", &merge]);
    std::fs::write(p.join("fixture/manifest.json"), "{}").unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-m", "malformed declared pack"]);
    let declared = git(p, &["rev-parse", "HEAD"]);
    assert!(
        !check(p, &base, &left, &right, &declared, true)
            .status
            .success()
    );
}
