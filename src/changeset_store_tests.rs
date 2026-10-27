use super::*;
use crate::changeset::{SPARQL_UPDATE, Statement};

fn ingest(store: &mut Store, nt: &str) -> i64 {
    crate::rdf::ingest_rdf(
        store,
        nt.as_bytes(),
        oxrdfio::RdfFormat::NTriples,
        None,
        "2026-10-01T00:00:00Z",
        None,
        None,
    )
    .unwrap()
    .0
}

fn st(triple: &str) -> Statement {
    Statement::new(triple, None)
}

const A: &str = "<urn:x:a> <urn:x:p> \"one\"";
const B: &str = "<urn:x:b> <urn:x:p> \"two\"";

#[test]
fn a_tx_range_diff_names_its_base_and_target() {
    let mut store = Store::open_in_memory().unwrap();
    let t1 = ingest(&mut store, &format!("{A} .\n"));
    ingest(&mut store, &format!("{B} .\n"));
    let a = store.lookup("urn:x:a").unwrap().unwrap();
    store
        .retract_triples(a, None, None, "2026-10-02T00:00:00Z", None, true, None)
        .unwrap();
    let head = store.latest_tx_id().unwrap();

    let cs = diff(&store, &DiffSide::Tx(t1), &DiffSide::Main, None).unwrap();
    assert_eq!(cs.deletes, vec![st(A)]);
    assert_eq!(cs.inserts, vec![st(B)]);
    assert_eq!(cs.header.base_tx, Some(t1));
    assert_eq!(cs.header.target_tx, Some(head));
    assert_eq!(cs.header.store_id, Some(store.store_id().unwrap()));
    assert!(cs.header.shapes_hash.is_some());
    assert_eq!(
        cs.header.source.as_deref(),
        Some(format!("diff:tx:{t1}..main").as_str())
    );

    // The reverse range is the inverse change.
    let back = diff(&store, &DiffSide::Main, &DiffSide::Tx(t1), None).unwrap();
    assert_eq!(back.deletes, cs.inserts);
    assert_eq!(back.inserts, cs.deletes);
}

#[test]
fn a_fork_diff_reads_as_a_change_to_root() {
    let mut store = Store::open_in_memory().unwrap();
    let t1 = ingest(&mut store, &format!("{A} .\n"));
    let fork = store
        .fork_create("feature", t1, "2026-10-02T00:00:00Z", None)
        .unwrap();
    let b = store.intern("urn:x:b").unwrap();
    let p = store.intern("urn:x:p").unwrap();
    store
        .transact_to_graph(
            &[crate::store::Datum {
                entity: b,
                attribute: p,
                value: crate::Value::Str("two".into()),
                valid_from: "2026-10-02T00:00:00Z".into(),
                valid_to: None,
                op: crate::Op::Assert,
            }],
            "2026-10-02T00:00:00Z",
            None,
            None,
            fork.g,
        )
        .unwrap();
    let cs = diff(
        &store,
        &DiffSide::Main,
        &DiffSide::Fork("feature".into()),
        None,
    )
    .unwrap();
    assert!(cs.deletes.is_empty());
    assert_eq!(cs.inserts, vec![st(B)]);
}

#[test]
fn sides_parse_and_bad_sides_are_refused() {
    assert_eq!(DiffSide::parse("tx:7").unwrap(), DiffSide::Tx(7));
    assert_eq!(DiffSide::parse("7").unwrap(), DiffSide::Tx(7));
    assert_eq!(DiffSide::parse("MAIN").unwrap(), DiffSide::Main);
    assert_eq!(
        DiffSide::parse("fork:f").unwrap(),
        DiffSide::Fork("f".into())
    );
    assert!(DiffSide::parse("tx:-1").is_err());
    assert!(DiffSide::parse("share:abc").is_err());

    let store = Store::open_in_memory().unwrap();
    let err = diff(&store, &DiffSide::Tx(999), &DiffSide::Main, None).unwrap_err();
    assert!(err.to_string().contains("beyond"), "{err}");
    assert!(
        diff(
            &store,
            &DiffSide::Main,
            &DiffSide::Fork("nope".into()),
            None
        )
        .is_err()
    );
}

