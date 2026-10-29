//! aegis-k7l: a fork promotion that stopped on conflicts, finished through
//! emit / propose / resolve / apply.

use super::*;
use crate::merge_reconcile::{Choice, ReconcileFile, Surface, render_text};
use crate::store::Datum;
use crate::types::Op;

const TS: &str = "2026-10-27T00:00:00Z";
const T: &str = "urn:t:t42";
const STATUS: &str = "urn:t:status";
const TAG: &str = "urn:t:tag";

const SHAPES: &str = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <urn:t:> .

ex:TicketShape a sh:NodeShape ;
    sh:targetClass ex:Ticket ;
    sh:property [ sh:path ex:status ; sh:maxCount 1 ;
                  sh:in ( "open" "blocked" "closed" "wontfix" ) ] .
"#;

fn datum(e: i64, a: i64, v: Value, op: Op) -> Datum {
    Datum {
        entity: e,
        attribute: a,
        value: v,
        valid_from: TS.to_string(),
        valid_to: None,
        op,
    }
}

fn ids(store: &Store) -> (i64, i64, i64) {
    (
        store.intern(T).unwrap(),
        store.intern(STATUS).unwrap(),
        store.intern(TAG).unwrap(),
    )
}

/// Base: ticket 42 is "open". ROOT (alice) moves it to "blocked"; the fork
/// (triage-bot) moves it to "closed" and adds a tag. Returns the store.
fn scenario() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    let (t, status, _) = ids(&store);
    let rdf_type = store.intern(crate::namespace::RDF_TYPE).unwrap();
    let ticket = store.intern("urn:t:Ticket").unwrap();
    let label = store.intern(RDFS_LABEL).unwrap();
    store
        .transact(
            &[
                datum(t, rdf_type, Value::Ref(ticket), Op::Assert),
                datum(t, label, Value::Str("Ticket 42".into()), Op::Assert),
                datum(t, status, Value::Str("open".into()), Op::Assert),
            ],
            TS,
            Some("seed"),
            None,
        )
        .unwrap();
    store.load_shapes("tickets", SHAPES, TS).unwrap();
    let latest = store.latest_tx_id().unwrap();
    let fork = store.fork_create("triage", latest, TS, None).unwrap();
    store
        .transact(
            &[
                datum(t, status, Value::Str("open".into()), Op::Retract),
                datum(t, status, Value::Str("blocked".into()), Op::Assert),
            ],
            TS,
            Some("alice"),
            Some("cli"),
        )
        .unwrap();
    let (_, _, tag) = ids(&store);
    store
        .transact_to_graph(
            &[
                datum(t, status, Value::Str("open".into()), Op::Retract),
                datum(t, status, Value::Str("closed".into()), Op::Assert),
                datum(t, tag, Value::Str("triaged".into()), Op::Assert),
            ],
            TS,
            Some("triage-bot"),
            None,
            fork.g,
        )
        .unwrap();
    store
}

fn root_values(store: &Store, attribute: i64) -> Vec<Value> {
    let (t, _, _) = ids(store);
    let mut out: Vec<Value> = store
        .entity_facts(t)
        .unwrap()
        .into_iter()
        .filter(|f| f.attribute == attribute)
        .map(|f| f.value)
        .collect();
    out.sort_by_key(|v| format!("{v:?}"));
    out
}

/// Emit, propose, record `choice` through the shared resolve, and return the
/// file's bytes exactly as `quipu merge resolve` writes them.
fn decided(store: &Store, choice: Choice) -> (ForkDecisionFile, Vec<u8>) {
    let mut file = ReconcileFile::Fork(propose(emit(store, "triage").unwrap()));
    file.resolve("d1", choice, "steve", TS).unwrap();
    let bytes = file.to_bytes().unwrap();
    let ReconcileFile::Fork(f) = ReconcileFile::load(&bytes).unwrap() else {
        panic!("not a fork file")
    };
    (f, bytes)
}

