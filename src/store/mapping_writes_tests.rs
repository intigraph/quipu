//! Master-only lifecycle controls: real registry changes, graph writes and rollback.
use super::*;
use crate::shape_mapping::Correspondence;
use crate::shape_mapping::RegistryKind;

const OLD: &str = "<urn:fixture:Ticket> <http://www.w3.org/2000/01/rdf-schema#label> \"old\" .";
#[cfg(feature = "shacl")]
const NEW: &str = "<urn:fixture:Ticket> <http://www.w3.org/2000/01/rdf-schema#label> \"new\" .";
const ACTOR: &str = "urn:fixture:reviewer";
const AT: &str = "2026-10-25T00:00:00Z";
#[cfg(feature = "shacl")]
const NEXT: &str = "2026-10-26T00:00:00Z";

fn fixture() -> (Store, ShapeMapping) {
    let mut store = Store::open_in_memory().unwrap();
    for graph in [
        "urn:fixture:from",
        "urn:fixture:to",
        "urn:fixture:mappings",
        OPERATIONS,
    ] {
        store.graph_create(graph).unwrap();
    }
    store.load_shapes("fixture", OLD, AT).unwrap();
    let mapping = ShapeMapping {
        id: "urn:fixture:mapping:one".into(),
        graph: "urn:fixture:mappings".into(),
        maps_from: ["urn:fixture:from".into()].into(),
        maps_to: ["urn:fixture:to".into()].into(),
        pins: store
            .current_shape_pins(RegistryKind::Shapes, "fixture")
            .unwrap()
            .into_iter()
            .collect(),
        rows: [Correspondence {
            subject_id: "urn:fixture:Ticket".into(),
            object_id: "urn:fixture:Task".into(),
            predicate_id: "http://www.w3.org/2002/07/owl#equivalentClass".into(),
            mapping_justification: "semapv:ManualMappingCuration".into(),
            author_id: ACTOR.into(),
            quipu_review: "accepted".into(),
        }]
        .into(),
    };
    (store, mapping)
}
#[cfg(feature = "shacl")]
fn apply(store: &mut Store, mapping: &ShapeMapping) -> MappingReceipt {
    store
        .apply_shape_mapping(mapping.clone(), &mapping.digest().unwrap(), ACTOR, None)
        .unwrap()
}

#[cfg(feature = "shacl")]
#[test]
fn aggregate_inverse_changes_when_another_mapping_is_retired() {
    let (mut store, first) = fixture();
    assert!(apply(&mut store, &first).assert_writable["urn:fixture:Task"]);
    let mut second = first.clone();
    second.id = "urn:fixture:mapping:two".into();
    let mut row = second.rows.pop_first().unwrap();
    row.subject_id = "urn:fixture:Issue".into();
    second.rows.insert(row);
    assert!(!apply(&mut store, &second).assert_writable["urn:fixture:Task"]);
    assert!(
        !store
            .shape_mapping_receipt(&first.id)
            .unwrap()
            .unwrap()
            .assert_writable["urn:fixture:Task"]
    );
    let retired = store
        .retire_shape_mapping(&second.id, &second.digest().unwrap(), ACTOR)
        .unwrap();
    assert!(!retired.assert_writable["urn:fixture:Task"]);
    assert!(
        store
            .shape_mapping_receipt(&first.id)
            .unwrap()
            .unwrap()
            .assert_writable["urn:fixture:Task"]
    );
}

