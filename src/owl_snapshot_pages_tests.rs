use super::*;
use std::collections::BTreeSet;
const TS: &str = "2026-01-01T00:00:00Z";

pub(super) fn corpus(count: usize) -> Store {
    let mut store = Store::open_in_memory().unwrap();
    store.load_ontology("properties", "@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> . <urn:p> rdfs:subPropertyOf <urn:q> . <urn:q> rdfs:range <urn:A> .", TS).unwrap();
    store.load_ontology("classes", "@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> . <urn:A> rdfs:subClassOf <urn:B> .", TS).unwrap();
    let attr = store.intern("urn:p").unwrap();
    let data: Vec<_> = (0..count)
        .map(|i| Datum {
            entity: store.intern(&format!("urn:s{i}")).unwrap(),
            attribute: attr,
            value: Value::Ref(store.intern(&format!("urn:o{i}")).unwrap()),
            valid_from: TS.into(),
            valid_to: None,
            op: Op::Assert,
        })
        .collect();
    store.transact(&data, TS, None, None).unwrap();
    store
}

pub(super) fn canonical(store: &Store) -> BTreeSet<(String, String, String)> {
    let Some(graph) = store.lookup(ROOT_INFERRED_GRAPH_IRI).unwrap() else {
        return BTreeSet::new();
    };
    store
        .without_plane_metadata(store.current_facts_in_graph(graph).unwrap())
        .unwrap()
        .into_iter()
        .map(|f| {
            let value = match f.value {
                Value::Ref(id) => format!("ref:{}", store.resolve(id).unwrap()),
                v => format!("{v:?}"),
            };
            (
                store.resolve(f.entity).unwrap(),
                store.resolve(f.attribute).unwrap(),
                value,
            )
        })
        .collect()
}

pub(super) fn reference(count: usize) -> BTreeSet<(String, String, String)> {
    let mut store = corpus(count);
    store.ensure_owl_cache().unwrap();
    let ontology = store.owl_cache.as_deref().unwrap().clone();
    assert!(
        !ontology
            .materialize(&mut store, TS)
            .unwrap()
            .pass_budget_exhausted
    );
    canonical(&store)
}

#[test]
fn all_pages_and_all_passes_match_small_uncapped_same_union() {
    for size in [1, 2, 200_000] {
        let mut live = corpus(3);
        let mut snapshot =
            Snapshot::capture(&live, Snapshot::temporary_store().unwrap(), TS).unwrap();
        pages::derive(&mut snapshot, size).unwrap();
        assert!(snapshot.report.passes >= 3);
        while snapshot.remaining() > 0 {
            snapshot.apply_batch(&mut live).unwrap();
        }
        snapshot.finish(&mut live).unwrap();
        assert_eq!(canonical(&live), reference(3));
    }
}

#[test]
fn crashes_preserve_frozen_pass_and_atomic_cursor() {
    for point in [
        "before_enumeration_commit",
        "after_enumeration_commit",
        "before_promotion_commit",
        "mid_promotion",
        "after_promotion_commit",
    ] {
        let mut live = corpus(3);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scratch.db");
        let mut snapshot =
            Snapshot::capture(&live, Store::open(path.to_str().unwrap()).unwrap(), TS).unwrap();
        let deadline = pages::load(&snapshot.scratch).unwrap().deadline;
        pages::fail_at(point);
        assert!(
            pages::derive(&mut snapshot, 1)
                .unwrap_err()
                .to_string()
                .contains("simulated crash")
        );
        assert_eq!(canonical(&live), BTreeSet::new());
        drop(snapshot);
        let mut snapshot = Snapshot::resume(Store::open(path.to_str().unwrap()).unwrap()).unwrap();
        assert_eq!(pages::load(&snapshot.scratch).unwrap().deadline, deadline);
        if !snapshot.derived {
            pages::derive(&mut snapshot, 1).unwrap();
        }
        while snapshot.remaining() > 0 {
            snapshot.apply_batch(&mut live).unwrap();
        }
        snapshot.finish(&mut live).unwrap();
        assert_eq!(canonical(&live), reference(3), "{point}");
    }
}

