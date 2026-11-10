//! Standard SSSOM import executes the real CLI in an isolated directory.
#![cfg(feature = "shacl")]
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

const SOURCE: &str = "#mapping_set_id: urn:external:set\n#curie_map:\n#  ex: https://example.org/\nsubject_id\tpredicate_id\tobject_id\tmapping_justification\tauthor_id\taccepted\nex:a\tskos:exactMatch\tex:b\tex:UnknownMatcher\tex:claimed-reviewer\ttrue\n";

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quipu"))
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap()
}

#[test]
fn external_exact_and_forged_authorship_stay_pending_without_opening_store() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("set.tsv"), SOURCE).unwrap();
    let before = fs::metadata(dir.path().join("set.tsv"))
        .unwrap()
        .modified()
        .unwrap();
    let out = run(
        dir.path(),
        &[
            "align",
            "import",
            "set.tsv",
            "--out",
            "proposals.json",
            "--db",
            "never-open.db",
        ],
    );
    assert!(out.status.success(), "{out:?}");
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.path().join("proposals.json")).unwrap()).unwrap();
    assert_eq!(report["local_acceptance"], "pending");
    assert_eq!(report["evidence"]["source"], SOURCE);
    assert_eq!(report["evidence"]["rows"][0]["fields"]["accepted"], "true");
    assert_eq!(
        report["evidence"]["rows"][0]["justification"],
        "https://example.org/UnknownMatcher"
    );
    assert!(!dir.path().join("never-open.db").exists());
    assert_eq!(
        fs::read_to_string(dir.path().join("set.tsv")).unwrap(),
        SOURCE
    );
    assert_eq!(
        fs::metadata(dir.path().join("set.tsv"))
            .unwrap()
            .modified()
            .unwrap(),
        before
    );
}

#[test]
fn refusal_preserves_previous_output_and_missing_output_is_not_defaulted() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("set.tsv"),
        SOURCE.replace("ex:a", "missing:a"),
    )
    .unwrap();
    fs::write(dir.path().join("proposals.json"), "retained").unwrap();
    let before = fs::metadata(dir.path().join("proposals.json"))
        .unwrap()
        .modified()
        .unwrap();
    let bad = run(
        dir.path(),
        &["align", "import", "set.tsv", "--out", "proposals.json"],
    );
    assert!(!bad.status.success());
    assert_eq!(
        fs::read_to_string(dir.path().join("proposals.json")).unwrap(),
        "retained"
    );
    assert_eq!(
        fs::metadata(dir.path().join("proposals.json"))
            .unwrap()
            .modified()
            .unwrap(),
        before
    );
    let no_output = run(dir.path(), &["align", "import", "set.tsv"]);
    assert!(!no_output.status.success());
    assert!(String::from_utf8_lossy(&no_output.stderr).contains("--out"));
}

#[test]
#[cfg(unix)]
fn import_refuses_to_replace_its_source_even_through_a_hardlink() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("set.tsv");
    fs::write(&source, SOURCE).unwrap();
    for output in ["set.tsv", "linked.tsv"] {
        if output == "linked.tsv" {
            fs::hard_link(&source, dir.path().join(output)).unwrap();
        }
        let result = run(dir.path(), &["align", "import", "set.tsv", "--out", output]);
        assert!(!result.status.success());
        assert_eq!(fs::read_to_string(&source).unwrap(), SOURCE);
    }
}