#[cfg(feature = "shacl")]
#[test]
fn a3_registry_flag_record_and_event_share_transaction_without_auto_repin() {
    let (mut store, mapping) = fixture();
    let applied = apply(&mut store, &mapping);
    store.load_shapes("fixture", NEW, NEXT).unwrap();
    let flagged = store.shape_mapping_receipt(&mapping.id).unwrap().unwrap();
    assert_eq!(flagged.status, "flagged");
    assert!(flagged.tx > applied.tx);
    assert!(!flagged.assert_writable["urn:fixture:Task"]);
    let event_tx: i64 = store.conn.query_row(
        "SELECT tx_id FROM events WHERE type='mapping.flagged' AND subject=?1 ORDER BY offset DESC LIMIT 1",
        [&mapping.id], |r| r.get(0)).unwrap();
    let registry_tx: i64 = store.conn.query_row(
        "SELECT tx_id FROM events WHERE type='shapes.loaded' AND subject='fixture' ORDER BY offset DESC LIMIT 1",
        [], |r| r.get(0)).unwrap();
    assert_eq!(event_tx, flagged.tx);
    assert_eq!(registry_tx, flagged.tx);
    let operations = store.mapping_graph(OPERATIONS).unwrap();
    let record = store
        .lookup(&flagged.record)
        .unwrap()
        .expect("record identity must exist");
    let record_facts = store.entity_facts_in_graph(record, operations).unwrap();
    assert!(!record_facts.is_empty());
    assert!(record_facts.iter().all(|fact| fact.tx == flagged.tx));
    let tx_attribute = store.lookup(&format!("{}tx", facts::NS)).unwrap().unwrap();
    assert!(
        record_facts
            .iter()
            .any(|fact| fact.attribute == tx_attribute
                && fact.value == crate::Value::Int(flagged.tx))
    );
    assert_eq!(
        store
            .mapping_state(&mapping.id)
            .unwrap()
            .unwrap()
            .mapping
            .pins,
        mapping.pins
    );
    assert!(
        store
            .apply_shape_mapping(mapping.clone(), &mapping.digest().unwrap(), ACTOR, None)
            .is_err()
    );
    let mut repinned = mapping.clone();
    repinned.pins = store
        .current_shape_pins(RegistryKind::Shapes, "fixture")
        .unwrap()
        .into_iter()
        .collect();
    assert_ne!(repinned.digest().unwrap(), mapping.digest().unwrap());
    assert_eq!(apply(&mut store, &repinned).status, "decided");
}

#[cfg(feature = "shacl")]
#[test]
fn late_flag_record_failure_rolls_back_registry_mapping_and_events() {
    let (mut store, mapping) = fixture();
    let applied = apply(&mut store, &mapping);
    let operations = store.mapping_graph(OPERATIONS).unwrap();
    let before_tx = store.latest_tx_id().unwrap();
    let before_events: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
        .unwrap();
    store.conn.execute_batch(&format!(
        "CREATE TRIGGER fixture_fail_record BEFORE INSERT ON facts WHEN NEW.g={operations} BEGIN SELECT RAISE(ABORT,'record failure'); END"
    )).unwrap();
    assert!(store.load_shapes("fixture", NEW, NEXT).is_err());
    assert_eq!(store.latest_tx_id().unwrap(), before_tx);
    assert_eq!(
        store
            .current_shape_pins(RegistryKind::Shapes, "fixture")
            .unwrap()
            .into_iter()
            .collect::<BTreeSet<_>>(),
        mapping.pins
    );
    let after = store.shape_mapping_receipt(&mapping.id).unwrap().unwrap();
    assert_eq!(after.status, "decided");
    assert_eq!(after.tx, applied.tx);
    assert_eq!(
        store
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        before_events
    );
    assert!(store.conn.is_autocommit());
}

#[cfg(feature = "shacl")]
#[test]
fn replay_returns_original_application_and_does_not_resurrect_retirement() {
    let (mut store, mapping) = fixture();
    let first = apply(&mut store, &mapping);
    assert!(apply(&mut store, &mapping).replayed);
    assert_eq!(store.latest_tx_id().unwrap(), first.tx);
    store
        .retire_shape_mapping(&mapping.id, &mapping.digest().unwrap(), ACTOR)
        .unwrap();
    let retired_tx = store.latest_tx_id().unwrap();
    let replay = apply(&mut store, &mapping);
    assert_eq!(replay.record, first.record);
    assert_eq!(replay.tx, first.tx);
    assert_eq!(replay.status, "retired");
    assert!(!replay.assert_writable["urn:fixture:Task"]);
    assert_eq!(store.latest_tx_id().unwrap(), retired_tx);
}

