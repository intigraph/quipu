//! Writer ownership is held before the merge reads its inputs.
use super::*;

#[test]
fn reservation_excludes_an_independent_writer_and_releases_after_failure() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("store.db");
    let mut first = Store::open(path.to_str().unwrap()).unwrap();
    let second = Store::open(path.to_str().unwrap()).unwrap();
    second.conn.busy_timeout(std::time::Duration::ZERO).unwrap();
    let before = first.latest_tx_id().unwrap();
    let err = atomic(&mut first, |store| -> Result<()> {
        assert_eq!(store.latest_tx_id()?, before);
        let blocked = second
            .conn
            .execute("UPDATE transactions SET id=id WHERE 0", [])
            .unwrap_err();
        assert!(matches!(blocked, rusqlite::Error::SqliteFailure(e, _)
            if e.code == rusqlite::ErrorCode::DatabaseBusy));
        Err(Error::InvalidValue("abort after reserved read".into()))
    })
    .unwrap_err();
    assert!(err.to_string().contains("abort after reserved read"));
    assert!(first.conn.is_autocommit());
    second
        .conn
        .execute("UPDATE transactions SET id=id WHERE 0", [])
        .unwrap();
    assert_eq!(first.latest_tx_id().unwrap(), before);
}

#[test]
fn one_side_share_deletion_closes_unchanged_root_at_share_authored_time() {
    let mut store = Store::open_in_memory().unwrap();
    let text = "<urn:test:s> <urn:test:p> \"base\" .";
    crate::rdf::ingest_rdf(
        &mut store,
        text.as_bytes(),
        oxrdfio::RdfFormat::NTriples,
        None,
        "2026-01-01T00:00:00Z",
        None,
        None,
    )
    .unwrap();
    let graph = parse_graph(text, "base").unwrap();
    let empty = Graph::new();
    let plan = plan(
        &store,
        &Inputs {
            base: &graph,
            ours: &graph,
            theirs: &empty,
            shapes: "",
            authored_at: "2026-01-05T00:00:00Z",
        },
        &BTreeMap::new(),
        "2026-01-10T00:00:00Z",
    )
    .unwrap();
    assert_eq!(
        plan.receive,
        vec![(
            ("<urn:test:s>".into(), "urn:test:p".into()),
            FactOp::Close {
                value: "\"base\"".into(),
                at: "2026-01-05T00:00:00Z".into()
            },
        )]
    );
    assert!(plan.decide.is_empty());
}
