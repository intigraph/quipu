//! aegis-yavo9c: emit / propose / apply on the merge that cannot auto-merge.

use std::path::{Path, PathBuf};

use oxrdfio::RdfFormat;

use super::*;
use crate::store::Store;

const S: &str = "https://example.org/s";
const STATUS: &str = "https://example.org/status";
const TAG: &str = "https://example.org/tag";
const LABEL: &str = "http://www.w3.org/2000/01/rdf-schema#label";
const SHAPES: &str = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
[] sh:path <https://example.org/status> ; sh:maxCount 1 .
"#;

fn put(store: &mut Store, nt: &str, ts: &str) {
    crate::rdf::ingest_rdf(
        store,
        nt.as_bytes(),
        RdfFormat::NTriples,
        None,
        ts,
        None,
        Some("t"),
    )
    .unwrap();
}

fn set_status(store: &mut Store, value: &str, ts: &str) {
    let e = store.lookup(S).unwrap().unwrap();
    let a = store.lookup(STATUS).unwrap().unwrap();
    store.retract_entity(e, Some(a), ts, None).unwrap();
    put(store, &format!("<{S}> <{STATUS}> \"{value}\" .\n"), ts);
}

fn store_with_base() -> Store {
    seed(Store::open_in_memory().unwrap())
}

fn seed(mut store: Store) -> Store {
    crate::share_scrub::seed_test_catalogue(&mut store);
    store.load_shapes("merge", SHAPES, "2026-10-07").unwrap();
    put(
        &mut store,
        &format!(
            "<{S}> <{STATUS}> \"open\" .\n<{S}> <{TAG}> \"a\" .\n\
             <{STATUS}> <{LABEL}> \"statut\"@fr .\n<{STATUS}> <{LABEL}> \"status\"@en .\n\
             <{S}> <{LABEL}> \"Thing S\" .\n"
        ),
        "2026-10-07T00:00:00Z",
    );
    store
}

/// base "open"; theirs "closed" + a new tag "theirs"; ours "blocked" + "ours".
fn scenario(root: &Path) -> (Store, PathBuf) {
    scenario_with(root, store_with_base())
}

fn scenario_with(root: &Path, mut local: Store) -> (Store, PathBuf) {
    let mut source = store_with_base();
    let base = crate::share::share(
        &source,
        root.join("base").to_str().unwrap(),
        &crate::share::ShareOptions::default(),
    )
    .unwrap();
    set_status(&mut source, "closed", "2026-10-07T00:01:00Z");
    put(
        &mut source,
        &format!("<{S}> <{TAG}> \"theirs\" .\n"),
        "2026-10-07T00:01:00Z",
    );
    let incoming = root.join("incoming");
    crate::share::share(
        &source,
        incoming.to_str().unwrap(),
        &crate::share::ShareOptions {
            parent_share: Some(base.share_id),
            ..Default::default()
        },
    )
    .unwrap();
    set_status(&mut local, "blocked", "2026-10-07T00:02:00Z");
    put(
        &mut local,
        &format!("<{S}> <{TAG}> \"ours\" .\n"),
        "2026-10-07T00:02:00Z",
    );
    (local, incoming)
}

fn values(store: &Store, predicate: &str) -> Vec<String> {
    let (graph, _) = crate::share_merge::root_graph(store).unwrap();
    let mut out: Vec<String> = graph
        .iter()
        .filter(|t| t.predicate.as_str() == predicate)
        .map(|t| t.object.to_string())
        .collect();
    out.sort();
    out
}

fn decide(file: &mut DecisionFile, decision: &Decision) -> Vec<u8> {
    for row in &mut file.rows {
        row.decision = Some(decision.clone());
    }
    serde_json::to_vec_pretty(file).unwrap()
}

fn fact_count(store: &Store) -> usize {
    crate::share_merge::root_graph(store).unwrap().0.len()
}

