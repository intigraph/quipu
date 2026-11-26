//! M2 exit proof: a fresh identity, members copied at the cut, a byte-copied
//! home file refused, local writes under the pinned gate, and X6/X8 each
//! refusing with nothing written.
use super::super::base_digest;
use super::super::tests::{DATA, FROM, Home, LEG, PROV, home};
use super::*;
use crate::{
    store::Datum,
    types::{Op, Value},
};
use std::path::PathBuf;

const SHAPES: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
<urn:value-shape> a sh:NodeShape ; sh:targetSubjectsOf <urn:value> ;
  sh:property [ sh:path <urn:value> ; sh:datatype xsd:string ] .";
const LATER: &str = "2026-02-01T00:00:00Z";

fn spec(members: &[(&str, MemberRole)]) -> MaterializeSpec {
    MaterializeSpec {
        name: "agent-a".into(),
        members: members
            .iter()
            .map(|(iri, role)| super::super::MiniForkMember {
                iri: (*iri).into(),
                role: *role,
            })
            .collect(),
        created_at: FROM.into(),
    }
}

use super::super::MemberRole::{self, Reference, Writable};

fn datum(store: &Store, entity: &str, value: Value) -> Datum {
    Datum {
        entity: store.intern(entity).unwrap(),
        attribute: store.intern("urn:value").unwrap(),
        value,
        valid_from: LATER.into(),
        valid_to: None,
        op: Op::Assert,
    }
}

fn legacy_write(home: &mut Home, graph: &str, value: &str) {
    let g = if graph == ROOT_GRAPH_IRI {
        0
    } else {
        home.legacy.committed_graph_id(graph).unwrap().unwrap()
    };
    let d = datum(&home.legacy, "urn:item", Value::Str(value.into()));
    home.legacy
        .ownership_operation(|s| s.transact_graph_batches_tx(&[(g, vec![d])], LATER, None, None))
        .unwrap();
}

/// (fact rows, transactions) — the "nothing written" measure.
fn footprint(store: &Store) -> (i64, i64) {
    let count = |sql: &str| store.conn.query_row(sql, [], |r| r.get(0)).unwrap();
    (
        count("SELECT count(*) FROM facts"),
        count("SELECT count(*) FROM transactions"),
    )
}

fn values(store: &Store, graph: &str) -> Vec<(String, String)> {
    let journal = member_journal(store, store.latest_tx_id().unwrap(), graph).unwrap();
    journal
        .rows()
        .iter()
        .map(|r| (r.triple.clone(), r.valid_from.clone()))
        .collect()
}

fn pinned_legacy(dir: &std::path::Path) -> (Home, MiniQuipu, PathBuf) {
    let mut home = home();
    legacy_write(&mut home, ROOT_GRAPH_IRI, "reference-at-cut");
    home.legacy
        .ownership_operation(|s| s.load_shapes("home-shapes", SHAPES, FROM))
        .unwrap();
    let path = dir.join("mini.db");
    let mq = materialize_on_legacy(
        &home.catalog,
        &mut home.legacy,
        &spec(&[(LEG, Writable), (ROOT_GRAPH_IRI, Reference)]),
        &path,
    )
    .unwrap();
    (home, mq, path)
}

#[test]
fn materialize_mints_a_fresh_identity_and_copies_members_at_the_cut() {
    let dir = tempfile::tempdir().unwrap();
    let (mut home, mq, _) = pinned_legacy(dir.path());
    let entry = mq.entry().clone();
    let local = mq.store();
    assert_ne!(local.store_id().unwrap(), home.legacy_id);
    assert_ne!(local.log_epoch().unwrap(), home.legacy.log_epoch().unwrap());
    assert_eq!(
        (entry.store_id.clone(), entry.log_epoch.clone()),
        (local.store_id().unwrap(), local.log_epoch().unwrap())
    );
    assert_eq!(
        super::super::mini_fork_entry(&home.legacy, "agent-a").unwrap(),
        Some(entry.clone())
    );
    assert!(super::super::is_mini_quipu(local).unwrap());
    // Valid time is carried, and both kinds of member are present.
    assert_eq!(values(local, LEG), values(&home.legacy, LEG));
    assert_eq!(
        values(local, ROOT_GRAPH_IRI),
        values(&home.legacy, ROOT_GRAPH_IRI)
    );
    assert!(values(local, LEG).iter().any(|(_, from)| from == FROM));
    // Home writes after the cut never reach the mini-Quipu.
    legacy_write(&mut home, LEG, "after-the-cut");
    assert_ne!(values(local, LEG), values(&home.legacy, LEG));
    // Pinned shapes revision equals the recorded one.
    let pinned = super::super::base::shapes_hash(local, local.latest_tx_id().unwrap()).unwrap();
    assert_eq!(pinned, entry.shapes_hash);
}

