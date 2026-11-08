use super::pages_tests::{canonical, corpus, reference};
use super::*;
const TS: &str = "2026-01-01T00:00:00Z";

fn derived(live: &Store) -> Snapshot {
    let mut snapshot = Snapshot::capture(live, Snapshot::temporary_store().unwrap(), TS).unwrap();
    snapshot.derive().unwrap();
    snapshot
}
fn publish(snapshot: &mut Snapshot, live: &mut Store) {
    while let Some(page) = snapshot.prepare_page().unwrap() {
        let committed = page.apply(live);
        assert!(snapshot.reconcile_page(committed).unwrap() <= APPLY_BATCH);
    }
}
#[test]
fn split_pages_match_cumulative_reference_across_boundaries() {
    let mut live = corpus(40);
    let mut snapshot = derived(&live);
    publish(&mut snapshot, &mut live);
    let prepared = snapshot.prepare_finish().unwrap();
    let committed = prepared.apply(&mut live);
    snapshot.reconcile_finish(committed).unwrap();
    assert_eq!(canonical(&live), reference(40));
    assert_eq!(pages::load(&snapshot.scratch).unwrap().phase, "COMPLETE");
}
#[test]
fn lost_page_acknowledgement_reopens_and_reconciles_the_same_receipt() {
    let mut live = corpus(40);
    let snapshot = derived(&live);
    let deadline = pages::load(&snapshot.scratch).unwrap().deadline;
    let committed = snapshot
        .prepare_page()
        .unwrap()
        .unwrap()
        .apply(&mut live)
        .unwrap();
    drop(committed); // crash after live commit, before private acknowledgement
    assert_eq!(pages::load(&snapshot.scratch).unwrap().apply_cursor, 0);
    let head = live.transaction_head().unwrap();
    let mut reopened = Snapshot::resume(snapshot.scratch).unwrap();
    let committed = reopened.prepare_page().unwrap().unwrap().apply(&mut live);
    assert_eq!(live.transaction_head().unwrap(), head);
    assert_eq!(reopened.reconcile_page(committed).unwrap(), 64);
    assert_eq!(pages::load(&reopened.scratch).unwrap().deadline, deadline);
}
#[test]
fn acknowledgement_failure_and_foreign_or_changed_cursor_refuse() {
    let mut live = corpus(40);
    let mut snapshot = derived(&live);
    let committed = snapshot.prepare_page().unwrap().unwrap().apply(&mut live);
    pages::fail_at("after_live_commit");
    assert!(snapshot.reconcile_page(committed).is_err());
    assert_eq!(pages::load(&snapshot.scratch).unwrap().apply_cursor, 0);
    let committed = snapshot.prepare_page().unwrap().unwrap().apply(&mut live);
    let mut other = derived(&live);
    assert!(other.reconcile_page(committed).is_err());
    let committed = snapshot.prepare_page().unwrap().unwrap().apply(&mut live);
    let mut job = pages::load(&snapshot.scratch).unwrap();
    job.apply_cursor = 1;
    pages::save(&snapshot.scratch, &job).unwrap();
    assert!(snapshot.reconcile_page(committed).is_err());
    assert_eq!(pages::load(&snapshot.scratch).unwrap().apply_cursor, 1);
    assert!(live.lookup(DERIVED_AS_OF_TX).unwrap().is_none());
}
#[test]
fn same_receipt_count_with_forged_range_or_digest_never_publishes_freshness() {
    for forge_range in [false, true] {
        let mut live = corpus(2);
        let mut snapshot = derived(&live);
        publish(&mut snapshot, &mut live);
        let (name, count): (String, i64) = live
            .conn
            .query_row(
                "SELECT min(batch),count(*) FROM owl_batch_receipts",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        if forge_range {
            live.conn
                .execute(
                    "UPDATE owl_batch_receipts SET batch=?1 WHERE batch=?2",
                    params![
                        format!(
                            "{}:1:999",
                            pages::load(&snapshot.scratch).unwrap().generation
                        ),
                        name
                    ],
                )
                .unwrap();
        } else {
            live.conn
                .execute(
                    "UPDATE owl_batch_receipts SET digest='forged' WHERE batch=?1",
                    [name],
                )
                .unwrap();
        }
        assert_eq!(
            live.conn
                .query_row("SELECT count(*) FROM owl_batch_receipts", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            count
        );
        let prepared = snapshot.prepare_finish().unwrap();
        assert!(
            snapshot
                .reconcile_finish(prepared.apply(&mut live))
                .is_err()
        );
        assert!(live.lookup(DERIVED_AS_OF_TX).unwrap().is_none());
    }
}
#[test]
fn prepared_page_revalidates_live_identity_and_ontology_before_publication() {
    for change_identity in [false, true] {
        let mut live = corpus(2);
        let mut snapshot = derived(&live);
        let prepared = snapshot.prepare_page().unwrap().unwrap();
        if change_identity {
            live = corpus(2);
        } else {
            live.load_ontology(
                "changed",
                "<urn:New> a <http://www.w3.org/2002/07/owl#Class> .",
                TS,
            )
            .unwrap();
        }
        let head = live.transaction_head().unwrap();
        assert!(snapshot.reconcile_page(prepared.apply(&mut live)).is_err());
        assert_eq!(live.transaction_head().unwrap(), head);
        assert_eq!(pages::load(&snapshot.scratch).unwrap().phase, "INVALIDATED");
    }
}
#[test]
fn scratch_read_failure_is_not_an_empty_prepared_page() {
    let live = corpus(2);
    let snapshot = derived(&live);
    snapshot
        .scratch
        .conn
        .execute("DROP TABLE owl_page_all", [])
        .unwrap();
    assert!(snapshot.prepare_page().is_err());
    assert!(snapshot.prepare_finish().is_err());
}

#[test]
fn prepared_page_revalidates_retraction_but_allows_monotone_additions() {
    for retract in [false, true] {
        let mut live = corpus(2);
        let mut snapshot = derived(&live);
        let prepared = snapshot.prepare_page().unwrap().unwrap();
        let data = if retract {
            let entity = live.lookup("urn:s0").unwrap().unwrap();
            let attribute = live.lookup("urn:p").unwrap().unwrap();
            let fact = live
                .current_facts()
                .unwrap()
                .into_iter()
                .find(|f| f.entity == entity && f.attribute == attribute)
                .unwrap();
            Datum {
                entity: fact.entity,
                attribute: fact.attribute,
                value: fact.value,
                valid_from: fact.valid_from,
                valid_to: fact.valid_to,
                op: Op::Retract,
            }
        } else {
            Datum {
                entity: live.intern("urn:later").unwrap(),
                attribute: live.intern("urn:added").unwrap(),
                value: Value::Int(1),
                valid_from: TS.into(),
                valid_to: None,
                op: Op::Assert,
            }
        };
        live.transact(&[data], TS, None, None).unwrap();
        let result = snapshot.reconcile_page(prepared.apply(&mut live));
        assert_eq!(result.is_err(), retract);
        assert_eq!(
            pages::load(&snapshot.scratch).unwrap().phase,
            if retract { "INVALIDATED" } else { "APPLYING" }
        );
    }
}

#[test]
fn prepared_finish_rechecks_retractions_before_freshness() {
    let mut live = corpus(2);
    let mut snapshot = derived(&live);
    publish(&mut snapshot, &mut live);
    let prepared = snapshot.prepare_finish().unwrap();
    let entity = live.lookup("urn:s0").unwrap().unwrap();
    let attribute = live.lookup("urn:p").unwrap().unwrap();
    let fact = live
        .current_facts()
        .unwrap()
        .into_iter()
        .find(|f| f.entity == entity && f.attribute == attribute)
        .unwrap();
    let datum = Datum {
        entity: fact.entity,
        attribute: fact.attribute,
        value: fact.value,
        valid_from: fact.valid_from,
        valid_to: fact.valid_to,
        op: Op::Retract,
    };
    live.transact(&[datum], TS, None, None).unwrap();
    assert!(
        snapshot
            .reconcile_finish(prepared.apply(&mut live))
            .is_err()
    );
    assert!(live.lookup(DERIVED_AS_OF_TX).unwrap().is_none());
}