#[cfg(feature = "shacl")]
#[test]
fn loaded_mapping_profile_accepts_records_and_removal_flags_old_pin() {
    let (mut store, mapping) = fixture();
    store
        .load_shapes(
            "mapping-profile",
            include_str!("../../shapes/shape-mappings.ttl"),
            AT,
        )
        .unwrap();
    let applied = apply(&mut store, &mapping);
    assert_eq!(applied.status, "decided");
    store.remove_shapes("fixture").unwrap();
    let flagged = store.shape_mapping_receipt(&mapping.id).unwrap().unwrap();
    assert_eq!(flagged.status, "flagged");
    assert!(
        store
            .current_shape_pins(RegistryKind::Shapes, "fixture")
            .unwrap()
            .is_empty()
    );
    let payload: String = store.conn.query_row(
        "SELECT payload FROM events WHERE type='mapping.flagged' AND subject=?1 ORDER BY offset DESC LIMIT 1",
        [&mapping.id], |r| r.get(0)).unwrap();
    let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(payload["versions"], serde_json::json!([]));
    assert_eq!(
        payload["old_pin"]["digest"],
        mapping.pins.first().unwrap().digest
    );
    assert_eq!(payload["tx"], flagged.tx);
}

#[cfg(feature = "shacl")]
#[test]
fn readonly_preparation_and_strict_transport_cannot_write_or_forge_authority() {
    let (mut store, mapping) = fixture();
    let before = store.latest_tx_id().unwrap();
    let proposal =
        crate::mcp::tool_mapping_propose(&store, &serde_json::json!({"mapping":mapping})).unwrap();
    let row = mapping.rows.first().unwrap();
    let decision = serde_json::json!({
        "proposal":proposal,"reviewer":ACTOR,"decisions":[{
            "subject_id":row.subject_id,"predicate_id":row.predicate_id,
            "object_id":row.object_id,"accept":true
        }]
    });
    let decided = crate::mcp::tool_mapping_decide(&decision).unwrap();
    assert_eq!(store.latest_tx_id().unwrap(), before);
    let mut write = serde_json::json!({"action":"apply", "mapping":decided["mapping"],
        "expected_digest":decided["expected_digest"],"actor":ACTOR});
    write["onAuthority"] = serde_json::json!("urn:fixture:forged");
    assert!(crate::mcp::tool_mapping_apply(&mut store, &write).is_err());
    assert_eq!(store.latest_tx_id().unwrap(), before);
    write.as_object_mut().unwrap().remove("onAuthority");
    let applied = crate::mcp::tool_mapping_apply(&mut store, &write).unwrap();
    assert_eq!(applied["status"], "decided");
}

#[test]
fn proposal_survives_sqlite_readonly_while_the_writer_refuses() {
    let (mut store, mapping) = fixture();
    let before = store.latest_tx_id().unwrap();
    store.conn.execute_batch("PRAGMA query_only=ON").unwrap();
    // Anti-vacuity: this connection really refuses a new persistent term.
    assert!(store.intern("urn:fixture:readonly-negative").is_err());
    let proposal = store.propose_shape_mapping(mapping.clone()).unwrap();
    assert_eq!(proposal.expected_digest, mapping.digest().unwrap());
    assert_eq!(store.latest_tx_id().unwrap(), before);
    assert!(
        store
            .apply_shape_mapping(mapping.clone(), &mapping.digest().unwrap(), ACTOR, None)
            .is_err()
    );
    assert!(store.conn.is_autocommit());
    assert_eq!(store.latest_tx_id().unwrap(), before);
}

#[cfg(feature = "shacl")]
#[test]
fn retired_mapping_identity_remains_reserved_against_generic_writes() {
    let (mut store, mapping) = fixture();
    apply(&mut store, &mapping);
    store
        .retire_shape_mapping(&mapping.id, &mapping.digest().unwrap(), ACTOR)
        .unwrap();
    let before = store.latest_tx_id().unwrap();
    let graph = store.mapping_graph(&mapping.graph).unwrap();
    let timestamp = crate::time::now_iso();
    let datums = parse(
        &store,
        &format!("<{}> <urn:fixture:forged> \"value\" .", mapping.id),
        &timestamp,
    )
    .unwrap();
    // Arbitrary IDs are supported; this is not the reserved-child-URN shortcut.
    assert!(!mapping.id.starts_with("urn:quipu:mapping:"));
    assert!(
        store
            .transact_to_graph(
                &datums,
                &timestamp,
                Some(ACTOR),
                Some("unrelated-producer"),
                graph
            )
            .is_err()
    );
    assert_eq!(store.latest_tx_id().unwrap(), before);
    assert_eq!(
        store
            .shape_mapping_receipt(&mapping.id)
            .unwrap()
            .unwrap()
            .status,
        "retired"
    );
    let unrelated = parse(
        &store,
        "<urn:fixture:ordinary> <urn:fixture:property> \"value\" .",
        &timestamp,
    )
    .unwrap();
    assert!(
        store
            .transact_to_graph(
                &unrelated,
                &timestamp,
                Some(ACTOR),
                Some("unrelated-producer"),
                graph
            )
            .is_ok()
    );
}

