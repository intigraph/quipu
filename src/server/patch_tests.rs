//! `/patch` and `/diff` (aegis-4mk): idempotent re-apply, gate refusal,
//! conflicts, and the diff-then-patch round trip. Outcomes are judged by what
//! the store HOLDS afterwards, not only by the response.

use std::sync::Arc;

use axum::http::StatusCode;
use quipu::Store;
use quipu::changeset::{ChangeSet, SPARQL_UPDATE};
use quipu::changeset_store::{DiffSide, diff, side_statements};

use super::super::update::{Attribution, apply_update_as};
use super::super::{SharedStore, StoreHandle};
use super::apply;

const PROFILE: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .\n\
     @prefix ex: <http://ex.org/> .\n\
     ex:ActionShape a sh:NodeShape ; sh:targetClass ex:Action ;\n\
       sh:property [ sh:path ex:sourceKind ; sh:minCount 1 ] .";

fn shared(shapes: Option<&str>) -> SharedStore {
    let store = Store::open_in_memory().unwrap();
    if let Some(s) = shapes {
        store
            .load_shapes("profile", s, "2026-10-07T00:00:00Z")
            .unwrap();
    }
    Arc::new(StoreHandle::writer_only(store))
}

fn update(store: &SharedStore, text: &str) {
    apply_update_as(store, text, false).unwrap();
}

fn run(store: &SharedStore, cs: &ChangeSet) -> (StatusCode, serde_json::Value) {
    let response = apply(store, cs, &Attribution::default()).unwrap();
    let status = response.status();
    let bytes = futures::executor::block_on(axum::body::to_bytes(response.into_body(), usize::MAX))
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn main_state(store: &SharedStore) -> std::collections::BTreeSet<quipu::changeset::Statement> {
    side_statements(&store.lock(), &DiffSide::Main, None).unwrap()
}

const SEED: &str = "INSERT DATA { <http://ex.org/s> <http://ex.org/status> \"open\" ; \
                    <http://ex.org/owner> \"wu\" . \
                    GRAPH <http://ex.org/g> { <http://ex.org/s> <http://ex.org/note> \"n1\" } }";
const EDIT: &str = "DELETE DATA { <http://ex.org/s> <http://ex.org/status> \"open\" . \
                    GRAPH <http://ex.org/g> { <http://ex.org/s> <http://ex.org/note> \"n1\" } } ; \
                    INSERT DATA { <http://ex.org/s> <http://ex.org/status> \"closed\" ; \
                    <http://ex.org/size> 3 . \
                    GRAPH <http://ex.org/g> { <http://ex.org/s> <http://ex.org/note> \"n2\" } }";

#[test]
fn applying_twice_writes_once_and_the_retry_says_already_applied() {
    let store = shared(None);
    update(&store, SEED);
    let cs = ChangeSet::parse(EDIT, SPARQL_UPDATE).unwrap();

    let (status, first) = run(&store, &cs);
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["applied"], true);
    assert_eq!(first["asserted"], 3);
    assert_eq!(first["retracted"], 2);
    let after_first = main_state(&store);
    let head = store.lock().latest_tx_id().unwrap();

    let (status, second) = run(&store, &cs);
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["applied"], false);
    assert_eq!(second["already_applied"], true);
    assert!(second["tx"].is_null());
    assert_eq!(second["digest"], first["digest"]);
    // Nothing was written by the retry: same state, same head.
    assert_eq!(main_state(&store), after_first);
    assert_eq!(store.lock().latest_tx_id().unwrap(), head);
}

#[test]
fn a_gate_refusal_writes_nothing() {
    let store = shared(Some(PROFILE));
    // An ex:Action without the required ex:sourceKind: reject-mode SHACL refuses it.
    let cs = ChangeSet::parse(
        "INSERT DATA { <http://ex.org/a1> a <http://ex.org/Action> }",
        SPARQL_UPDATE,
    )
    .unwrap();
    let head = store.lock().latest_tx_id().unwrap();
    let err = apply(&store, &cs, &Attribution::default())
        .expect_err("a non-conforming change-set must be refused");
    assert!(format!("{err:?}").contains("SHACL"), "{err:?}");
    assert!(main_state(&store).is_empty());
    assert_eq!(store.lock().latest_tx_id().unwrap(), head);

    // An off-vocabulary type: the /knot type gate refuses it too.
    let cs = ChangeSet::parse(
        "INSERT DATA { <http://ex.org/u1> a <http://ex.org/Unknown> ; \
         <http://ex.org/sourceKind> \"observed\" }",
        SPARQL_UPDATE,
    )
    .unwrap();
    assert!(apply(&store, &cs, &Attribution::default()).is_err());
    assert!(main_state(&store).is_empty());
}