#[test]
fn emit_binds_both_sides_and_writes_nothing() {
    let store = scenario();
    let tx = store.latest_tx_id().unwrap();
    let file = emit(&store, "triage").unwrap();
    assert_eq!(file.schema, FORK_DECISIONS_SCHEMA);
    assert_eq!(file.rows.len(), 1);
    let row = &file.rows[0];
    assert_eq!(row.record.base, ["\"open\""]);
    assert_eq!(row.record.ours, ["\"blocked\""]);
    assert_eq!(row.record.theirs, ["\"closed\""]);
    assert_eq!(row.provenance.ours[0].actor.as_deref(), Some("alice"));
    assert_eq!(
        row.provenance.theirs[0].actor.as_deref(),
        Some("triage-bot")
    );
    assert_eq!(file.labels.get(T).map(String::as_str), Some("Ticket 42"));
    assert_ne!(row.root_slot_hash, row.fork_slot_hash);
    assert!(row.decision.is_none());
    assert_eq!(store.latest_tx_id().unwrap(), tx);
    assert_eq!(store.fork_lookup("triage").unwrap().unwrap().status, "open");
}

#[test]
fn propose_takes_the_newer_side_and_never_decides() {
    let store = scenario();
    let file = propose(emit(&store, "triage").unwrap());
    let row = &file.rows[0];
    let p = row.proposal.as_ref().unwrap();
    assert_eq!(p.choose, Side::Theirs, "{:?}", p.evidence);
    assert!(row.decision.is_none());
}

#[test]
fn each_choice_promotes_once_and_a_reapply_is_a_noop() {
    let cases: [(Choice, Value); 4] = [
        (Choice::Side(Side::Ours), Value::Str("blocked".into())),
        (Choice::Side(Side::Theirs), Value::Str("closed".into())),
        (Choice::Side(Side::Base), Value::Str("open".into())),
        (
            Choice::Values(vec!["\"wontfix\"".into()]),
            Value::Str("wontfix".into()),
        ),
    ];
    for (choice, expected) in cases {
        let mut store = scenario();
        let (_, status, tag) = ids(&store);
        let (file, bytes) = decided(&store, choice.clone());
        let out = apply(
            &mut store, "triage", &file, &bytes, "steve", TS, None, false,
        )
        .unwrap();
        assert_eq!(out.outcome, "promoted", "{choice:?}");
        assert_eq!(
            root_values(&store, status),
            std::slice::from_ref(&expected),
            "{choice:?}"
        );
        // The clean part of the promotion landed too.
        assert_eq!(root_values(&store, tag), [Value::Str("triaged".into())]);
        assert_eq!(
            store.fork_lookup("triage").unwrap().unwrap().status,
            "promoted"
        );
        if let Some(tx) = out.tx_id.filter(|tx| *tx > 0) {
            let source = store.get_transaction(tx).unwrap().unwrap().source.unwrap();
            assert!(source.contains("reviewer=steve"), "{source}");
            assert!(source.contains(&out.decisions_sha256), "{source}");
        }
        // Idempotent: the same decisions again write nothing.
        let latest = store.latest_tx_id().unwrap();
        let again = apply(
            &mut store, "triage", &file, &bytes, "steve", TS, None, false,
        )
        .unwrap();
        assert_eq!(again.outcome, "already-applied");
        assert_eq!(again.tx_id, out.tx_id);
        assert_eq!(store.latest_tx_id().unwrap(), latest);
        assert_eq!(root_values(&store, status), [expected]);
    }
}

#[test]
fn both_is_not_offered_within_max_count_one() {
    let store = scenario();
    let mut file = ReconcileFile::Fork(emit(&store, "triage").unwrap());
    let view = file.view("f.json", Surface::Cli);
    assert!(!view.conflicts[0].choices.contains(&"both".to_string()));
    let err = file
        .resolve("d1", Choice::Side(Side::Both), "steve", TS)
        .unwrap_err();
    assert!(err.to_string().contains("above sh:maxCount 1"), "{err}");
}

#[test]
fn undecided_deferred_and_stale_files_refuse_and_write_nothing() {
    let mut store = scenario();
    let (t, status, _) = ids(&store);
    let tx = store.latest_tx_id().unwrap();

    let file = emit(&store, "triage").unwrap();
    let bytes = serde_json::to_vec(&file).unwrap();
    let err = apply(
        &mut store, "triage", &file, &bytes, "steve", TS, None, false,
    )
    .unwrap_err();
    assert!(err.to_string().contains("undecided"), "{err}");

    let (file, bytes) = decided(&store, Choice::Defer);
    let err = apply(
        &mut store, "triage", &file, &bytes, "steve", TS, None, false,
    )
    .unwrap_err();
    assert!(err.to_string().contains("undecided"), "{err}");
    assert_eq!(store.latest_tx_id().unwrap(), tx);

    // ROOT's side moves after the decision: stale.
    let (file, bytes) = decided(&store, Choice::Side(Side::Theirs));
    store
        .transact(
            &[
                datum(t, status, Value::Str("blocked".into()), Op::Retract),
                datum(t, status, Value::Str("open".into()), Op::Assert),
            ],
            TS,
            Some("alice"),
            None,
        )
        .unwrap();
    let tx = store.latest_tx_id().unwrap();
    let err = apply(
        &mut store, "triage", &file, &bytes, "steve", TS, None, false,
    )
    .unwrap_err();
    assert!(err.to_string().contains("stale"), "{err}");
    assert_eq!(store.latest_tx_id().unwrap(), tx);
    assert_eq!(store.fork_lookup("triage").unwrap().unwrap().status, "open");
}