#[test]
fn emit_writes_nothing_and_binds_the_conflict_to_its_inputs() {
    let root = tempfile::tempdir().unwrap();
    let (local, incoming) = scenario(root.path());
    let before = fact_count(&local);
    let file = emit(&local, &incoming).unwrap();
    assert_eq!(file.schema, DECISIONS_SCHEMA_V1);
    assert!(file.rows.iter().all(|r| r.local_slot_hash.is_none()));
    assert_eq!(fact_count(&local), before);
    assert_eq!(file.rows.len(), 1);
    let row = &file.rows[0];
    assert_eq!(row.record.base, ["\"open\""]);
    assert_eq!(row.record.ours, ["\"blocked\""]);
    assert_eq!(row.record.theirs, ["\"closed\""]);
    assert_eq!(row.provenance.ours.len(), 1);
    assert_eq!(row.provenance.ours[0].value, "\"blocked\"");
    assert!(row.decision.is_none() && row.proposal.is_none());
}

#[test]
fn propose_fills_evidence_and_never_decides() {
    let root = tempfile::tempdir().unwrap();
    let (local, incoming) = scenario(root.path());
    let proposed = propose(emit(&local, &incoming).unwrap());
    let row = &proposed.rows[0];
    assert!(row.decision.is_none(), "an agent proposal must not decide");
    let p = row.proposal.as_ref().unwrap();
    assert!(!p.evidence.is_empty());
    assert!(
        p.evidence.iter().any(|e| e.contains("operator decides")),
        "{p:?}"
    );
}

#[test]
fn deciding_theirs_finishes_the_merge_in_one_attributed_transaction() {
    let root = tempfile::tempdir().unwrap();
    let (mut local, incoming) = scenario(root.path());
    let mut file = emit(&local, &incoming).unwrap();
    let bytes = decide(
        &mut file,
        &Decision::Choose {
            choose: Side::Theirs,
        },
    );
    let out = apply(
        &mut local,
        &incoming,
        &file,
        &bytes,
        "stiwi",
        "2026-10-07T01:00:00Z",
        Some("agent"),
    )
    .unwrap();
    assert_eq!(out.merge.outcome, "merged");
    assert_eq!(values(&local, STATUS), ["\"closed\""]);
    // The clean part lands too: both sides' tags, the base tag kept.
    assert_eq!(values(&local, TAG), ["\"a\"", "\"ours\"", "\"theirs\""]);
    let tx = local
        .get_transaction(out.merge.tx_id.unwrap())
        .unwrap()
        .unwrap();
    let source = tx.source.unwrap();
    assert!(source.contains("reviewer=stiwi"), "{source}");
    assert!(
        source.contains(&format!("decisions={}", out.decisions_sha256)),
        "{source}"
    );
    assert_eq!(out.applied.len(), 1);
}

#[test]
fn an_edited_value_is_applied_and_an_invalid_one_refused() {
    let root = tempfile::tempdir().unwrap();
    let (mut local, incoming) = scenario(root.path());
    let mut file = emit(&local, &incoming).unwrap();
    let bad = decide(
        &mut file,
        &Decision::Values {
            values: vec!["not a term".into()],
        },
    );
    let before = values(&local, STATUS);
    assert!(
        apply(
            &mut local,
            &incoming,
            &file,
            &bad,
            "r",
            "2026-10-07T01:00:00Z",
            None
        )
        .is_err()
    );
    assert_eq!(
        values(&local, STATUS),
        before,
        "a refused apply writes nothing"
    );
    let good = decide(
        &mut file,
        &Decision::Values {
            values: vec!["\"resolved\"".into()],
        },
    );
    apply(
        &mut local,
        &incoming,
        &file,
        &good,
        "r",
        "2026-10-07T01:00:00Z",
        None,
    )
    .unwrap();
    assert_eq!(values(&local, STATUS), ["\"resolved\""]);
}