#[test]
fn classify_names_pre_state_post_state_and_mixed() {
    let mut store = Store::open_in_memory().unwrap();
    ingest(&mut store, &format!("{A} .\n"));
    let forward = ChangeSet::new(Header::default(), vec![st(A)], vec![st(B)]).unwrap();
    assert_eq!(classify(&store, &forward).unwrap(), PatchState::Applicable);

    // The reverse change's post-state (A present, B absent) is the store now.
    let reverse = ChangeSet::new(Header::default(), vec![st(B)], vec![st(A)]).unwrap();
    assert_eq!(
        classify(&store, &reverse).unwrap(),
        PatchState::AlreadyApplied
    );

    // Mixed: A present (pre-state) but B also already present (post-state).
    ingest(&mut store, &format!("{B} .\n"));
    assert_eq!(
        classify(&store, &forward).unwrap(),
        PatchState::Conflict {
            already_deleted: vec![],
            already_inserted: vec![st(B)],
        }
    );

    let empty = ChangeSet::new(Header::default(), vec![], vec![]).unwrap();
    assert_eq!(
        classify(&store, &empty).unwrap(),
        PatchState::AlreadyApplied
    );
}

#[test]
fn classify_compares_literals_in_the_stores_value_model() {
    let mut store = Store::open_in_memory().unwrap();
    ingest(
        &mut store,
        "<urn:x:a> <urn:x:n> \"5\"^^<http://www.w3.org/2001/XMLSchema#integer> .\n",
    );
    let cs = ChangeSet::parse("DELETE DATA { <urn:x:a> <urn:x:n> 5 }", SPARQL_UPDATE).unwrap();
    assert_eq!(classify(&store, &cs).unwrap(), PatchState::Applicable);
}

#[test]
fn blank_nodes_are_refused_on_both_paths() {
    let mut store = Store::open_in_memory().unwrap();
    ingest(&mut store, "_:b1 <urn:x:p> \"x\" .\n");
    let err = diff(&store, &DiffSide::Tx(0), &DiffSide::Main, None).unwrap_err();
    assert!(err.to_string().contains("blank node"), "{err}");

    let cs = ChangeSet::parse("INSERT DATA { _:n <urn:x:p> \"y\" }", SPARQL_UPDATE).unwrap();
    let err = classify(&store, &cs).unwrap_err();
    assert!(err.to_string().contains("blank node"), "{err}");
}

#[test]
fn a_named_graph_diff_carries_its_graph() {
    let mut store = Store::open_in_memory().unwrap();
    store.graph_create("urn:x:g").unwrap();
    let g = store.lookup("urn:x:g").unwrap().unwrap();
    let t0 = store.latest_tx_id().unwrap();
    let a = store.intern("urn:x:a").unwrap();
    let p = store.intern("urn:x:p").unwrap();
    store
        .transact_to_graph(
            &[crate::store::Datum {
                entity: a,
                attribute: p,
                value: crate::Value::Str("one".into()),
                valid_from: "2026-10-02T00:00:00Z".into(),
                valid_to: None,
                op: crate::Op::Assert,
            }],
            "2026-10-02T00:00:00Z",
            None,
            None,
            g,
        )
        .unwrap();
    let cs = diff(&store, &DiffSide::Tx(t0), &DiffSide::Main, Some("urn:x:g")).unwrap();
    assert_eq!(cs.inserts, vec![Statement::new(A, Some("urn:x:g".into()))]);
    // ROOT is untouched, and an unknown graph or a fork+graph is refused.
    assert!(
        diff(&store, &DiffSide::Tx(t0), &DiffSide::Main, None)
            .unwrap()
            .is_empty()
    );
    assert!(diff(&store, &DiffSide::Main, &DiffSide::Main, Some("urn:x:nope")).is_err());
    assert!(
        diff(
            &store,
            &DiffSide::Main,
            &DiffSide::Fork("f".into()),
            Some("urn:x:g")
        )
        .is_err()
    );
}