#[test]
fn lost_live_ack_reconciles_same_receipt_before_cursor_advance() {
    let mut live = corpus(30);
    let mut snapshot = Snapshot::capture(&live, Snapshot::temporary_store().unwrap(), TS).unwrap();
    snapshot.derive().unwrap();
    let remaining = snapshot.remaining();
    pages::fail_at("after_live_commit");
    assert!(snapshot.apply_batch(&mut live).is_err());
    let head = live.transaction_head().unwrap();
    assert_eq!(snapshot.remaining(), remaining);
    assert_eq!(snapshot.apply_batch(&mut live).unwrap(), APPLY_BATCH);
    assert_eq!(
        live.transaction_head().unwrap(),
        head,
        "no second live commit after lost ack"
    );
    while snapshot.remaining() > 0 {
        snapshot.apply_batch(&mut live).unwrap();
    }
    snapshot.finish(&mut live).unwrap();
    assert_eq!(canonical(&live), reference(30));
}

#[test]
fn durable_deadline_and_logical_pass_caps_never_reset_on_resume() {
    for (pass, deadline) in [(64, i64::MAX), (0, 0)] {
        let live = corpus(1);
        let mut snapshot =
            Snapshot::capture(&live, Snapshot::temporary_store().unwrap(), TS).unwrap();
        let mut job = pages::load(&snapshot.scratch).unwrap();
        job.pass = pass;
        job.deadline = deadline;
        pages::save(&snapshot.scratch, &job).unwrap();
        snapshot = Snapshot::resume(snapshot.scratch).unwrap();
        assert!(snapshot.derive().is_err());
        assert!(snapshot.finish(&mut corpus(1)).is_err());
    }
}

#[test]
fn promotion_exact_200000_boundary_preserves_200001_novel_keys() {
    let live = corpus(0);
    let mut snapshot = Snapshot::capture(&live, Snapshot::temporary_store().unwrap(), TS).unwrap();
    let entity = snapshot.scratch.intern("urn:boundary").unwrap();
    let attr = snapshot.scratch.intern("urn:boundary-predicate").unwrap();
    {
        let tx = snapshot.scratch.conn.unchecked_transaction().unwrap();
        let mut insert = snapshot
            .scratch
            .conn
            .prepare("INSERT INTO owl_page_queue(e,a,v) VALUES(?1,?2,?3)")
            .unwrap();
        for i in 0..200001 {
            insert
                .execute(params![entity, attr, Value::Int(i).to_bytes()])
                .unwrap();
        }
        drop(insert);
        tx.commit().unwrap();
    }
    let mut job = pages::load(&snapshot.scratch).unwrap();
    job.phase = "PASS_COMPLETE".into();
    job.pass = 1;
    job.report.total = 200001;
    job.queue_count = 200001;
    pages::save(&snapshot.scratch, &job).unwrap();
    pages::fail_at("after_promotion_commit");
    assert!(pages::derive(&mut snapshot, 200000).is_err());
    let job = pages::load(&snapshot.scratch).unwrap();
    assert_eq!(job.cursor, 200000);
    let promoted: i64 = snapshot
        .scratch
        .conn
        .query_row("SELECT count(*) FROM owl_page_all", [], |r| r.get(0))
        .unwrap();
    assert_eq!(promoted, 200000);
    snapshot = Snapshot::resume(snapshot.scratch).unwrap();
    snapshot.derive().unwrap();
    assert_eq!(snapshot.remaining(), 200001);
    let keys = pages::read_page(&snapshot.scratch, "owl_page_all", 200000, 200000, TS).unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].1.value, Value::Int(200000));
    assert_eq!(
        pages::load(&snapshot.scratch).unwrap().phase,
        "FIXPOINT_COMPLETE"
    );
    assert_eq!(snapshot.report.total, 200001);
}