#[test]
fn refusals_write_nothing() {
    let root = tempfile::tempdir().unwrap();
    let (mut local, incoming) = scenario(root.path());
    let emitted = emit(&local, &incoming).unwrap();
    let before = (values(&local, STATUS), values(&local, TAG));
    let attempt = |local: &mut Store, file: &DecisionFile| {
        let bytes = serde_json::to_vec(file).unwrap();
        apply(
            local,
            &incoming,
            file,
            &bytes,
            "r",
            "2026-10-07T01:00:00Z",
            None,
        )
        .unwrap_err()
        .to_string()
    };

    // Undecided.
    let err = attempt(&mut local, &emitted);
    assert!(err.contains("undecided"), "{err}");

    // Over sh:maxCount.
    let mut over = emitted.clone();
    decide(
        &mut over,
        &Decision::Values {
            values: vec!["\"x\"".into(), "\"y\"".into()],
        },
    );
    let err = attempt(&mut local, &over);
    assert!(err.contains("maxCount"), "{err}");

    // A file for a different conflict set.
    let mut foreign = emitted.clone();
    decide(&mut foreign, &Decision::Choose { choose: Side::Ours });
    foreign.rows[0].record.theirs = vec!["\"other\"".into()];
    let err = attempt(&mut local, &foreign);
    assert!(err.contains("stale"), "{err}");

    assert_eq!((values(&local, STATUS), values(&local, TAG)), before);

    // Legacy files retain their whole-ROOT binding even for unrelated writes.
    let mut decided = emitted.clone();
    decided.schema = DECISIONS_SCHEMA_V1.into();
    for row in &mut decided.rows {
        row.local_slot_hash = None;
    }
    decide(
        &mut decided,
        &Decision::Choose {
            choose: Side::Theirs,
        },
    );
    put(
        &mut local,
        &format!("<{S}> <{TAG}> \"later\" .\n"),
        "2026-10-07T00:30:00Z",
    );
    let moved = (values(&local, STATUS), values(&local, TAG));
    let err = attempt(&mut local, &decided);
    assert!(err.contains("stale") && err.contains("ROOT"), "{err}");
    assert_eq!((values(&local, STATUS), values(&local, TAG)), moved);
}

#[test]
fn a_reordered_file_is_not_stale_and_an_empty_reviewer_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let (mut local, incoming) = scenario(root.path());
    let mut file = emit(&local, &incoming).unwrap();
    let bytes = decide(&mut file, &Decision::Choose { choose: Side::Ours });
    let err = apply(
        &mut local,
        &incoming,
        &file,
        &bytes,
        " ",
        "2026-10-07T01:00:00Z",
        None,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("reviewer"), "{err}");
    file.rows.reverse();
    apply(
        &mut local,
        &incoming,
        &file,
        &bytes,
        "r",
        "2026-10-07T01:00:00Z",
        None,
    )
    .unwrap();
    assert_eq!(values(&local, STATUS), ["\"blocked\""]);
}

fn inline(dir: &Path) -> serde_json::Value {
    let read = |n: &str| std::fs::read_to_string(dir.join(n)).unwrap();
    serde_json::json!({
        "manifest": serde_json::from_str::<serde_json::Value>(&read("manifest.json")).unwrap(),
        "export_ntriples": read("export.nt"),
        "shapes_turtle": read("shapes.ttl"),
    })
}

#[test]
fn inline_shares_emit_what_the_directory_emits_and_apply_through_the_tool() {
    let root = tempfile::tempdir().unwrap();
    let (mut local, incoming) = scenario(root.path());
    let input = serde_json::json!({
        "incoming": inline(&incoming),
        "base": inline(&root.path().join("base")),
        "propose": true,
    });
    let from_dir = propose(emit(&local, &incoming).unwrap());
    let tool = crate::mcp::merge_decisions::tool_merge_decisions(&local, &input).unwrap();
    assert_eq!(serde_json::to_value(&from_dir).unwrap(), tool);

    let mut decisions = tool;
    decisions["rows"][0]["decision"] = serde_json::json!({"choose": "theirs"});
    let apply_input = serde_json::json!({
        "incoming": input["incoming"],
        "base": input["base"],
        "decisions": decisions,
        "reviewer": "stiwi",
    });
    let out = crate::mcp::merge_decisions::tool_merge_apply(&mut local, &apply_input).unwrap();
    assert_eq!(out["outcome"], "merged");
    assert_eq!(out["reviewer"], "stiwi");
    assert_eq!(values(&local, STATUS), ["\"closed\""]);
}

