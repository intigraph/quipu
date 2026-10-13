use super::*;
use crate::{
    store::Datum,
    types::{Op, Value},
};

const PROJECT: &str = "https://example.org/project/board";
const COMMENTS: &str = "https://example.org/project/comments";

fn datum(store: &Store, text: &str, op: Op) -> Datum {
    Datum {
        entity: store.intern("https://example.org/item/one").unwrap(),
        attribute: store.intern("https://example.org/text").unwrap(),
        value: Value::Str(text.into()),
        valid_from: "2026-01-01T00:00:00Z".into(),
        valid_to: None,
        op,
    }
}

fn seed(store: &mut Store) -> i64 {
    for iri in [PROJECT, COMMENTS] {
        let graph = store.graph_create(iri).unwrap();
        let datums = [datum(store, "before", Op::Assert)];
        store
            .transact_to_graph(&datums, "2026-01-01T00:00:00Z", None, None, graph)
            .unwrap();
    }
    store.latest_tx_id().unwrap()
}

fn change(store: &mut Store) -> i64 {
    let mut batches = Vec::new();
    for iri in [PROJECT, COMMENTS] {
        batches.push((
            store.registered_graph_id(iri).unwrap().unwrap(),
            vec![
                datum(store, "before", Op::Retract),
                datum(store, "after", Op::Assert),
            ],
        ));
    }
    store
        .transact_graph_batches(&batches, "2026-02-01T00:00:00Z", None, None)
        .unwrap();
    store.latest_tx_id().unwrap()
}

fn request(store: &Store, tx: Option<i64>) -> Request {
    Request {
        source_store_id: store.store_id().unwrap(),
        graphs: vec![PROJECT.into(), COMMENTS.into()],
        tx,
        max_bytes: None,
    }
}

#[test]
fn acquisition_before_retraction_after_and_present_have_applied_pins() {
    let mut store = Store::open_in_memory().unwrap();
    let before = seed(&mut store);
    let after = change(&mut store);
    for (pin, expected, applied) in [
        (Some(before), "before", before),
        (Some(after), "after", after),
        (None, "after", after),
    ] {
        let req = request(&store, pin);
        let payload = acquire(&store, &req).unwrap();
        payload.verify(&req).unwrap();
        assert_eq!(payload.source_receipt.applied_tx, applied);
        assert_eq!(payload.source_receipt.requested_tx, pin);
        for graph in payload.graphs.values() {
            assert!(graph.contains(&format!("\"{expected}\"")), "{graph}");
            assert!(!graph.contains(if expected == "before" {
                "\"after\""
            } else {
                "\"before\""
            }));
        }
    }
    let graph = store.registered_graph_id(PROJECT).unwrap().unwrap();
    let retract = [datum(&store, "after", Op::Retract)];
    let retracted = store
        .transact_to_graph(&retract, "2026-03-01T00:00:00Z", None, None, graph)
        .unwrap();
    let old = acquire(&store, &request(&store, Some(after))).unwrap();
    assert!(old.graphs[PROJECT].contains("after"));
    let empty = acquire(&store, &request(&store, Some(retracted))).unwrap();
    assert_eq!(empty.graphs[PROJECT], "");
    assert_eq!(empty.source_receipt.graphs[PROJECT].triples, 0);
    assert_eq!(
        empty.source_receipt.graphs[COMMENTS].triples, 1,
        "positive scope control"
    );
}

#[test]
fn concurrent_writer_cannot_skew_head_payload_or_graphs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.db");
    let mut writer = Store::open(path.to_str().unwrap()).unwrap();
    let head = seed(&mut writer);
    let reader = Store::open_read_only(path.to_str().unwrap()).unwrap();
    let req = request(&reader, None);
    let payload = acquire_with_control(&reader, &req, || {
        change(&mut writer);
        let graph = writer.registered_graph_id(COMMENTS)?.unwrap();
        let retract = [datum(&writer, "after", Op::Retract)];
        writer.transact_to_graph(&retract, "2026-03-01T00:00:00Z", None, None, graph)?;
        writer.graph_unregister(graph)?;
        Ok(())
    })
    .unwrap();
    payload.verify(&req).unwrap();
    assert_eq!(payload.source_receipt.source_head_tx, head);
    for graph in payload.graphs.values() {
        assert!(graph.contains("before"));
        assert!(!graph.contains("after"));
    }
    assert!(
        writer.latest_tx_id().unwrap() > head,
        "writer must actually commit during snapshot"
    );
    assert!(
        writer.registered_graph_id(COMMENTS).unwrap().is_none(),
        "writer must remove the graph during the snapshot"
    );
    writer.graph_create(COMMENTS).unwrap();
    let next = acquire(&reader, &request(&reader, None)).unwrap();
    assert!(
        next.graphs[PROJECT].contains("after"),
        "transaction must release after response"
    );
}

