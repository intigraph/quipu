use super::*;
use axum::extract::State;
use std::sync::Arc;
use std::time::Duration;

const TS: &str = "2026-01-01T00:00:00Z";
const ASK: &str = "ASK FROM <urn:quipu:graph:root> FROM <urn:quipu:graph:root#inferred> \
    { <urn:twin> <urn:likes> <urn:object> }";

#[tokio::test]
async fn ontology_window_production_handler_preview_and_apply() {
    let (_dir, store) = seeded();
    let (before, through) = {
        let mut live = store.lock();
        let graph = live.ensure_companion_inferred_graph(0, TS).unwrap();
        let before = live.latest_tx_id().unwrap();
        let d = quipu::Datum {
            entity: live.intern("urn:window:derived").unwrap(),
            attribute: live.intern("urn:window:predicate").unwrap(),
            value: quipu::Value::Str("entailed".into()),
            valid_from: TS.into(),
            valid_to: None,
            op: quipu::Op::Assert,
        };
        let through = live
            .transact_to_graph(&[d], TS, None, Some("owl:materialize"), graph)
            .unwrap();
        (before, through)
    };
    let body = json!({"action":"retract_materialization", "name":"test", "after_tx":before, "through_tx":through});
    let preview = crate::tools::ontology(
        State(store.clone()),
        axum::http::HeaderMap::new(),
        None,
        axum::Json(body.clone()),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(preview["planned"], 1);
    assert_eq!(preview["applied"], false);
    let mut apply = body;
    apply["apply"] = json!(true);
    apply["expect"] = preview["expect"].clone();
    let result = crate::tools::ontology(
        State(store.clone()),
        axum::http::HeaderMap::new(),
        None,
        axum::Json(apply),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(result["remaining"], 0);
    assert_eq!(result["retracted"], 1);
    assert_eq!(result["ontology_removed"], false);
    assert_eq!(store.lock().list_ontologies().unwrap().len(), 1);
}

fn seeded() -> (tempfile::TempDir, SharedStore) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    let mut store = quipu::Store::open(path.to_str().unwrap()).unwrap();
    store
        .load_ontology(
            "test",
            "<urn:A> <http://www.w3.org/2000/01/rdf-schema#subClassOf> <urn:B> .",
            TS,
        )
        .unwrap();
    let subject = store.intern("urn:original").unwrap();
    let twin = store.intern("urn:twin").unwrap();
    let same_as = store
        .intern("http://www.w3.org/2002/07/owl#sameAs")
        .unwrap();
    let likes = store.intern("urn:likes").unwrap();
    let object = store.intern("urn:object").unwrap();
    store
        .transact(
            &[
                quipu::Datum {
                    entity: subject,
                    attribute: likes,
                    value: quipu::Value::Ref(object),
                    valid_from: TS.into(),
                    valid_to: None,
                    op: quipu::Op::Assert,
                },
                quipu::Datum {
                    entity: twin,
                    attribute: same_as,
                    value: quipu::Value::Ref(subject),
                    valid_from: TS.into(),
                    valid_to: None,
                    op: quipu::Op::Assert,
                },
            ],
            TS,
            None,
            None,
        )
        .unwrap();
    let readers = crate::ReadPool::open(path.to_str().unwrap(), &store, 1);
    let mut handle = crate::StoreHandle::writer_only(store);
    handle.readers = readers;
    (dir, Arc::new(handle))
}

#[tokio::test]
#[allow(clippy::await_holding_lock)] // Intentionally stall snapshot capture, not the writer.
async fn scheduled_handler_entails_and_leaves_ordinary_write_admission_free() {
    let (_dir, store) = seeded();
    assert!(matches!(
        quipu::sparql::query(&store.lock(), ASK).unwrap(),
        quipu::sparql::QueryResult::Ask(false)
    ));
    let held_reader = store.readers.conns[0].lock();
    let task_store = store.clone();
    let task = tokio::spawn(async move {
        crate::tools::ontology(
            State(task_store),
            axum::http::HeaderMap::new(),
            None,
            axum::Json(json!({"action":"materialize"})),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while MATERIALIZE.available_permits() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("handler must enter independent materialisation admission");
    let writer = store.clone();
    tokio::time::timeout(
        Duration::from_secs(5),
        write_blocking(move || {
            // Real writer work while materialisation is blocked in capture.
            writer.lock().intern("urn:concurrent-writer")?;
            Ok(())
        }),
    )
    .await
    .expect("materialisation must not monopolise write admission")
    .unwrap();
    // Overlap is rejected, rather than accumulating another full snapshot.
    assert!(
        materialize(store.clone(), json!({"action":"materialize"}))
            .await
            .is_err()
    );
    drop(held_reader);
    let result = tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .0;
    assert_eq!(result["complete"], true);
    assert!(
        result["materialized"]["same_as_inferences"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(matches!(
        quipu::sparql::query(&store.lock(), ASK).unwrap(),
        quipu::sparql::QueryResult::Ask(true)
    ));
    assert!(matches!(
        quipu::sparql::query(&store.lock(), &ASK.replace("urn:twin", "urn:unrelated")).unwrap(),
        quipu::sparql::QueryResult::Ask(false)
    ));
    // A second completed run is idempotent.
    let second = materialize(store, json!({"action":"materialize"}))
        .await
        .unwrap()
        .0;
    assert_eq!(second["materialized"]["total"], 0);
}