#[test]
fn inline_shares_are_verified_and_must_be_parent_and_child() {
    let root = tempfile::tempdir().unwrap();
    let (local, incoming) = scenario(root.path());
    let base = inline(&root.path().join("base"));
    let mut tampered = inline(&incoming);
    tampered["export_ntriples"] =
        serde_json::json!("<https://example.org/x> <https://example.org/y> \"z\" .\n");
    let err = crate::mcp::merge_decisions::tool_merge_decisions(
        &local,
        &serde_json::json!({"incoming": tampered, "base": base}),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("hash mismatch"), "{err}");
    // The base passed as its own child: not its parent.
    let err = crate::mcp::merge_decisions::tool_merge_decisions(
        &local,
        &serde_json::json!({"incoming": base, "base": inline(&incoming)}),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("parent"), "{err}");
}

// aegis-yavo9c item 6: what a person reads, and what a tool may not add.

#[test]
fn emit_carries_labels_kind_and_the_rule_in_words() {
    let root = tempfile::tempdir().unwrap();
    let (local, incoming) = scenario(root.path());
    let file = emit(&local, &incoming).unwrap();
    assert_eq!(
        file.labels.get(STATUS).map(String::as_str),
        Some("status"),
        "@en over @fr"
    );
    assert_eq!(
        file.labels.get(S).map(String::as_str),
        Some("Thing S"),
        "untagged first"
    );
    let row = &file.rows[0];
    assert_eq!(row.kind, Some(ConflictKind::MaxCountExceeded));
    assert_eq!(
        row.rule.as_deref(),
        Some("sh:maxCount 1 on status: ours and theirs together hold 2 values")
    );
}

#[test]
fn a_one_sided_delete_against_a_replacement_is_its_own_kind() {
    let record = DecisionRecord {
        subject: format!("<{S}>"),
        predicate: STATUS.into(),
        max_count: 1,
        base: vec!["\"open\"".into()],
        ours: vec![],
        theirs: vec!["\"closed\"".into()],
    };
    assert_eq!(
        crate::share_merge_decisions_view::kind(&record),
        ConflictKind::DeleteReplace
    );
    let rule = crate::share_merge_decisions_view::rule(&record, &Default::default());
    assert!(rule.contains("deleted"), "{rule}");
}

#[test]
fn decided_by_is_round_tripped_and_an_unknown_field_refuses() {
    let root = tempfile::tempdir().unwrap();
    let (mut local, incoming) = scenario(root.path());
    let mut file = emit(&local, &incoming).unwrap();
    file.rows[0].decision = Some(Decision::Choose {
        choose: Side::Theirs,
    });
    file.rows[0].decided_by = Some("page:stiwi".into());
    file.rows[0].decided_at = Some("2026-10-07T23:00:00Z".into());

    let mut extra: serde_json::Value = serde_json::to_value(&file).unwrap();
    extra["rows"][0]["confidence"] = serde_json::json!("high");
    let bytes = serde_json::to_vec(&extra).unwrap();
    let before = values(&local, STATUS);
    let err = apply(
        &mut local,
        &incoming,
        &file,
        &bytes,
        "r",
        "2026-10-07T01:00:00Z",
        None,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("unknown field 'confidence'"), "{err}");
    assert_eq!(values(&local, STATUS), before, "refused: nothing written");

    let bytes = serde_json::to_vec(&file).unwrap();
    let out = apply(
        &mut local,
        &incoming,
        &file,
        &bytes,
        "r",
        "2026-10-07T01:00:00Z",
        None,
    )
    .unwrap();
    assert_eq!(out.applied[0].decided_by.as_deref(), Some("page:stiwi"));
    assert_eq!(
        out.applied[0].decided_at.as_deref(),
        Some("2026-10-07T23:00:00Z")
    );
}