#[cfg(feature = "shacl")]
#[test]
fn a_decided_value_is_gated_by_shacl_and_dry_run_writes_nothing() {
    let mut store = scenario();
    let tx = store.latest_tx_id().unwrap();

    let (file, bytes) = decided(&store, Choice::Values(vec!["\"bogus\"".into()]));
    let err = apply(
        &mut store, "triage", &file, &bytes, "steve", TS, None, false,
    )
    .unwrap_err();
    assert!(err.to_string().contains("SHACL"), "{err}");
    assert_eq!(store.latest_tx_id().unwrap(), tx);

    let (file, bytes) = decided(&store, Choice::Side(Side::Theirs));
    let dry = apply(&mut store, "triage", &file, &bytes, "steve", TS, None, true).unwrap();
    assert_eq!(dry.outcome, "dry-run");
    assert_eq!((dry.asserted, dry.retracted), (2, 1));
    assert_eq!(store.latest_tx_id().unwrap(), tx);
    assert_eq!(store.fork_lookup("triage").unwrap().unwrap().status, "open");
}

#[test]
fn a_foreign_or_extended_file_is_refused() {
    let mut store = scenario();
    let (file, bytes) = decided(&store, Choice::Side(Side::Theirs));
    let err = apply(&mut store, "other", &file, &bytes, "steve", TS, None, false).unwrap_err();
    assert!(err.to_string().contains("for fork 'triage'"), "{err}");

    let mut value: JsonValue = serde_json::from_slice(&bytes).unwrap();
    value["rows"][0]["note"] = "x".into();
    let bytes = serde_json::to_vec(&value).unwrap();
    let err = apply(
        &mut store, "triage", &file, &bytes, "steve", TS, None, false,
    )
    .unwrap_err();
    assert!(err.to_string().contains("unknown field 'note'"), "{err}");
}

#[test]
fn the_cli_view_names_the_next_commands() {
    let store = scenario();
    let file = ReconcileFile::Fork(propose(emit(&store, "triage").unwrap()));
    let text = render_text(&file.view("t.json", Surface::Cli));
    assert!(
        text.starts_with("Merge needs your decision: fork 'triage' -> ROOT\n"),
        "{text}"
    );
    assert!(text.contains("[d1] Ticket 42 · urn:t:status"), "{text}");
    assert!(
        text.contains(
            "Next: quipu merge resolve t.json d1 --choose <ours|theirs|base|value|defer> --by <you>"
        ),
        "{text}"
    );
    assert!(
        text.contains("Then: quipu fork promote triage --decisions t.json --reviewer <you>"),
        "{text}"
    );
}

/// aegis-gxfm: a decision that only REMOVES values used to skip SHACL, since
/// write-time validation judges what is asserted. The deletion gate refuses it.
#[cfg(feature = "shacl")]
#[test]
fn a_decision_that_only_removes_a_required_value_is_refused() {
    let mut store = scenario();
    store
        .load_shapes(
            "status-required",
            r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <urn:t:> .
ex:TicketStatusShape a sh:NodeShape ;
    sh:targetClass ex:Ticket ;
    sh:property [ sh:path ex:status ; sh:minCount 1 ] .
"#,
            TS,
        )
        .unwrap();
    let tx = store.latest_tx_id().unwrap();
    let (file, bytes) = decided(&store, Choice::Values(Vec::new()));
    let err = apply(
        &mut store, "triage", &file, &bytes, "steve", TS, None, false,
    )
    .unwrap_err();
    assert!(err.to_string().contains("SHACL"), "{err}");
    assert_eq!(store.latest_tx_id().unwrap(), tx);
    let (_, status, _) = ids(&store);
    assert_eq!(root_values(&store, status).len(), 1);
    assert_eq!(store.fork_lookup("triage").unwrap().unwrap().status, "open");
}
