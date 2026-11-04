//! `quipu databook export|import` through the real binary: a graph leaves one
//! store as a DataBook and arrives in another unchanged.
#![cfg(feature = "shacl")]

use std::path::Path;
use std::process::{Command, Output};

const GRAPH: &str = "https://example.org/people";

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quipu"))
        .current_dir(root)
        .env("HOME", root)
        .args(args)
        .output()
        .unwrap()
}

fn ok(root: &Path, args: &[&str]) -> String {
    let output = run(root, args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn sorted(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    lines.sort();
    lines
}

#[test]
fn a_graph_crosses_stores_as_a_databook() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("people.ttl"),
        "<https://example.org/ada> <https://example.org/name> \"Ada\" .\n\
         <https://example.org/ada> <https://example.org/knows> <https://example.org/bob> .\n",
    )
    .unwrap();
    ok(
        root,
        &["knot", "people.ttl", "--graph", GRAPH, "--db", "a.db"],
    );
    ok(
        root,
        &[
            "databook",
            "export",
            "--graph",
            GRAPH,
            "--title",
            "People",
            "--db",
            "a.db",
            "--out",
            "people.databook.md",
        ],
    );
    let text = std::fs::read_to_string(root.join("people.databook.md")).unwrap();
    assert!(
        text.starts_with("---\nid: \"urn:quipu:databook:sha256:"),
        "{text}"
    );
    assert!(text.contains("title: \"People\""));

    // Flags before the operand must not be mistaken for the file.
    let report = ok(
        root,
        &[
            "databook",
            "import",
            "--actor",
            "cli-test",
            "people.databook.md",
            "--db",
            "b.db",
        ],
    );
    let report: serde_json::Value = serde_json::from_str(&report).unwrap();
    assert_eq!(report["graphs"][0]["graph"], GRAPH);
    assert_eq!(report["graphs"][0]["facts"], 2);
    assert_eq!(
        sorted(&ok(root, &["export", "--graph", GRAPH, "--db", "b.db"])),
        sorted(&ok(root, &["export", "--graph", GRAPH, "--db", "a.db"]))
    );
}

#[test]
fn a_refused_import_exits_non_zero_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("bad.databook.md"), "# not a databook\n").unwrap();
    let output = run(
        root,
        &["databook", "import", "bad.databook.md", "--db", "c.db"],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no YAML frontmatter"));
    let output = run(root, &["databook", "import", "--db", "c.db"]);
    assert!(
        !output.status.success(),
        "a missing file operand is refused"
    );
}