#[test]
fn dry_run_reports_what_apply_then_writes_and_writes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let (mut local, incoming) = scenario(root.path());
    let mut file = emit(&local, &incoming).unwrap();
    let bytes = decide(
        &mut file,
        &Decision::Choose {
            choose: Side::Theirs,
        },
    );
    let before = (values(&local, STATUS), values(&local, TAG));
    // A dry run refuses what apply refuses: an undecided file.
    let undecided = emit(&local, &incoming).unwrap();
    let raw = serde_json::to_vec(&undecided).unwrap();
    let err = dry_run(&mut local, &incoming, &undecided, &raw, "r")
        .unwrap_err()
        .to_string();
    assert!(err.contains("undecided"), "{err}");
    let dry = dry_run(&mut local, &incoming, &file, &bytes, "r").unwrap();
    assert_eq!(dry.merge.outcome, "dry-run");
    assert!(dry.merge.tx_id.is_none());
    assert_eq!((values(&local, STATUS), values(&local, TAG)), before);
    // The same file is still fresh after a dry run, and applies as reported.
    let real = apply(
        &mut local,
        &incoming,
        &file,
        &bytes,
        "r",
        "2026-10-07T01:00:00Z",
        None,
    )
    .unwrap();
    assert_eq!(
        (dry.merge.asserted, dry.merge.retracted),
        (real.merge.asserted, real.merge.retracted)
    );
}

/// wu [wu-quipu435-review]: a decided value is spliced into one N-Triples
/// line, so a newline in it must not smuggle a second triple in, either onto
/// an unrelated subject or into the decided slot past sh:maxCount.
#[test]
fn a_value_cannot_smuggle_a_second_triple() {
    const VICTIM: &str = "https://example.org/victim";
    let root = tempfile::tempdir().unwrap();
    let (mut local, incoming) = scenario(root.path());
    let emitted = emit(&local, &incoming).unwrap();
    let before = fact_count(&local);
    for smuggled in [
        format!("\"x\" .\n<{VICTIM}> <{TAG}> \"injected\""),
        format!("\"x\" .\n<{S}> <{STATUS}> \"y\""),
        format!("\"x\" .\r<{VICTIM}> <{TAG}> \"injected\""),
    ] {
        let mut file = emitted.clone();
        let bytes = decide(
            &mut file,
            &Decision::Values {
                values: vec![smuggled.clone()],
            },
        );
        let err = apply(
            &mut local,
            &incoming,
            &file,
            &bytes,
            "r",
            "2026-10-07T01:00:00Z",
            None,
        )
        .expect_err(&format!("{smuggled:?} must be refused"))
        .to_string();
        assert!(err.contains("one RDF term"), "{err}");
        assert!(values(&local, TAG).iter().all(|v| v != "\"injected\""));
        assert_eq!(fact_count(&local), before, "a refused apply writes nothing");
    }
}

/// wu [wu-quipu435-review] note 1: `/merge/decisions` is served from the READ
/// pool, whose connections are `SQLITE_OPEN_READ_ONLY`. Emit and propose must
/// therefore succeed there, and say exactly what the writer says.
#[test]
fn decisions_are_emitted_from_a_read_only_connection() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("store.db");
    let path = path.to_str().unwrap();
    let (local, incoming) = scenario_with(root.path(), seed(Store::open(path).unwrap()));
    let input = serde_json::json!({
        "incoming": inline(&incoming),
        "base": inline(&root.path().join("base")),
        "propose": true,
    });
    let from_writer = crate::mcp::merge_decisions::tool_merge_decisions(&local, &input).unwrap();
    let mut reader = Store::open_read_only(path).unwrap();
    reader.adopt_read_config_from(&local);
    let from_reader = crate::mcp::merge_decisions::tool_merge_decisions(&reader, &input).unwrap();
    assert_eq!(from_reader, from_writer);
    assert!(!from_reader["rows"].as_array().unwrap().is_empty());
}

