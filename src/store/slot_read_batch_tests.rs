use super::*;
use crate::Value;
use crate::resolver_contract::{ContentDigest, SourceBinding};
use crate::write_base::{LegacyBasePolicy, WriterSnapshot, prepare_slot_reads};
use rusqlite::params;

const T0: &str = "2026-01-01T00:00:00Z";
const TZ: &str = "2026-01-03T00:00:00Z";
fn limits() -> BatchReadLimits {
    BatchReadLimits {
        claims_per_read: 8,
        bytes_per_read: 4096,
        total_claims: 16,
        total_bytes: 8192,
    }
}
fn fixture() -> (Store, WriterSnapshot, SourceBinding, Vec<Slot>) {
    let store = Store::open_in_memory().unwrap();
    let e = store.intern("urn:batch:item").unwrap();
    let a = store.intern("urn:batch:value").unwrap();
    let base_tx = store.latest_tx_id().unwrap() + 1;
    let current_tx = base_tx + 2;
    for tx in base_tx..=current_tx {
        store
            .conn
            .execute(
                "INSERT INTO transactions(id,timestamp) VALUES(?1,?2)",
                params![tx, T0],
            )
            .unwrap();
    }
    // One later closure and its replacement; both physical rows exist at read time.
    for (value, tx, start, end, closed) in [
        ("old", base_tx, T0, Some(TZ), Some(current_tx)),
        ("new", current_tx, TZ, None, None),
    ] {
        store.conn.execute("INSERT INTO facts(g,e,a,v,tx,valid_from,valid_to,op,retracted_tx) VALUES(0,?1,?2,?3,?4,?5,?6,1,?7)",
            params![e,a,Value::Str(value.into()).to_bytes(),tx,start,end,closed]).unwrap();
    }
    let slots = vec![Slot {
        subject: "<urn:batch:item>".into(),
        predicate: "urn:batch:value".into(),
    }];
    let snapshot = WriterSnapshot {
        store_id: store.store_id().unwrap(),
        log_epoch: store.log_epoch().unwrap(),
        graph: crate::schema::ROOT_GRAPH_IRI.into(),
        oldest_complete_tx: 0,
        head_tx: u64::try_from(current_tx).unwrap(),
    };
    let source = SourceBinding {
        store_id: snapshot.store_id.clone(),
        log_epoch: snapshot.log_epoch.clone(),
        graph: snapshot.graph.clone(),
        base_tx: u64::try_from(base_tx).unwrap(),
        base_slice_hash: ContentDigest([0; 32]),
        scope: Some(slots.clone()),
        inputs: vec![],
    };
    (store, snapshot, source, slots)
}

#[test]
fn prepared_batch_binds_both_cuts_and_charges_physical_work_before_visibility() {
    let (store, snapshot, source, slots) = fixture();
    let prepared = prepare_slot_reads(
        Some(&source),
        &snapshot,
        LegacyBasePolicy::RequireDeclared,
        &slots,
        2,
        TZ,
    )
    .unwrap();
    let out = store.read_prepared_slot_batch(&prepared, limits()).unwrap();
    assert_eq!(out.slots.len(), 1);
    assert_eq!(out.slots[0].slot.subject, slots[0].subject);
    assert!(matches!(&out.slots[0].base[0].value,Value::Str(v) if v=="old"));
    assert!(matches!(&out.slots[0].current[0].value,Value::Str(v) if v=="new"));
    assert_eq!(
        out.physical_claims, 4,
        "both cuts scan both rows, including invisible rows"
    );
    assert!(out.encoded_bytes > 0);
    let mut low = limits();
    low.total_claims = 3;
    assert!(store.read_prepared_slot_batch(&prepared, low).is_err());
    let mut low = limits();
    low.total_bytes = out.encoded_bytes - 1;
    assert!(store.read_prepared_slot_batch(&prepared, low).is_err());
    let mut empty = source.clone();
    empty.scope = Some(vec![]);
    let prepared = prepare_slot_reads(
        Some(&empty),
        &snapshot,
        LegacyBasePolicy::RequireDeclared,
        &[],
        2,
        TZ,
    )
    .unwrap();
    let out = store.read_prepared_slot_batch(&prepared, limits()).unwrap();
    assert!(out.slots.is_empty());
    assert_eq!((out.physical_claims, out.encoded_bytes), (0, 0));
}

#[test]
fn prepared_batch_refuses_foreign_coordinates_and_moved_head_or_epoch() {
    let (store, snapshot, source, slots) = fixture();
    for field in ["store", "graph"] {
        let mut wrong = snapshot.clone();
        let mut declaration = source.clone();
        if field == "store" {
            wrong.store_id = "other".into();
            declaration.store_id = "other".into();
        } else {
            wrong.graph = "urn:missing:graph".into();
            declaration.graph = wrong.graph.clone();
        }
        let prepared = prepare_slot_reads(
            Some(&declaration),
            &wrong,
            LegacyBasePolicy::RequireDeclared,
            &slots,
            2,
            TZ,
        )
        .unwrap();
        assert!(store.read_prepared_slot_batch(&prepared, limits()).is_err());
    }
    let prepared = prepare_slot_reads(
        Some(&source),
        &snapshot,
        LegacyBasePolicy::RequireDeclared,
        &slots,
        2,
        TZ,
    )
    .unwrap();
    let moved = i64::try_from(snapshot.head_tx).unwrap() + 1;
    store
        .conn
        .execute(
            "INSERT INTO transactions(id,timestamp) VALUES(?1,?2)",
            params![moved, TZ],
        )
        .unwrap();
    assert!(store.read_prepared_slot_batch(&prepared, limits()).is_err());
    store
        .conn
        .execute("DELETE FROM transactions WHERE id=?1", params![moved])
        .unwrap();
    store
        .mint_log_epoch("test changed history identity")
        .unwrap();
    assert!(store.read_prepared_slot_batch(&prepared, limits()).is_err());
}