#[test]
fn malformed_foreign_scope_and_truncated_acquisitions_fail_closed() {
    let mut store = Store::open_in_memory().unwrap();
    let head = seed(&mut store);
    let mut req = request(&store, Some(head));
    acquire(&store, &req).unwrap().verify(&req).unwrap();
    for tx in [0, -1, i64::MAX, head + 1] {
        req.tx = Some(tx);
        assert!(acquire(&store, &req).is_err(), "tx={tx}");
    }
    req.tx = Some(head);
    req.source_store_id = "foreign".into();
    assert!(acquire(&store, &req).is_err());
    req = request(&store, Some(head));
    req.graphs.push("https://example.org/unknown".into());
    assert!(acquire(&store, &req).is_err());
    req = request(&store, Some(head));
    req.graphs.push(PROJECT.into());
    assert!(acquire(&store, &req).is_err());
    req = request(&store, Some(head));
    req.max_bytes = Some(1);
    assert!(acquire(&store, &req).is_err());
    req.max_bytes = None;
    store.search_config_mut().max_sparql_rows = 0;
    assert!(
        acquire(&store, &req).is_err(),
        "truncation cannot mint complete receipt"
    );
}

#[test]
fn wire_pins_never_coerce_wrong_types_or_mixed_clocks() {
    let mut store = Store::open_in_memory().unwrap();
    let head = seed(&mut store);
    for value in [
        json!(null),
        json!("1"),
        json!(true),
        json!(1.5),
        json!([]),
        json!({}),
        json!(0),
        json!(-1),
        json!(u64::MAX),
    ] {
        let input =
            json!({"source_store_id": store.store_id().unwrap(), "graphs": [PROJECT], "tx":value});
        assert!(serde_json::from_value::<Request>(input).is_err(), "{value}");
        assert!(
            crate::mcp::tool_query(&store, &json!({"query":"ASK {}", "tx":value})).is_err(),
            "{value}"
        );
    }
    for field in ["valid_at", "fork"] {
        let mut input = json!({"query":"ASK {}", "tx":head});
        input[field] = json!("2026-01-01T00:00:00Z");
        assert!(crate::mcp::tool_query(&store, &input).is_err());
    }
    assert_eq!(
        crate::mcp::tool_query(&store, &json!({"query":"ASK {}", "tx":head})).unwrap()["result"],
        true
    );
    assert_eq!(
        crate::mcp::tool_query(&store, &json!({"query":"ASK {}"})).unwrap()["result"],
        true
    );
}

#[test]
fn source_receipt_and_graph_tampering_and_wrong_expectations_are_rejected() {
    let mut store = Store::open_in_memory().unwrap();
    seed(&mut store);
    let req = request(&store, None);
    let payload = acquire(&store, &req).unwrap();
    let encoded = serde_json::to_vec(&payload).unwrap();
    let mut missing = serde_json::to_value(&payload).unwrap();
    missing["source_receipt"]
        .as_object_mut()
        .unwrap()
        .remove("requested_tx");
    assert!(
        serde_json::from_value::<Payload>(missing).is_err(),
        "missing nullable field is UNKNOWN, not a present-mode receipt"
    );
    let mut copy: Payload = serde_json::from_slice(&encoded).unwrap();
    copy.verify(&req).unwrap();
    copy.graphs
        .get_mut(PROJECT)
        .unwrap()
        .push_str("<https://example.org/a> <https://example.org/p> \"tampered\" .\n");
    assert!(copy.verify(&req).is_err());
    let mut copy: Payload = serde_json::from_slice(&encoded).unwrap();
    copy.source_receipt.source_head_tx += 1;
    assert!(copy.verify(&req).is_err());
    let mut wrong = request(&store, Some(store.latest_tx_id().unwrap()));
    assert!(
        payload.verify(&wrong).is_err(),
        "same applied pin is not same requested mode"
    );
    wrong = request(&store, None);
    wrong.source_store_id = "other".into();
    assert!(payload.verify(&wrong).is_err());
    wrong = request(&store, None);
    wrong.graphs.pop();
    assert!(payload.verify(&wrong).is_err());
}