#[test]
fn scoped_decisions_survive_unrelated_writes_and_preserve_them() {
    let root = tempfile::tempdir().unwrap();
    let (mut local, incoming) = scenario(root.path());
    let mut file = emit_scoped(&local, &incoming).unwrap();
    assert_eq!(file.schema, DECISIONS_SCHEMA);
    assert!(file.rows.iter().all(|r| r.local_slot_hash.is_some()));
    let bytes = decide(
        &mut file,
        &Decision::Choose {
            choose: Side::Theirs,
        },
    );
    put(
        &mut local,
        "<urn:unrelated> <urn:p> \"later\" .",
        "2026-10-07T00:30:00Z",
    );
    put(
        &mut local,
        &format!("<{S}> <{TAG}> \"same subject, other predicate\" ."),
        "2026-10-07T00:30:01Z",
    );
    apply(
        &mut local,
        &incoming,
        &file,
        &bytes,
        "r",
        "2026-10-07T01:00:00Z",
        None,
    )
    .unwrap();
    assert_eq!(values(&local, STATUS), ["\"closed\""]);
    assert_eq!(values(&local, "urn:p"), ["\"later\""]);
    assert!(values(&local, TAG).contains(&"\"same subject, other predicate\"".to_string()));
}

#[test]
fn scoped_decisions_refuse_changed_slot_and_same_value_new_provenance() {
    for value in ["different", "blocked"] {
        let root = tempfile::tempdir().unwrap();
        let (mut local, incoming) = scenario(root.path());
        let mut file = emit_scoped(&local, &incoming).unwrap();
        let bytes = decide(
            &mut file,
            &Decision::Choose {
                choose: Side::Theirs,
            },
        );
        set_status(&mut local, value, "2026-10-07T00:30:00Z");
        let before = root_graph(&local).unwrap();
        assert!(
            apply(
                &mut local,
                &incoming,
                &file,
                &bytes,
                "r",
                "2026-10-07T01:00:00Z",
                None
            )
            .is_err()
        );
        assert_eq!(root_graph(&local).unwrap(), before);
    }
}

#[test]
fn scoped_decisions_refuse_missing_or_forged_slot_hash() {
    for hash in [None, Some("sha256:invalid".into())] {
        let root = tempfile::tempdir().unwrap();
        let (mut local, incoming) = scenario(root.path());
        let mut file = emit_scoped(&local, &incoming).unwrap();
        file.rows[0].local_slot_hash = hash;
        let bytes = decide(
            &mut file,
            &Decision::Choose {
                choose: Side::Theirs,
            },
        );
        let before = root_graph(&local).unwrap();
        let err = apply(
            &mut local,
            &incoming,
            &file,
            &bytes,
            "r",
            "2026-10-07T01:00:00Z",
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("reviewed slot"));
        assert_eq!(root_graph(&local).unwrap(), before);
    }
}

#[test]
fn scoped_decisions_refuse_changed_incoming_bindings() {
    for field in ["incoming", "base", "graph"] {
        let root = tempfile::tempdir().unwrap();
        let (mut local, incoming) = scenario(root.path());
        let mut file = emit_scoped(&local, &incoming).unwrap();
        match field {
            "incoming" => file.incoming_share.push_str("-changed"),
            "base" => file.base_share.push_str("-changed"),
            _ => file.incoming_graph_hash.push_str("-changed"),
        }
        let bytes = decide(
            &mut file,
            &Decision::Choose {
                choose: Side::Theirs,
            },
        );
        let before = root_graph(&local).unwrap();
        assert!(
            apply(
                &mut local,
                &incoming,
                &file,
                &bytes,
                "r",
                "2026-10-07T01:00:00Z",
                None
            )
            .is_err()
        );
        assert_eq!(root_graph(&local).unwrap(), before);
    }
}

