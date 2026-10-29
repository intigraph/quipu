//! Snapshot tests of the standard presentation, and the shared resolve.

use super::*;
use crate::fork_merge_decisions::{ForkDecisionRow, ForkProvenance};
use crate::share_merge_decisions::Proposal;

fn fact(value: &str, actor: &str, at: &str, tx: i64) -> LocalFact {
    LocalFact {
        value: value.into(),
        valid_from: at.into(),
        tx,
        actor: Some(actor.into()),
        source: None,
    }
}

fn fixture() -> ReconcileFile {
    let record = DecisionRecord {
        subject: "<urn:t:t42>".into(),
        predicate: "urn:t:status".into(),
        max_count: 1,
        base: vec!["\"open\"".into()],
        ours: vec!["\"blocked\"".into()],
        theirs: vec!["\"closed\"".into()],
    };
    let labels = BTreeMap::from([
        ("urn:t:t42".to_string(), "Ticket 42".to_string()),
        ("urn:t:status".to_string(), "status".to_string()),
    ]);
    ReconcileFile::Fork(ForkDecisionFile {
        schema: FORK_DECISIONS_SCHEMA.into(),
        fork: "triage".into(),
        fork_tx: 117,
        rows: vec![ForkDecisionRow {
            id: "d1".into(),
            provenance: ForkProvenance {
                ours: vec![fact("\"blocked\"", "alice", "2026-10-27T14:02:00Z", 118)],
                theirs: vec![fact(
                    "\"closed\"",
                    "triage-bot",
                    "2026-10-27T15:40:00Z",
                    131,
                )],
                base_tx: 117,
            },
            root_slot_hash: "r".into(),
            fork_slot_hash: "f".into(),
            proposal: Some(Proposal {
                choose: Side::Theirs,
                evidence: vec![
                    "theirs is newer: fork tx 131 >= ROOT tx 118".into(),
                    "mechanical proposal: the operator decides".into(),
                ],
            }),
            decision: None,
            kind: Some(kind(&record)),
            rule: Some(rule(&record, &labels)),
            decided_by: None,
            decided_at: None,
            record,
        }],
        labels,
    })
}

const TEXT: &str = "\
Merge needs your decision: fork 'triage' -> ROOT
1 conflict, 1 undecided. Nothing has been written.

[d1] Ticket 42 · status
  rule     sh:maxCount 1 on status: ours and theirs together hold 2 values
  base     \"open\"
  ours     \"blocked\"  (alice, 2026-10-27T14:02:00Z, tx 118)
  theirs   \"closed\"  (triage-bot, 2026-10-27T15:40:00Z, tx 131)
  suggest  theirs: theirs is newer: fork tx 131 >= ROOT tx 118
  choose   1) ours  2) theirs  3) base  4) value  5) defer
  decided  (none)

Next: quipu merge resolve f.json d1 --choose <ours|theirs|base|value|defer> --by <you>
Then: quipu fork promote triage --decisions f.json --reviewer <you> (add --dry-run first)
";

const MARKDOWN: &str = "\
**Merge needs your decision:** fork 'triage' -> ROOT
1 conflict, 1 undecided. Nothing has been written.

#### d1. Ticket 42 · status

| | Value | Changed by |
|---|---|---|
| base | `\"open\"` | |
| ours | `\"blocked\"` | alice, 2026-10-27T14:02:00Z, tx 118 |
| theirs | `\"closed\"` | triage-bot, 2026-10-27T15:40:00Z, tx 131 |

Rule: sh:maxCount 1 on status: ours and theirs together hold 2 values
Suggested: **theirs**: theirs is newer: fork tx 131 >= ROOT tx 118
Decided: (none)

1. ours
2. theirs
3. base
4. value
5. defer

Next: `quipu merge resolve f.json d1 --choose <ours|theirs|base|value|defer> --by <you>`
Then: `quipu fork promote triage --decisions f.json --reviewer <you> (add --dry-run first)`
";

#[test]
fn text_layout_snapshot() {
    let view = fixture().view("f.json", Surface::Cli);
    assert_eq!(render_text(&view), TEXT);
}

#[test]
fn markdown_layout_snapshot() {
    let view = fixture().view("f.json", Surface::Cli);
    assert_eq!(render_markdown(&view), MARKDOWN);
}

#[test]
fn json_layout_snapshot() {
    let view = fixture().view("f.json", Surface::Mcp);
    let json = serde_json::to_value(&view).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "schema": VIEW_SCHEMA,
            "merge": { "kind": "fork", "title": "fork 'triage' -> ROOT" },
            "conflicts": [{
                "id": "d1",
                "entity": { "iri": "<urn:t:t42>", "label": "Ticket 42" },
                "field": { "iri": "urn:t:status", "label": "status" },
                "rule": {
                    "kind": "max_count_exceeded",
                    "max_count": 1,
                    "text": "sh:maxCount 1 on status: ours and theirs together hold 2 values"
                },
                "base": ["\"open\""],
                "ours": { "values": ["\"blocked\""], "changed_by": ["alice, 2026-10-27T14:02:00Z, tx 118"] },
                "theirs": { "values": ["\"closed\""], "changed_by": ["triage-bot, 2026-10-27T15:40:00Z, tx 131"] },
                "suggestion": { "choose": "theirs", "reason": "theirs is newer: fork tx 131 >= ROOT tx 118" },
                "choices": ["ours", "theirs", "base", "value", "defer"],
                "decision": null
            }],
            "undecided": 1,
            "next": [
                "quipu_merge_resolve with the decisions, row: \"d1\", choose and by",
                "quipu_merge_apply with fork: \"triage\", the decisions and reviewer (dry_run: true first)"
            ]
        })
    );
}