#[test]
fn final_freshness_unknown_reconciles_without_duplicate_commit() {
    let mut live = corpus(2);
    let mut snapshot = Snapshot::capture(&live, Snapshot::temporary_store().unwrap(), TS).unwrap();
    snapshot.derive().unwrap();
    while snapshot.remaining() > 0 {
        snapshot.apply_batch(&mut live).unwrap();
    }
    pages::fail_at("after_freshness_commit");
    assert!(snapshot.finish(&mut live).is_err());
    let head = live.transaction_head().unwrap();
    snapshot.finish(&mut live).unwrap();
    assert_eq!(live.transaction_head().unwrap(), head);
    assert_eq!(pages::load(&snapshot.scratch).unwrap().phase, "COMPLETE");
}

#[test]
fn receipt_conflict_refuses_and_atomic_receipt_rolls_back_with_assertions() {
    let mut live = corpus(1);
    let graph = live.intern(ROOT_INFERRED_GRAPH_IRI).unwrap();
    let datum = Datum {
        entity: live.intern("urn:receipt-test").unwrap(),
        attribute: live.intern("urn:attr").unwrap(),
        value: Value::Int(1),
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    };
    live.transact_owl_page(
        std::slice::from_ref(&datum),
        TS,
        graph,
        ("batch", "digest"),
        "owl:materialize",
    )
    .unwrap();
    assert!(
        live.transact_owl_page(
            &[datum],
            TS,
            graph,
            ("batch", "conflict"),
            "owl:materialize"
        )
        .is_err()
    );
    let head = live.transaction_head().unwrap();
    // A commit-hook refusal exercises the actual outer SQLite commit boundary.
    live.conn.commit_hook(Some(|| true)).unwrap();
    assert!(
        live.transact_owl_page(&[], TS, graph, ("refused", "digest"), "owl:materialize")
            .is_err()
    );
    live.conn.commit_hook(None::<fn() -> bool>).unwrap();
    assert_eq!(live.transaction_head().unwrap(), head);
    let n: i64 = live
        .conn
        .query_row(
            "SELECT count(*) FROM owl_batch_receipts WHERE batch='refused'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn named_job_resumes_across_handle_close_without_new_generation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.db");
    std::fs::write(&path, corpus(3).serialize_db().unwrap()).unwrap();
    let mut live = Store::open(path.to_str().unwrap()).unwrap();
    let mut snapshot = Snapshot::capture_or_resume(&live, TS).unwrap();
    let generation = pages::load(&snapshot.scratch).unwrap().generation;
    pages::fail_at("after_promotion_commit");
    assert!(pages::derive(&mut snapshot, 1).is_err());
    drop(snapshot);
    let mut snapshot = Snapshot::capture_or_resume(&live, TS).unwrap();
    assert_eq!(
        pages::load(&snapshot.scratch).unwrap().generation,
        generation
    );
    snapshot.derive_or_resume().unwrap();
    let mut worst = std::time::Duration::ZERO;
    while snapshot.remaining() > 0 {
        let start = std::time::Instant::now();
        assert!(snapshot.apply_batch(&mut live).unwrap() <= APPLY_BATCH);
        worst = worst.max(start.elapsed());
    }
    snapshot.finish(&mut live).unwrap();
    assert_eq!(canonical(&live), reference(3));
    eprintln!(
        "small native writer batch max_us={} (includes validation/remapping/commit)",
        worst.as_micros()
    );
    assert!(
        worst < std::time::Duration::from_secs(2),
        "small-fixture writer bound"
    );
}