#[test]
fn scoped_decisions_refuse_new_conflicts_after_emit() {
    let root = tempfile::tempdir().unwrap();
    let mut source = store_with_base();
    let mut local = store_with_base();
    for store in [&mut source, &mut local] {
        put(
            store,
            "<urn:second> <https://example.org/status> \"open\" .",
            "2026-10-07T00:00:01Z",
        );
    }
    let base = crate::share::share(
        &source,
        root.path().join("base").to_str().unwrap(),
        &Default::default(),
    )
    .unwrap();
    set_status(&mut source, "closed", "2026-10-07T00:01:00Z");
    let second = source.lookup("urn:second").unwrap().unwrap();
    let status = source.lookup(STATUS).unwrap().unwrap();
    source
        .retract_entity(second, Some(status), "2026-10-07T00:01:00Z", None)
        .unwrap();
    put(
        &mut source,
        "<urn:second> <https://example.org/status> \"closed\" .",
        "2026-10-07T00:01:00Z",
    );
    let incoming = root.path().join("incoming");
    crate::share::share(
        &source,
        incoming.to_str().unwrap(),
        &crate::share::ShareOptions {
            parent_share: Some(base.share_id),
            ..Default::default()
        },
    )
    .unwrap();
    set_status(&mut local, "blocked", "2026-10-07T00:02:00Z");
    let mut file = emit_scoped(&local, &incoming).unwrap();
    assert_eq!(file.rows.len(), 1);
    let bytes = decide(
        &mut file,
        &Decision::Choose {
            choose: Side::Theirs,
        },
    );
    let second = local.lookup("urn:second").unwrap().unwrap();
    let status = local.lookup(STATUS).unwrap().unwrap();
    local
        .retract_entity(second, Some(status), "2026-10-07T00:03:00Z", None)
        .unwrap();
    put(
        &mut local,
        "<urn:second> <https://example.org/status> \"blocked\" .",
        "2026-10-07T00:03:00Z",
    );
    let before = root_graph(&local).unwrap();
    let err = apply(
        &mut local,
        &incoming,
        &file,
        &bytes,
        "r",
        "2026-10-07T01:00:00Z",
        None,
    )
    .unwrap_err();
    assert!(err.to_string().contains("set of conflicts"));
    assert_eq!(root_graph(&local).unwrap(), before);
}

#[test]
fn legacy_decisions_still_apply_without_a_slot_hash_when_root_is_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let (mut local, incoming) = scenario(root.path());
    let mut file = emit(&local, &incoming).unwrap();
    file.schema = DECISIONS_SCHEMA_V1.into();
    for row in &mut file.rows {
        row.local_slot_hash = None;
    }
    let bytes = decide(
        &mut file,
        &Decision::Choose {
            choose: Side::Theirs,
        },
    );
    apply(
        &mut local,
        &incoming,
        &file,
        &bytes,
        "r",
        "2026-10-07T01:00:00Z",
        None,
    )
    .unwrap();
    assert_eq!(values(&local, STATUS), ["\"closed\""]);
}

#[test]
fn scoped_decisions_refuse_empty_slot_aba() {
    let root = tempfile::tempdir().unwrap();
    let (mut local, incoming) = scenario(root.path());
    let entity = local.lookup(S).unwrap().unwrap();
    let predicate = local.lookup(STATUS).unwrap().unwrap();
    local
        .retract_entity(entity, Some(predicate), "2026-10-07T00:03:00Z", None)
        .unwrap();
    let mut file = emit_scoped(&local, &incoming).unwrap();
    assert!(file.rows[0].record.ours.is_empty());
    let bytes = decide(
        &mut file,
        &Decision::Choose {
            choose: Side::Theirs,
        },
    );
    put(
        &mut local,
        &format!("<{S}> <{STATUS}> \"temporary\" ."),
        "2026-10-07T00:04:00Z",
    );
    local
        .retract_entity(entity, Some(predicate), "2026-10-07T00:05:00Z", None)
        .unwrap();
    let before = root_graph(&local).unwrap();
    let result = dry_run(&mut local, &incoming, &file, &bytes, "reviewer");
    assert!(result.is_err(), "empty -> value -> empty must be stale");
    assert_eq!(root_graph(&local).unwrap(), before);
}

