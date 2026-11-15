//! Native record immutability must survive adapters which use raw SQL.
use super::*;
use crate::fluent::{
    Policy, Scalar,
    writer::{Append, read_log},
};
use crate::transaction_auth::{Identity, with_identity};

fn native_store() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    store
        .load_shapes(
            "quipu-fluents-v1",
            include_str!("../../shapes/fluents-v1.ttl"),
            "2026-11-08T00:00:00Z",
        )
        .unwrap();
    store.load_shapes("counter", &format!("<urn:counter-shape> <http://www.w3.org/ns/shacl#path> <urn:count>; <http://www.w3.org/ns/shacl#targetNode> <urn:subject>; <{}currentValuePolicy> \"sum\" .", crate::namespace::QUIPU_FLUENT), "2026-11-08T00:00:00Z").unwrap();
    let identity = Identity {
        principal: "urn:writer".into(),
        credential_id: Some("fixture".into()),
        auth_class: "named_bearer".into(),
        attestation: Default::default(),
    };
    let request = Append {
        fluent: "urn:fluent".into(),
        event: "urn:event".into(),
        subject: "urn:subject".into(),
        predicate: "urn:count".into(),
        property_shape: "urn:counter-shape".into(),
        as_of: "2026-11-08T00:00:00Z".into(),
        value: Scalar::Integer(3),
    };
    with_identity(Some(identity), || store.append_fluent_native(&request, 0)).unwrap();
    store
}

#[test]
fn raw_sql_cannot_close_delete_or_extend_native_observations() {
    let store = native_store();
    let event = store.lookup("urn:event").unwrap().unwrap();
    let before = read_log(&store, 0, "urn:fluent", Policy::Sum).unwrap();
    assert!(
        store
            .conn
            .execute(
                "UPDATE facts SET valid_to='2026-11-09T00:00:00Z' WHERE e=?1",
                [event]
            )
            .is_err(),
        "raw SQL closed immutable observation"
    );
    assert!(
        store
            .conn
            .execute("DELETE FROM facts WHERE e=?1", [event])
            .is_err()
    );
    let attribute = store.intern("urn:extra").unwrap();
    let tx = store.latest_tx_id().unwrap();
    assert!(store.conn.execute("INSERT INTO facts(e,a,v,g,tx,valid_from,op) VALUES(?1,?2,?3,0,?4,'2026-11-08T00:00:00Z',1)",params![event,attribute,Value::Int(9).to_bytes(),tx]).is_err());
    assert_eq!(
        read_log(&store, 0, "urn:fluent", Policy::Sum).unwrap(),
        before
    );
}