fn every_family(mut store: Store) -> Store {
    store
        .load_ontology(
            "additional",
            r"@prefix owl: <http://www.w3.org/2002/07/owl#> .
        <urn:q> owl:inverseOf <urn:inverse> ; owl:equivalentProperty <urn:equivalent> .
        <urn:symmetric> a owl:SymmetricProperty .
        <urn:transitive> a owl:TransitiveProperty .
        <urn:A> owl:equivalentClass <urn:Alias> .",
            TS,
        )
        .unwrap();
    // load_ontology is the low-level &self registry writer; normal /ontology
    // invalidates this cache explicitly. Match that contract in the reference.
    store.invalidate_owl_cache();
    for (s, p, o) in [
        (
            "urn:s0",
            "http://www.w3.org/2002/07/owl#sameAs",
            "urn:s-alias",
        ),
        ("urn:x", "urn:symmetric", "urn:y"),
        ("urn:x", "urn:transitive", "urn:y"),
        ("urn:y", "urn:transitive", "urn:z"),
    ] {
        let datum = Datum {
            entity: store.intern(s).unwrap(),
            attribute: store.intern(p).unwrap(),
            value: Value::Ref(store.intern(o).unwrap()),
            valid_from: TS.into(),
            valid_to: None,
            op: Op::Assert,
        };
        store.transact(&[datum], TS, None, None).unwrap();
    }
    store
}

#[test]
fn unique_disk_sink_matches_reference_across_all_rule_families() {
    let mut reference = every_family(corpus(3));
    reference.ensure_owl_cache().unwrap();
    let ontology = reference.owl_cache.as_deref().unwrap().clone();
    let report = ontology.materialize(&mut reference, TS).unwrap();
    assert!(!report.pass_budget_exhausted);
    for size in [1, 2, 200000] {
        let mut live = every_family(corpus(3));
        let mut snapshot =
            Snapshot::capture(&live, Snapshot::temporary_store().unwrap(), TS).unwrap();
        pages::derive(&mut snapshot, size).unwrap();
        let r = &snapshot.report;
        assert!(
            r.subclass_inferences > 0
                && r.sub_property_inferences > 0
                && r.domain_range_inferences > 0
        );
        assert!(
            r.inverse_inferences > 0
                && r.equivalent_property_inferences > 0
                && r.equivalent_class_inferences > 0
        );
        assert!(
            r.same_as_inferences > 0 && r.symmetric_inferences > 0 && r.transitive_inferences > 0
        );
        assert_eq!(
            r.raw_proposals,
            r.known_proposals + r.repeated_proposals + r.total
        );
        assert!(r.repeated_proposals > 0);
        while snapshot.remaining() > 0 {
            snapshot.apply_batch(&mut live).unwrap();
        }
        snapshot.finish(&mut live).unwrap();
        assert_eq!(canonical(&live), canonical(&reference));
    }
}

#[test]
fn scratch_disk_quota_refuses_without_live_assertions_or_freshness() {
    let live = corpus(1);
    let mut snapshot = Snapshot::capture(&live, Snapshot::temporary_store().unwrap(), TS).unwrap();
    let pages: i64 = snapshot
        .scratch
        .conn
        .query_row("PRAGMA page_count", [], |r| r.get(0))
        .unwrap();
    snapshot
        .scratch
        .conn
        .execute_batch(&format!("PRAGMA max_page_count={pages}"))
        .unwrap();
    let before = live.transaction_head().unwrap();
    // Fill the durable queue with a single oversized candidate under the actual
    // SQLite quota, rather than substituting an allocation-error fake.
    let value = Value::Str("x".repeat(1_000_000));
    let result = snapshot.scratch.conn.execute(
        "INSERT INTO owl_page_queue(e,a,v) VALUES(1,1,?1)",
        [value.to_bytes()],
    );
    assert!(result.is_err());
    assert!(!snapshot.derived);
    assert_eq!(snapshot.remaining(), 0);
    assert!(snapshot.finish(&mut corpus(1)).is_err());
    assert_eq!(live.transaction_head().unwrap(), before);
    // A normal admitted restart may reapply the fixed ceiling, never a larger one.
    snapshot = Snapshot::resume(snapshot.scratch).unwrap();
    snapshot.derive().unwrap();
}