#[test]
fn routed_owner_materializes_its_whole_set_at_the_recorded_digest() {
    let home = home();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mini.db");
    let mq = home
        .owner
        .materialize_mini_quipu(&spec(&[(DATA, Writable), (PROV, Writable)]), &path)
        .unwrap();
    assert_eq!(mq.entry().base.store_id, home.routed_id);
    assert_ne!(mq.store().store_id().unwrap(), home.routed_id);
    let head = mq.store().latest_tx_id().unwrap();
    let journals: Vec<_> = [DATA, PROV]
        .iter()
        .map(|g| member_journal(mq.store(), head, g).unwrap())
        .collect();
    assert_eq!(base_digest(&journals).unwrap(), mq.entry().base_digest);
    drop(mq);
    let entry = home.owner.mini_fork("agent-a").unwrap().unwrap();
    assert!(MiniQuipu::open(&path, &entry).is_ok());
}

#[test]
fn a_byte_copied_home_file_is_not_accepted_as_a_mini_quipu() {
    let dir = tempfile::tempdir().unwrap();
    let (home, mq, path) = pinned_legacy(dir.path());
    let entry = mq.entry().clone();
    drop(mq);
    assert!(MiniQuipu::open(&path, &entry).is_ok());
    let copy = dir.path().join("copied-home.db");
    home.legacy
        .conn
        .execute("VACUUM INTO ?1", [copy.to_str().unwrap()])
        .unwrap();
    let copied = Store::open(copy.to_str().unwrap()).unwrap();
    assert_eq!(
        copied.store_id().unwrap(),
        home.legacy_id,
        "store_id survives a copy"
    );
    drop(copied);
    let err = MiniQuipu::open(&copy, &entry).err().unwrap().to_string();
    assert!(err.contains("not a mini-Quipu"), "{err}");
}

// The pinned shapes gate exists only with SHACL; without it nothing refuses.
#[cfg(feature = "shacl")]
#[test]
fn local_writes_pass_the_pinned_shapes_gate() {
    let dir = tempfile::tempdir().unwrap();
    let (_home, mut mq, _) = pinned_legacy(dir.path());
    let ok = datum(mq.store(), "urn:local", Value::Str("local".into()));
    let tx = mq.transact(LEG, &[ok], LATER, Some("agent")).unwrap();
    assert_eq!(tx, mq.store().latest_tx_id().unwrap());
    let before = footprint(mq.store());
    let bad = datum(mq.store(), "urn:bad", Value::Int(5));
    let err = mq.transact(LEG, &[bad], LATER, Some("agent")).unwrap_err();
    let text = err.to_string().to_lowercase();
    assert!(
        text.contains("shacl") || text.contains("shape") || text.contains("datatype"),
        "{err}"
    );
    assert_eq!(footprint(mq.store()), before);
}

#[test]
fn x6_reference_write_refuses_with_nothing_written() {
    let dir = tempfile::tempdir().unwrap();
    let (_home, mut mq, _) = pinned_legacy(dir.path());
    let before = footprint(mq.store());
    let d = datum(mq.store(), "urn:item", Value::Str("changed".into()));
    let err = mq
        .transact(ROOT_GRAPH_IRI, std::slice::from_ref(&d), LATER, None)
        .unwrap_err();
    assert!(
        err.to_string().contains("X6 read-only-reference-member"),
        "{err}"
    );
    assert_eq!(footprint(mq.store()), before);
    // The backstop holds on the file's ordinary pipeline, not only the wrapper.
    let err = mq
        .store
        .transact_to_graph(&[d], LATER, None, None, 0)
        .unwrap_err();
    assert!(err.to_string().contains("X6"), "{err}");
    assert_eq!(footprint(mq.store()), before);
}

#[test]
fn x8_home_cursor_locally_and_local_cursor_at_home_refuse_with_nothing_written() {
    let dir = tempfile::tempdir().unwrap();
    let (home, mq, _) = pinned_legacy(dir.path());
    let (local_before, home_before) = (footprint(mq.store()), footprint(&home.legacy));
    let home_cursor = StoreCursor::head(&home.legacy).unwrap();
    let local_cursor = mq.head().unwrap();
    let err = mq.at(&home_cursor).err().unwrap().to_string();
    assert!(err.contains("X8 foreign-cursor"), "{err}");
    let err = local_cursor.admit(&home.legacy).err().unwrap().to_string();
    assert!(err.contains("X8 foreign-cursor"), "{err}");
    // Same tx number, other identity: the number alone is never a position.
    let forged = StoreCursor {
        tx: local_cursor.tx,
        ..home_cursor.clone()
    };
    assert!(mq.at(&forged).is_err());
    assert_eq!(mq.at(&local_cursor).ok().unwrap().tx, Some(local_cursor.tx));
    assert_eq!(
        home_cursor.admit(&home.legacy).ok().unwrap().tx,
        Some(home_cursor.tx)
    );
    assert_eq!(
        (footprint(mq.store()), footprint(&home.legacy)),
        (local_before, home_before)
    );
}

#[test]
fn a_home_refusal_leaves_no_file_and_no_entry() {
    let mut home = home();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mini.db");
    let before = footprint(&home.legacy);
    let err = materialize_on_legacy(
        &home.catalog,
        &mut home.legacy,
        &spec(&[(LEG, Writable), (DATA, Writable)]),
        &path,
    )
    .err()
    .unwrap()
    .to_string();
    assert!(err.contains("X4"), "{err}");
    assert!(!path.exists());
    assert_eq!(footprint(&home.legacy), before);
    assert_eq!(super::super::mini_fork_count(&home.legacy).unwrap(), 0);
}