#[test]
fn fluent_type_codes_match_native_ref_codec_at_large_term_ids() {
    for id in [255_i64, 256, (1_i64 << 48) + 513, i64::MAX] {
        let store = Store::open_in_memory().unwrap();
        store
            .conn
            .execute(
                "INSERT INTO terms(id,iri) VALUES(?1,?2)",
                params![id, format!("{}Fluent", crate::namespace::QUIPU_FLUENT)],
            )
            .unwrap();
        let actual: Vec<u8> = store
            .conn
            .query_row(
                "SELECT value FROM fluent_type_codes WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(actual, Value::Ref(id).to_bytes(), "term {id}");
    }
}

#[test]
fn raw_replacement_and_cross_graph_moves_preserve_native_history() {
    let store = native_store();
    let event = store.lookup("urn:event").unwrap().unwrap();
    let before = read_log(&store, 0, "urn:fluent", Policy::Sum).unwrap();
    assert!(
        store
            .conn
            .execute("UPDATE facts SET g=99 WHERE e=?1", [event])
            .is_err()
    );
    assert!(store.conn.execute("INSERT OR REPLACE INTO facts(e,a,v,g,tx,valid_from,valid_to,op,retracted_tx) SELECT e,a,v,99,tx,valid_from,valid_to,op,retracted_tx FROM facts WHERE e=?1", [event]).is_err());
    assert!(
        store
            .conn
            .execute("UPDATE facts SET v=X'00' WHERE e=?1", [event])
            .is_err()
    );
    let permits: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM fluent_write_permits", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(permits, 0, "native append leaked committed permits");
    assert_eq!(
        read_log(&store, 0, "urn:fluent", Policy::Sum).unwrap(),
        before
    );
}

#[test]
fn fluent_triggers_survive_disk_copy_and_independent_connection() {
    let original = native_store();
    let event = original.lookup("urn:event").unwrap().unwrap();
    let before = read_log(&original, 0, "urn:fluent", Policy::Sum).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fluent.db");
    std::fs::write(&path, original.serialize_db().unwrap()).unwrap();
    let raw = rusqlite::Connection::open(&path).unwrap();
    assert!(
        raw.execute("DELETE FROM facts WHERE e=?1", [event])
            .is_err()
    );
    assert!(
        raw.execute("UPDATE facts SET retracted_tx=tx WHERE e=?1", [event])
            .is_err()
    );
    // Unrelated ordinary records remain writable on the same raw adapter.
    raw.execute("INSERT INTO terms(iri) VALUES('urn:ordinary')", [])
        .unwrap();
    let ordinary: i64 = raw
        .query_row("SELECT id FROM terms WHERE iri='urn:ordinary'", [], |r| {
            r.get(0)
        })
        .unwrap();
    raw.execute(
        "INSERT INTO facts(e,a,v,g,tx,valid_from,op) VALUES(?1,?1,?2,0,1,'2026-11-08T00:00:00Z',1)",
        params![ordinary, Value::Int(7).to_bytes()],
    )
    .unwrap();
    assert!(
        raw.execute("UPDATE facts SET e=?1 WHERE e=?2", params![event, ordinary])
            .is_err()
    );
    raw.execute("DELETE FROM facts WHERE e=?1", [ordinary])
        .unwrap();
    drop(raw);
    let reopened = Store::open(path.to_str().unwrap()).unwrap();
    assert_eq!(
        read_log(&reopened, 0, "urn:fluent", Policy::Sum).unwrap(),
        before
    );
    super::respace::classify_live_schema(&reopened.conn).unwrap();
}

#[test]
fn offline_import_cannot_extend_native_event_and_rolls_back_atomically() {
    let original = native_store();
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("destination.db");
    let source = dir.path().join("source.db");
    std::fs::write(&destination, original.serialize_db().unwrap()).unwrap();
    let mut foreign = Store::open(source.to_str().unwrap()).unwrap();
    let entity = foreign.intern("urn:event").unwrap();
    let attribute = foreign.intern("urn:foreign-extra").unwrap();
    foreign
        .transact(
            &[Datum {
                entity,
                attribute,
                value: Value::Int(9),
                valid_from: "2026-11-08T00:00:00Z".into(),
                valid_to: None,
                op: crate::Op::Assert,
            }],
            "2026-11-08T00:00:00Z",
            None,
            None,
        )
        .unwrap();
    drop(foreign);
    let before = read_log(&original, 0, "urn:fluent", Policy::Sum).unwrap();
    let source_bytes = std::fs::read(&source).unwrap();
    assert!(
        super::import::import_graph(&destination, &source, crate::schema::ROOT_GRAPH_IRI).is_err()
    );
    assert_eq!(std::fs::read(&source).unwrap(), source_bytes);
    let reopened = Store::open(destination.to_str().unwrap()).unwrap();
    assert_eq!(
        reopened.latest_tx_id().unwrap(),
        original.latest_tx_id().unwrap()
    );
    assert_eq!(
        reopened.lookup("urn:foreign-extra").unwrap(),
        None,
        "failed import leaked terms"
    );
    assert_eq!(
        read_log(&reopened, 0, "urn:fluent", Policy::Sum).unwrap(),
        before
    );
    drop(reopened);
    let raw = rusqlite::Connection::open(&source).unwrap();
    raw.execute(
        "UPDATE terms SET iri='urn:ordinary-foreign' WHERE iri='urn:event'",
        [],
    )
    .unwrap();
    drop(raw);
    let report =
        super::import::import_graph(&destination, &source, crate::schema::ROOT_GRAPH_IRI).unwrap();
    assert_eq!(report.facts, 1, "ordinary import positive control");
    let reopened = Store::open(destination.to_str().unwrap()).unwrap();
    assert!(reopened.lookup("urn:ordinary-foreign").unwrap().is_some());
    assert_eq!(
        read_log(&reopened, 0, "urn:fluent", Policy::Sum).unwrap(),
        before
    );
}

#[test]
fn native_domain_corrections_source_cut_and_restart_preserve_all_events() {
    let mut store = native_store();
    let identity = Identity {
        principal: "urn:writer".into(),
        credential_id: Some("fixture".into()),
        auth_class: "named_bearer".into(),
        attestation: Default::default(),
    };
    let mut request = Append {
        fluent: "urn:fluent".into(),
        event: "urn:event:future".into(),
        subject: "urn:subject".into(),
        predicate: "urn:count".into(),
        property_shape: "urn:counter-shape".into(),
        as_of: "2026-11-10T00:00:00Z".into(),
        value: Scalar::Integer(5),
    };
    with_identity(Some(identity), || {
        store.append_fluent_native(&request, 0).unwrap();
        request.event = "urn:event:late-correction".into();
        request.as_of = "2026-11-07T01:00:00+01:00".into();
        request.value = Scalar::Integer(-1);
        store.append_fluent_native(&request, 0).unwrap();
    });
    let log = read_log(&store, 0, "urn:fluent", Policy::Sum).unwrap();
    let source_cut = log.observations("urn:fluent")[1].order.clone();
    let at = "2026-11-08T00:00:00Z";
    let corrected = log
        .current_as_of("urn:fluent", Policy::Sum, at, None)
        .unwrap()
        .unwrap();
    assert_eq!(
        corrected.values,
        std::collections::BTreeSet::from([Scalar::Integer(2)])
    );
    assert_eq!(
        corrected.produced_by,
        std::collections::BTreeSet::from(["urn:event".into(), "urn:event:late-correction".into()])
    );
    let original = log
        .current_as_of("urn:fluent", Policy::Sum, at, Some(&source_cut))
        .unwrap()
        .unwrap();
    assert_eq!(
        original.values,
        std::collections::BTreeSet::from([Scalar::Integer(3)])
    );
    let intervals = log
        .domain_intervals("urn:fluent", Policy::Sum, None)
        .unwrap();
    assert_eq!(intervals.len(), 3);
    assert_eq!(intervals[0].from, "2026-11-07T00:00:00Z");
    assert_eq!(intervals[0].until.as_deref(), Some(at));
    assert_eq!(intervals[1].until.as_deref(), Some("2026-11-10T00:00:00Z"));
    assert_eq!(intervals[1].current, corrected);
    assert_eq!(
        intervals[2].current.values,
        std::collections::BTreeSet::from([Scalar::Integer(7)])
    );
    assert!(
        log.current_as_of("urn:fluent", Policy::Sum, "2026-11-08T00:00:00", None)
            .is_err()
    );
    let tx = store.latest_tx_id().unwrap();
    let facts = store.current_facts().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("domain.db");
    std::fs::write(&path, store.serialize_db().unwrap()).unwrap();
    drop(store);
    let reopened = Store::open(path.to_str().unwrap()).unwrap();
    let restored = read_log(&reopened, 0, "urn:fluent", Policy::Sum).unwrap();
    assert_eq!(restored, log);
    assert_eq!(
        restored
            .domain_intervals("urn:fluent", Policy::Sum, None)
            .unwrap(),
        intervals
    );
    assert_eq!(
        restored
            .current_as_of("urn:fluent", Policy::Sum, at, Some(&source_cut))
            .unwrap(),
        Some(original)
    );
    assert_eq!(reopened.latest_tx_id().unwrap(), tx);
    assert_eq!(reopened.current_facts().unwrap(), facts);
    assert_eq!(restored.observations("urn:fluent").len(), 3);
}