#[test]
fn a_mixed_target_is_a_conflict_that_writes_nothing() {
    let store = shared(None);
    update(&store, SEED);
    // Someone else already closed it, but the old status is still there too.
    update(
        &store,
        "INSERT DATA { <http://ex.org/s> <http://ex.org/status> \"closed\" }",
    );
    let before = main_state(&store);
    let cs = ChangeSet::parse(EDIT, SPARQL_UPDATE).unwrap();
    let (status, body) = run(&store, &cs);
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(
        body["already_inserted"],
        serde_json::json!(["<http://ex.org/s> <http://ex.org/status> \"closed\""])
    );
    assert_eq!(main_state(&store), before);
}

#[test]
fn diff_then_patch_reproduces_the_target_in_another_store() {
    const G: &str = "http://ex.org/g";
    let source = shared(None);
    update(&source, SEED);
    let base = source.lock().latest_tx_id().unwrap();
    update(&source, EDIT);
    let state = |store: &SharedStore, graph| {
        side_statements(&store.lock(), &DiffSide::Main, graph).unwrap()
    };
    // ROOT and the named graph, each as its own change-set.
    let changes = [None, Some(G)]
        .map(|graph| diff(&source.lock(), &DiffSide::Tx(base), &DiffSide::Main, graph).unwrap());
    assert!(changes.iter().all(|cs| !cs.is_empty()));
    // Over the wire and back, in both media types.
    for media in [quipu::changeset::SPARQL_UPDATE, quipu::changeset::RDF_PATCH] {
        let target = shared(None);
        update(&target, SEED);
        for cs in &changes {
            let wire = ChangeSet::parse(&cs.render(media), media).unwrap();
            let (status, body) = run(&target, &wire);
            assert_eq!(status, StatusCode::OK, "{body}");
        }
        for graph in [None, Some(G)] {
            assert_eq!(
                state(&target, graph),
                state(&source, graph),
                "{media} {graph:?}"
            );
        }
    }
}

#[test]
fn diff_then_patch_rewinds_a_store_to_an_earlier_tx() {
    let store = shared(None);
    update(&store, SEED);
    let base = store.lock().latest_tx_id().unwrap();
    let then = side_statements(&store.lock(), &DiffSide::Tx(base), None).unwrap();
    update(&store, EDIT);
    let rewind = diff(&store.lock(), &DiffSide::Main, &DiffSide::Tx(base), None).unwrap();
    let (status, body) = run(&store, &rewind);
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(main_state(&store), then);
}

#[test]
fn valid_time_and_source_come_from_the_header() {
    let store = shared(None);
    let cs = ChangeSet::parse(
        "# quipu-changeset: v1\n# valid-time: 2026-01-02T00:00:00Z\n# source: fork:feature\n\
         INSERT DATA { <http://ex.org/v> <http://ex.org/p> \"x\" }",
        SPARQL_UPDATE,
    )
    .unwrap();
    let attribution = super::attribution(&[], &cs).unwrap();
    assert_eq!(attribution.source, "fork:feature");
    apply(&store, &cs, &attribution).unwrap();
    let guard = store.lock();
    let rows = quipu::sparql::query(
        &guard,
        "SELECT ?tx WHERE { <http://ex.org/v> <http://ex.org/p> \"x\" }",
    )
    .unwrap();
    assert_eq!(rows.rows().len(), 1);
    let v = guard.lookup("http://ex.org/v").unwrap().unwrap();
    let facts = guard.entity_facts(v).unwrap();
    assert!(
        facts[0].valid_from.starts_with("2026-01-02"),
        "{:?}",
        facts[0]
    );
    let txs = guard.list_transactions().unwrap();
    assert_eq!(txs.last().unwrap().source.as_deref(), Some("fork:feature"));
}

#[test]
fn a_diff_over_the_ceiling_is_a_clear_422() {
    let store = shared(None);
    update(&store, SEED);
    let outcome = quipu::changeset_store::diff_bounded(
        &store.lock(),
        &DiffSide::Tx(0),
        &DiffSide::Main,
        None,
        1,
    )
    .unwrap();
    let response = super::diff_response(outcome, SPARQL_UPDATE);
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let bytes = futures::executor::block_on(axum::body::to_bytes(response.into_body(), usize::MAX))
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"], "diff_too_large");
    assert_eq!(body["side"], "main");
    assert_eq!(body["limit"], 1);
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("update_full_copy_max_facts")
    );
}

#[test]
fn a_bad_valid_time_is_refused_before_anything_is_written() {
    // Parsing is the gate: /patch parses before it takes the writer lock.
    let err = ChangeSet::parse(
        "# quipu-changeset: v1\n# valid-time: not-a-time\n\
         INSERT DATA { <http://ex.org/v> <http://ex.org/p> \"x\" }",
        SPARQL_UPDATE,
    )
    .unwrap_err();
    assert!(err.to_string().contains("valid-time"), "{err}");
}