#[test]
fn inline_slot_scope_requires_explicit_boolean_opt_in() {
    let root = tempfile::tempdir().unwrap();
    let (local, incoming) = scenario(root.path());
    let mut input = serde_json::json!({
        "incoming": inline(&incoming),
        "base": inline(&root.path().join("base")),
    });
    let emit_inline = |input: &serde_json::Value| {
        crate::mcp::merge_decisions::tool_merge_decisions(&local, input)
    };
    assert_eq!(emit_inline(&input).unwrap()["schema"], DECISIONS_SCHEMA_V1);
    input["slot_scoped"] = serde_json::json!(true);
    assert_eq!(
        emit_inline(&input).unwrap(),
        serde_json::to_value(emit_scoped(&local, &incoming).unwrap()).unwrap()
    );
    input["slot_scoped"] = serde_json::json!(false);
    assert_eq!(emit_inline(&input).unwrap()["schema"], DECISIONS_SCHEMA_V1);
    input["slot_scoped"] = serde_json::json!("true");
    assert!(emit_inline(&input).is_err());
}

/// aegis-k7l: show -> resolve each choice through the shared verb -> apply ->
/// the shape holds -> re-applying the same file is a no-op.
#[test]
fn reconcile_each_choice_end_to_end_and_reapply_is_a_noop() {
    use crate::merge_reconcile::{Choice, ReconcileFile, Surface, render_text};
    let cases = [
        (Choice::Side(Side::Ours), "\"blocked\""),
        (Choice::Side(Side::Theirs), "\"closed\""),
        (Choice::Side(Side::Base), "\"open\""),
        (Choice::Values(vec!["\"wontfix\"".into()]), "\"wontfix\""),
    ];
    for (choice, expected) in cases {
        let root = tempfile::tempdir().unwrap();
        let (mut local, incoming) = scenario(root.path());
        let mut file = ReconcileFile::Share(propose(emit_scoped(&local, &incoming).unwrap()));
        let text = render_text(&file.view("d.json", Surface::Cli));
        assert!(text.contains("[d1] Thing S · status"), "{text}");
        assert!(
            text.contains("Next: quipu merge resolve d.json d1"),
            "{text}"
        );
        file.resolve("d1", choice.clone(), "steve", "2026-10-07T00:03:00Z")
            .unwrap();
        let bytes = file.to_bytes().unwrap();
        let ReconcileFile::Share(decided) = ReconcileFile::load(&bytes).unwrap() else {
            panic!("not a share file")
        };
        let out = apply(
            &mut local,
            &incoming,
            &decided,
            &bytes,
            "steve",
            "2026-10-07T00:04:00Z",
            None,
        )
        .unwrap();
        assert_eq!(out.merge.outcome, "merged", "{choice:?}");
        assert_eq!(values(&local, STATUS), [expected], "{choice:?}");
        assert_eq!(values(&local, TAG), ["\"a\"", "\"ours\"", "\"theirs\""]);
        assert_eq!(out.applied[0].decided_by.as_deref(), Some("steve"));

        let before = local.latest_tx_id().unwrap();
        let again = apply(
            &mut local,
            &incoming,
            &decided,
            &bytes,
            "steve",
            "2026-10-07T00:05:00Z",
            None,
        )
        .unwrap();
        assert_eq!(again.merge.outcome, "already-applied");
        assert_eq!(again.merge.tx_id, out.merge.tx_id);
        assert_eq!(local.latest_tx_id().unwrap(), before);
        assert_eq!(values(&local, STATUS), [expected]);
    }
}

#[test]
fn both_takes_the_union_and_is_refused_above_max_count() {
    let root = tempfile::tempdir().unwrap();
    let (mut local, incoming) = scenario(root.path());
    let mut file = emit(&local, &incoming).unwrap();
    let bytes = decide(&mut file, &Decision::Choose { choose: Side::Both });
    let err = apply(
        &mut local,
        &incoming,
        &file,
        &bytes,
        "steve",
        "2026-10-07T00:03:00Z",
        None,
    )
    .unwrap_err();
    assert!(err.to_string().contains("above sh:maxCount 1"), "{err}");
    assert_eq!(
        union(&["\"b\"".into(), "\"a\"".into()], &["\"a\"".into()]),
        ["\"a\"", "\"b\""]
    );
}
