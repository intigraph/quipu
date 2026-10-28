//! aegis-k7l: the Git merge tool uses the shared show / resolve and layout.
#![cfg(feature = "shacl")]
#[path = "support/git_qpack_fixture.rs"]
mod fixture;
use fixture::*;
use serde_json::Value;
use std::process::Output;

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}
#[test]
fn git_merge_prints_the_standard_layout_and_the_shared_resolve_finishes_it() {
    let f = Fixture::new("functional");
    let o = f.merge();
    assert_eq!(o.status.code(), Some(2));
    let text = out(&o);
    assert!(
        text.contains("Merge needs your decision: qpack 'qpack' (Git merge: MERGE_HEAD -> HEAD)"),
        "{text}"
    );
    assert!(text.contains("[conflict:0] "), "{text}");
    assert!(text.contains("  ours     \"ours\"  (HEAD)"), "{text}");
    assert!(
        text.contains(
            "Next: quipu git-merge resolve qpack conflict:0 --choose <ours|theirs|base|defer> --by <you>"
        ),
        "{text}"
    );
    assert!(String::from_utf8_lossy(&o.stderr).contains("qpack/decisions.json"));

    // The same file through `quipu merge show`, as JSON.
    let o = cli(
        f.path(),
        &["merge", "show", "qpack/decisions.json", "--format", "json"],
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let view: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(view["merge"]["kind"], "git");
    assert_eq!(view["undecided"], 1);

    // `both` would exceed sh:maxCount 1: refused, nothing rewritten.
    let before = f.graph();
    let o = cli(
        f.path(),
        &[
            "merge",
            "resolve",
            "qpack/decisions.json",
            "conflict:0",
            "--choose",
            "both",
            "--by",
            "steve",
        ],
    );
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("above sh:maxCount 1"));
    assert_eq!(f.graph(), before);

    // The shared verb, from either entry point, with refs from MERGE_HEAD.
    let o = cli(
        f.path(),
        &[
            "git-merge",
            "resolve",
            "qpack",
            "conflict:0",
            "--choose",
            "theirs",
            "--by",
            "steve",
        ],
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(
        out(&o).contains("  decided  theirs (by steve at "),
        "{}",
        out(&o)
    );
    assert!(
        out(&o).contains("Next: git add qpack && git commit"),
        "{}",
        out(&o)
    );
    let resolved = f.graph();
    assert!(resolved.contains(&format!(
        "<{NS}ability/aaa-tracking> <{NS}effectText> \"theirs\" ."
    )));
    assert_eq!(f.decisions()["resolutions"]["conflict:0"], "theirs");
    // Idempotent: the same choice again rewrites the pack to the same bytes.
    let o = cli(
        f.path(),
        &[
            "merge",
            "resolve",
            "./qpack/decisions.json",
            "conflict:0",
            "--choose",
            "theirs",
            "--by",
            "steve",
        ],
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(f.graph(), resolved);
    f.commit();
    let o = f.check();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}
#[test]
fn defer_reopens_a_git_decision_and_ci_refuses_it() {
    let f = Fixture::new("functional");
    f.merge();
    for choice in ["base", "defer"] {
        let o = cli(
            f.path(),
            &[
                "git-merge",
                "resolve",
                "qpack",
                "conflict:0",
                "--choose",
                choice,
                "--by",
                "steve",
            ],
        );
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    }
    assert!(f.decisions()["resolutions"].get("conflict:0").is_none());
    let o = cli(f.path(), &["git-merge", "show", "qpack"]);
    assert!(
        out(&o).contains("  decided  (deferred by steve at "),
        "{}",
        out(&o)
    );
    f.commit();
    assert!(!f.check().status.success());
}
#[test]
fn an_alias_proposal_is_decided_through_the_shared_verb() {
    let f = Fixture::new("alias");
    let o = f.merge();
    assert_eq!(o.status.code(), Some(2));
    assert!(out(&o).contains("[alias:0] same entity? "), "{}", out(&o));
    let o = cli(
        f.path(),
        &[
            "merge",
            "resolve",
            "qpack/decisions.json",
            "alias:0",
            "--choose",
            "accept",
            "--by",
            "steve",
        ],
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(f.graph().contains("owl#sameAs"));
    f.commit();
    assert!(f.check().status.success());
}