#[test]
fn local_export_positive_deadline_unknown_tx_and_unsupported_graph_controls() {
    let mut store = Store::open_in_memory().unwrap();
    seed(&mut store);
    let req = request(&store, None);
    let payload = acquire(&store, &req).unwrap();
    let (export, count) =
        crate::export_rdf_subset(&store, oxrdfio::RdfFormat::NTriples, Some(PROJECT)).unwrap();
    assert_eq!(count, 1);
    assert_eq!(
        crate::share::canonicalize_ntriples(&export).unwrap(),
        payload.graphs[PROJECT].as_bytes()
    );
    {
        let _deadline =
            crate::time::set_request_deadline(Some(crate::time::Deadline::after_millis(0)));
        assert!(matches!(
            acquire(&store, &req),
            Err(Error::QueryTimeout { .. })
        ));
    }
    acquire(&store, &req).unwrap();
    store
        .conn
        .execute(
            "INSERT INTO transactions(id, timestamp) VALUES (100, '2026-03-01T00:00:00Z')",
            [],
        )
        .unwrap();
    assert!(
        acquire(&store, &request(&store, Some(50))).is_err(),
        "unknown tx below head must refuse"
    );
    let overlay = "https://example.org/overlay";
    store.overlay_create(overlay, 0).unwrap();
    let mut scope = request(&store, None);
    scope.graphs = vec![overlay.into()];
    assert!(acquire(&store, &scope).is_err());
    for field in ["valid_at", "fork", "graph", "query", "complete"] {
        let mut wire = json!({"source_store_id": store.store_id().unwrap(), "graphs": [PROJECT]});
        wire[field] = json!("unexpected");
        assert!(serde_json::from_value::<Request>(wire).is_err(), "{field}");
    }
}

#[test]
fn attached_stores_refuse_source_receipts_and_pins_but_present_queries_work() {
    let dir = tempfile::tempdir().unwrap();
    let attachment = dir.path().join("attached.db");
    Store::open(attachment.to_str().unwrap()).unwrap();
    let spaced = dir.path().join("attached-spaced.db");
    crate::store::respace::respace_file(&attachment, &spaced, 1).unwrap();
    let main = dir.path().join("main.db");
    let mut store = Store::open_with_attachments(
        main.to_str().unwrap(),
        &[crate::store::attach::Attachment::read_only(
            "layer",
            spaced.to_str().unwrap(),
        )],
    )
    .unwrap();
    let head = seed(&mut store);
    assert!(acquire(&store, &request(&store, Some(head))).is_err());
    assert!(acquire(&store, &request(&store, None)).is_err());
    assert!(crate::mcp::tool_query(&store, &json!({"query":"ASK {}", "tx":head})).is_err());
    assert_eq!(
        crate::mcp::tool_query(&store, &json!({"query":"ASK {}"})).unwrap()["result"],
        true
    );
}

#[test]
fn graph_order_roundtrip_and_source_blank_node_identity_are_preserved() {
    let mut store = Store::open_in_memory().unwrap();
    let shared = store.intern("_:shared").unwrap();
    let attribute = store.intern("https://example.org/link").unwrap();
    for (iri, label) in [(PROJECT, "_:project"), (COMMENTS, "_:comment")] {
        let graph = store.graph_create(iri).unwrap();
        let entity = store.intern(label).unwrap();
        let datums = [Datum {
            entity,
            attribute,
            value: Value::Ref(shared),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
            op: Op::Assert,
        }];
        store
            .transact_to_graph(&datums, "2026-01-01T00:00:00Z", None, None, graph)
            .unwrap();
    }
    let mut req = request(&store, None);
    let first = acquire(&store, &req).unwrap();
    first.verify(&req).unwrap();
    assert!(first.graphs[PROJECT].contains("_:project"));
    assert!(first.graphs[COMMENTS].contains("_:comment"));
    for graph in first.graphs.values() {
        assert!(graph.contains("_:shared"));
    }
    req.graphs.reverse();
    let reversed = acquire(&store, &req).unwrap();
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&reversed).unwrap()
    );
    let roundtrip: Payload = serde_json::from_slice(&serde_json::to_vec(&first).unwrap()).unwrap();
    roundtrip.verify(&req).unwrap();
    assert_eq!(roundtrip.source_receipt, first.source_receipt);
}