#[cfg(not(feature = "shacl"))]
#[test]
fn preparation_without_shacl_cannot_admit_mapping_writes() {
    let (mut store, mapping) = fixture();
    let before = store.latest_tx_id().unwrap();
    let proposal = store.propose_shape_mapping(mapping.clone()).unwrap();
    assert_eq!(proposal.expected_digest, mapping.digest().unwrap());
    let error = store
        .apply_shape_mapping(mapping.clone(), &mapping.digest().unwrap(), ACTOR, None)
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("write SHACL requires the shacl feature")
    );
    assert!(store.conn.is_autocommit());
    assert_eq!(store.latest_tx_id().unwrap(), before);
    assert!(store.shape_mapping_receipt(&mapping.id).unwrap().is_none());
}

/// A schema proposal accepted with a migration loads its shapes inside the
/// migration's savepoint, where the flagging settlement cannot open its own
/// transaction. Closing a decided pin there must be refused with nothing
/// written, never land without its `MappingRecord`; the same proposal without
/// a migration still flags through the atomic settlement.
#[cfg(feature = "shacl")]
#[test]
fn enclosed_migration_cannot_close_a_decided_pin_without_its_settlement() {
    use crate::proposal::{NewProposal, ProposalKind, ProposalStatus};
    use crate::proposal_migration::MigrationOp;

    let (mut store, mapping) = fixture();
    apply(&mut store, &mapping);
    let propose = |store: &Store, target: &str| {
        store
            .insert_proposal(&NewProposal {
                kind: &ProposalKind::Shape,
                target,
                diff: NEW,
                rationale: None,
                proposer: "agent/test",
                trigger_ref: None,
                created_at: AT,
            })
            .unwrap()
    };
    let program = [MigrationOp::RenamePredicate {
        from: "urn:fixture:unusedFrom".into(),
        to: "urn:fixture:unusedTo".into(),
    }];
    let events = |store: &Store| -> i64 {
        store
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
            .unwrap()
    };
    let pinned = store
        .current_shape_pins(RegistryKind::Shapes, "fixture")
        .unwrap();

    let id = propose(&store, "fixture");
    store.set_proposal_migration(id, &program).unwrap();
    let before = events(&store);
    let dry = crate::proposal_migration::dry_run(&store, id, 10).unwrap_err();
    assert!(dry.to_string().contains("own mapping settlement"), "{dry}");
    let refused = store
        .accept_proposal_with_migration(id, ACTOR, NEXT, None)
        .unwrap_err();
    assert!(
        refused.to_string().contains("own mapping settlement"),
        "{refused}"
    );
    assert_eq!(
        store.get_proposal(id).unwrap().unwrap().status,
        ProposalStatus::Pending
    );
    assert_eq!(
        store
            .current_shape_pins(RegistryKind::Shapes, "fixture")
            .unwrap(),
        pinned
    );
    assert_eq!(
        store
            .shape_mapping_receipt(&mapping.id)
            .unwrap()
            .unwrap()
            .status,
        "decided"
    );
    assert_eq!(events(&store), before, "a refused acceptance emits nothing");
    assert!(store.conn.is_autocommit());

    // An unpinned shape set still loads inside the migration savepoint.
    let other = propose(&store, "unpinned");
    store.set_proposal_migration(other, &program).unwrap();
    crate::proposal_migration::dry_run(&store, other, 10).unwrap();
    let (accepted, _) = store
        .accept_proposal_with_migration(other, ACTOR, NEXT, None)
        .unwrap();
    assert_eq!(accepted.status, ProposalStatus::Accepted);
    assert!(
        !store
            .current_shape_pins(RegistryKind::Shapes, "unpinned")
            .unwrap()
            .is_empty()
    );

    // Without a migration the pinned load settles atomically and flags.
    let plain = propose(&store, "fixture");
    store.accept_proposal(plain, ACTOR, NEXT, None).unwrap();
    assert_eq!(
        store
            .shape_mapping_receipt(&mapping.id)
            .unwrap()
            .unwrap()
            .status,
        "flagged"
    );
}