#[test]
fn demo_total_budget_to_bounded_pages() {
    let mut old = corpus(2);
    old.ensure_owl_cache().unwrap();
    let ontology = old.owl_cache.as_deref().unwrap().clone();
    let error = ontology.materialize_limited(&mut old, TS, 1).unwrap_err();
    println!("fixture: two property facts; same property/range/subclass ontology union");
    println!("unchanged baseline total-limit=1: REFUSED ({error})");
    let mut live = corpus(2);
    let mut snapshot = Snapshot::capture(&live, Snapshot::temporary_store().unwrap(), TS).unwrap();
    pages::derive(&mut snapshot, 1).unwrap();
    println!(
        "candidate page-limit=1: total={} logical_passes={}",
        snapshot.report.total, snapshot.report.passes
    );
    while snapshot.remaining() > 0 {
        snapshot.apply_batch(&mut live).unwrap();
    }
    snapshot.finish(&mut live).unwrap();
    let equal = canonical(&live) == reference(2);
    println!(
        "complete={} canonical_full_set_matches_reference={equal}",
        pages::load(&snapshot.scratch).unwrap().phase == "COMPLETE"
    );
    assert!(equal);
}

#[test]
fn missing_live_receipt_or_corrupt_cursor_never_publishes_freshness() {
    for corrupt_cursor in [false, true] {
        let mut live = corpus(2);
        let mut snapshot =
            Snapshot::capture(&live, Snapshot::temporary_store().unwrap(), TS).unwrap();
        snapshot.derive().unwrap();
        while snapshot.remaining() > 0 {
            snapshot.apply_batch(&mut live).unwrap();
        }
        if corrupt_cursor {
            let mut job = pages::load(&snapshot.scratch).unwrap();
            job.apply_cursor += 1;
            pages::save(&snapshot.scratch, &job).unwrap();
        } else {
            live.conn
                .execute("DELETE FROM owl_batch_receipts", [])
                .unwrap();
        }
        assert_eq!(snapshot.remaining(), 0);
        assert!(
            snapshot
                .finish(&mut live)
                .unwrap_err()
                .to_string()
                .contains("do not cover")
        );
        let fresh = live.lookup(DERIVED_AS_OF_TX).unwrap();
        if let Some(attr) = fresh {
            let graph = live.lookup(ROOT_INFERRED_GRAPH_IRI).unwrap().unwrap();
            assert!(
                !live
                    .current_facts_in_graph(graph)
                    .unwrap()
                    .iter()
                    .any(|f| f.attribute == attr)
            );
        }
    }
}

#[test]
fn retracting_a_newly_created_companion_assertion_invalidates_publication() {
    let mut live = corpus(2);
    let mut snapshot = Snapshot::capture(&live, Snapshot::temporary_store().unwrap(), TS).unwrap();
    assert!(snapshot.source_companion.is_none());
    snapshot.derive().unwrap();
    while snapshot.remaining() > 0 {
        snapshot.apply_batch(&mut live).unwrap();
    }
    let graph = live.lookup(ROOT_INFERRED_GRAPH_IRI).unwrap().unwrap();
    let fact = live
        .without_plane_metadata(live.current_facts_in_graph(graph).unwrap())
        .unwrap()
        .remove(0);
    live.transact_to_graph(
        &[Datum {
            entity: fact.entity,
            attribute: fact.attribute,
            value: fact.value,
            valid_from: TS.into(),
            valid_to: None,
            op: Op::Retract,
        }],
        TS,
        Some("owl"),
        Some("owl:materialize"),
        graph,
    )
    .unwrap();
    assert!(
        snapshot
            .finish(&mut live)
            .unwrap_err()
            .to_string()
            .contains("retracted")
    );
    assert_eq!(pages::load(&snapshot.scratch).unwrap().phase, "INVALIDATED");
}