#[test]
fn a_decided_view_says_ready_and_names_only_apply() {
    let mut file = fixture();
    file.resolve(
        "d1",
        Choice::Side(Side::Theirs),
        "steve",
        "2026-10-28T00:00:00Z",
    )
    .unwrap();
    let view = file.view("f.json", Surface::Cli);
    assert_eq!(view.undecided, 0);
    let text = render_text(&view);
    assert!(
        text.starts_with(
            "Merge decided, ready to apply: fork 'triage' -> ROOT\n1 conflict, 0 undecided."
        ),
        "{text}"
    );
    assert!(text.contains("  decided  theirs (by steve)\n"), "{text}");
    assert!(
        text.contains("\nNext: quipu fork promote triage --decisions f.json"),
        "{text}"
    );
}

#[test]
fn resolve_validates_the_choice_against_the_row() {
    let mut file = fixture();
    let at = "2026-10-28T00:00:00Z";
    assert!(file.resolve("d9", Choice::Defer, "steve", at).is_err());
    assert!(file.resolve("d1", Choice::Defer, " ", at).is_err());
    let err = file
        .resolve(
            "d1",
            Choice::Values(vec!["\"a\"".into(), "\"b\"".into()]),
            "steve",
            at,
        )
        .unwrap_err();
    assert!(err.to_string().contains("above sh:maxCount 1"), "{err}");
    let err = file
        .resolve(
            "d1",
            Choice::Values(vec!["\"a\" .\n<urn:x> <urn:y> \"z\"".into()]),
            "steve",
            at,
        )
        .unwrap_err();
    assert!(err.to_string().contains("not one RDF term"), "{err}");
    file.resolve(
        "d1",
        Choice::Values(vec!["\"wontfix\"".into()]),
        "steve",
        at,
    )
    .unwrap();
    file.resolve("d1", Choice::Defer, "steve", at).unwrap();
    let view = file.view("f.json", Surface::Cli);
    assert_eq!(view.undecided, 1);
    assert!(render_text(&view).contains("  decided  (deferred by steve)\n"));
}

#[test]
fn choices_parse_and_refuse_mixtures() {
    assert_eq!(
        Choice::parse(Some("ours"), vec![]).unwrap(),
        Choice::Side(Side::Ours)
    );
    assert_eq!(Choice::parse(Some("defer"), vec![]).unwrap(), Choice::Defer);
    assert_eq!(
        Choice::parse(None, vec!["\"x\"".into()]).unwrap(),
        Choice::Values(vec!["\"x\"".into()])
    );
    assert!(Choice::parse(Some("ours"), vec!["\"x\"".into()]).is_err());
    assert!(Choice::parse(Some("value"), vec![]).is_err());
    assert!(Choice::parse(Some("mine"), vec![]).is_err());
}

#[test]
fn files_load_by_schema() {
    let bytes = fixture().to_bytes().unwrap();
    assert_eq!(ReconcileFile::load(&bytes).unwrap(), fixture());
    let err = ReconcileFile::load(br#"{"schema":"x"}"#).unwrap_err();
    assert!(
        err.to_string().contains("not a merge decisions file"),
        "{err}"
    );
}

fn git_fixture() -> ReconcileFile {
    use crate::git_merge_alias::AliasProposal;
    ReconcileFile::Git {
        decisions: GitDecisions {
            schema: GIT_DECISIONS_SCHEMA.into(),
            inputs: ["b".into(), "o".into(), "t".into()],
            conflicts: vec![DecisionRecord {
                subject: "<urn:t:t42>".into(),
                predicate: "urn:t:status".into(),
                max_count: 1,
                base: vec!["\"open\"".into()],
                ours: vec!["\"blocked\"".into()],
                theirs: vec!["\"closed\"".into()],
            }],
            aliases: vec![AliasProposal {
                ours: "<urn:t:a>".into(),
                theirs: "<urn:t:b>".into(),
                similarity: 0.95,
            }],
            resolutions: BTreeMap::from([("alias:0".to_string(), "reject".to_string())]),
            resolved_by: BTreeMap::from([(
                "alias:0".to_string(),
                "steve at 2026-10-28T00:00:00Z".to_string(),
            )]),
        },
        dir: "qpack".into(),
        labels: BTreeMap::from([("urn:t:t42".to_string(), "Ticket 42".to_string())]),
    }
}

const GIT_TEXT: &str = "\
Merge needs your decision: qpack 'qpack' (Git merge: MERGE_HEAD -> HEAD)
1 conflict, 1 alias proposal, 1 undecided. Nothing has been written.

[conflict:0] Ticket 42 · urn:t:status
  rule     sh:maxCount 1 on urn:t:status: ours and theirs together hold 2 values
  base     \"open\"
  ours     \"blocked\"  (HEAD)
  theirs   \"closed\"  (MERGE_HEAD)
  choose   1) ours  2) theirs  3) base  4) defer
  decided  (none)

[alias:0] same entity? <urn:t:a> and <urn:t:b> (label similarity 0.950)
  choose   1) accept  2) reject  3) defer
  decided  reject (by steve at 2026-10-28T00:00:00Z)

Next: quipu git-merge resolve qpack conflict:0 --choose <ours|theirs|base|defer> --by <you>
Then: git add qpack && git commit (CI pendant-check re-runs the merge and verifies the result)
";

#[test]
fn git_text_layout_snapshot() {
    let file = git_fixture();
    let text = render_text(&file.view("qpack/decisions.json", Surface::Cli));
    assert_eq!(text, GIT_TEXT);
    // Git choices rewrite the pack, so the pure resolve points at the Git verb.
    let mut file = file;
    let err = file
        .resolve("conflict:0", Choice::Side(Side::Theirs), "steve", "t")
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("quipu git-merge resolve qpack conflict:0"),
        "{err}"
    );
}
