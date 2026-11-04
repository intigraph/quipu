use std::fs;
use std::path::PathBuf;

use crate::store::Store;
use crate::{Datum, Value};

use super::import_graph;

/// The `TempDir` is returned so the caller HOLDS it: dropping it removes the
/// two stores, including when the test panics. Returning bare paths leaked a
/// directory per process, per name (aegis-t4oyjy).
fn paths(name: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix(&format!("quipu-import-{name}-"))
        .tempdir()
        .unwrap();
    let local = dir.path().join("local.db");
    let foreign = dir.path().join("foreign.db");
    (dir, local, foreign)
}

#[test]
fn imports_with_eager_alias_remap_and_ref_reachability() {
    let (_tmp, local, foreign) = paths("aliases");
    let dst = Store::open(local.to_str().unwrap()).unwrap();
    let shared_local = dst.intern("urn:shared").unwrap();
    let collision_local = dst.intern("urn:local-only").unwrap();

    let mut src = Store::open(foreign.to_str().unwrap()).unwrap();
    let collision_foreign = src.intern("urn:foreign-only").unwrap();
    let shared_foreign = src.intern("urn:shared").unwrap();
    assert_eq!(
        shared_local, collision_foreign,
        "fixture must contain different IRIs at the same raw id"
    );
    assert_ne!(
        shared_local, shared_foreign,
        "fixture must contain the same IRI at different raw ids"
    );
    let p = src.intern("urn:p").unwrap();
    src.transact(
        &[Datum {
            entity: shared_foreign,
            attribute: p,
            value: Value::Ref(collision_foreign),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
            op: crate::Op::Assert,
        }],
        "2026-01-01T00:00:00Z",
        None,
        None,
    )
    .unwrap();
    drop(src);

    let before = fs::read(&foreign).unwrap();
    let report = import_graph(&local, &foreign, "urn:imported").unwrap();
    assert_eq!(
        fs::read(&foreign).unwrap(),
        before,
        "source must remain byte-identical"
    );
    drop(dst);
    let store = Store::open(local.to_str().unwrap()).unwrap();
    assert_eq!(store.lookup("urn:shared").unwrap(), Some(shared_local));
    assert_eq!(
        store.lookup("urn:local-only").unwrap(),
        Some(collision_local)
    );
    let crate::sparql::QueryResult::Select { rows, .. } = crate::sparql::query(
        &store,
        "SELECT ?o WHERE { GRAPH <urn:imported> { <urn:shared> <urn:p> ?o } }",
    )
    .unwrap() else {
        panic!("expected SELECT")
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].get("o"),
        Some(&Value::Ref(
            store.lookup("urn:foreign-only").unwrap().unwrap()
        ))
    );
    assert_eq!(report.facts, 1);
}

#[test]
fn unclassified_source_column_refuses_before_destination_write() {
    let (_tmp, local, foreign) = paths("unknown-column");
    drop(Store::open(local.to_str().unwrap()).unwrap());
    drop(Store::open(foreign.to_str().unwrap()).unwrap());
    rusqlite::Connection::open(&foreign)
        .unwrap()
        .execute_batch("ALTER TABLE facts ADD COLUMN mystery INTEGER")
        .unwrap();
    let before = fs::read(&local).unwrap();
    let err = import_graph(&local, &foreign, "urn:nope")
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("facts.mystery") && err.contains("COLUMN_CLASSIFICATION"),
        "{err}"
    );
    assert_eq!(fs::read(&local).unwrap(), before);
}

#[test]
fn import_carries_embeddings_re_keyed_by_iri() {
    // The recipient half of quipu-0v4: an archive (or any `--with-vectors`
    // pack) handed to another store must arrive WITH its semantic index. This
    // path silently dropped every `vectors` row before, which made
    // `pack --with-vectors` produce a file nothing could restore from.
    use crate::vector::KnowledgeVectorStore;
    let (_tmp, local, foreign) = paths("vectors");
    // A destination that already interns an unrelated IRI, so the id spaces
    // differ and the join has to be by IRI rather than by raw id.
    let dst = Store::open(local.to_str().unwrap()).unwrap();
    dst.intern("urn:local-decoy").unwrap();
    drop(dst);

    let mut src = Store::open(foreign.to_str().unwrap()).unwrap();
    let e = src.intern("urn:subject").unwrap();
    let p = src.intern("urn:p").unwrap();
    src.transact(
        &[Datum {
            entity: e,
            attribute: p,
            value: Value::Str("hello".into()),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
            op: crate::Op::Assert,
        }],
        "2026-01-01T00:00:00Z",
        None,
        None,
    )
    .unwrap();
    let emb = vec![0.0f32, 1.0, 0.0, 0.0];
    src.embed_entity(e, "the subject", &emb, "2026-01-01T00:00:00Z")
        .unwrap();
    drop(src);

    let report = import_graph(&local, &foreign, "urn:imported").unwrap();
    assert_eq!(report.vectors, 1, "the embedding must travel");

    let store = Store::open(local.to_str().unwrap()).unwrap();
    let matches = store.vector_search(&emb, 5, None).unwrap();
    assert_eq!(matches.len(), 1, "and be searchable here: {matches:?}");
    assert_eq!(
        store.resolve(matches[0].entity_id).unwrap(),
        "urn:subject",
        "re-keyed onto THIS store's id for that IRI"
    );
}

fn protected_destination_must_refuse(lifecycle: Option<&str>, source: Option<&str>) {
    let (temp, local, foreign) = paths("protected-destination");
    let mut dst = Store::open(local.to_str().unwrap()).unwrap();
    let graph = dst.graph_create("urn:protected-import").unwrap();
    if lifecycle == Some("frozen") {
        let entity = dst.intern("urn:existing-frozen-subject").unwrap();
        let attribute = dst.intern("urn:existing-frozen-predicate").unwrap();
        dst.transact_to_graph(
            &[Datum {
                entity,
                attribute,
                value: Value::Str("retained".into()),
                valid_from: "2026-01-01T00:00:00Z".into(),
                valid_to: None,
                op: crate::Op::Assert,
            }],
            "2026-01-01T00:00:00Z",
            None,
            None,
            graph,
        )
        .unwrap();
        let archive = temp.path().join("archive");
        fs::create_dir(&archive).unwrap();
        let frozen = dst
            .freeze_graph(
                "urn:protected-import",
                archive.to_str().unwrap(),
                "2026-01-02T00:00:00Z",
                None,
            )
            .unwrap();
        assert_eq!(frozen.facts, 1, "freeze did not relocate populated graph");
        let crate::sparql::QueryResult::Select { rows, .. } = crate::sparql::query(
            &dst, "SELECT ?value WHERE { GRAPH <urn:protected-import> { <urn:existing-frozen-subject> <urn:existing-frozen-predicate> ?value } }",
        ).unwrap() else { panic!("expected SELECT") };
        assert_eq!(rows.len(), 1, "frozen graph is not readable in fixture");
    } else {
        dst.conn
            .execute(
                "UPDATE graphs SET source=?1 WHERE g=?2",
                rusqlite::params![source, graph],
            )
            .unwrap();
    }
    let mut src = Store::open(foreign.to_str().unwrap()).unwrap();
    let subject = src.intern("urn:import-must-not-land").unwrap();
    let predicate = src.intern("urn:import-predicate").unwrap();
    src.transact(
        &[Datum {
            entity: subject,
            attribute: predicate,
            value: Value::Str("probe".into()),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
            op: crate::Op::Assert,
        }],
        "2026-01-01T00:00:00Z",
        None,
        None,
    )
    .unwrap();
    assert_eq!(
        src.conn
            .query_row("SELECT COUNT(*) FROM facts", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(src);
    let counts = |store: &Store| {
        store.conn.query_row(
        "SELECT (SELECT COUNT(*) FROM terms), (SELECT COUNT(*) FROM transactions), (SELECT COUNT(*) FROM facts)",
        [], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)),
    ).unwrap()
    };
    let before = counts(&dst);
    let result = import_graph(&local, &foreign, "urn:protected-import");
    assert!(result.is_err(), "protected target imported: {result:?}");
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("FROZEN") || error.contains("mounted read-only"),
        "{error}"
    );
    assert_eq!(counts(&dst), before, "refused import mutated destination");
    assert!(dst.lookup("urn:import-must-not-land").unwrap().is_none());
}

#[test]
fn offline_import_refuses_frozen_destination_before_copy() {
    protected_destination_must_refuse(Some("frozen"), None);
}

#[test]
fn offline_import_refuses_attached_destination_before_copy() {
    protected_destination_must_refuse(None, Some("synthetic_read_only_attachment"));
}

#[test]
fn offline_import_refuses_lost_native_ownership_before_dictionary_copy() {
    let (_tmp, local, foreign) = paths("lost-ownership");
    drop(Store::open(local.to_str().unwrap()).unwrap());
    Store::adopt(local.to_str().unwrap()).unwrap();
    let source = Store::open(foreign.to_str().unwrap()).unwrap();
    source.intern("urn:owned-initial-probe").unwrap();
    drop(source);
    let accepted = import_graph(&local, &foreign, "urn:owned-positive").unwrap();
    assert!(accepted.terms > 0, "valid owned destination did not import");
    let source = Store::open(foreign.to_str().unwrap()).unwrap();
    source.intern("urn:ownership-import-probe").unwrap();
    drop(source);
    std::fs::remove_file(local.with_extension("db.ownership.json")).unwrap();
    let inspect = Store::open(local.to_str().unwrap()).unwrap();
    assert!(
        inspect.ensure_ownership_writable().is_err(),
        "fixture still writable"
    );
    let before: i64 = inspect
        .conn
        .query_row("SELECT COUNT(*) FROM terms", [], |r| r.get(0))
        .unwrap();
    let error = import_graph(&local, &foreign, "urn:ownership-import-target")
        .unwrap_err()
        .to_string();
    assert!(error.contains("store ownership"), "{error}");
    let after: i64 = inspect
        .conn
        .query_row("SELECT COUNT(*) FROM terms", [], |r| r.get(0))
        .unwrap();
    assert_eq!(after, before);
    assert!(
        inspect
            .lookup("urn:ownership-import-probe")
            .unwrap()
            .is_none()
    );
}

#[test]
fn offline_import_refuses_attached_iri_absent_from_main_dictionary() {
    let (temp, local, foreign) = paths("external-only-dictionary");
    drop(Store::open(local.to_str().unwrap()).unwrap());
    let mut layer = Store::open(foreign.to_str().unwrap()).unwrap();
    let graph = layer.graph_create("urn:external-only-graph").unwrap();
    let entity = layer.intern("urn:external-only-subject").unwrap();
    let attribute = layer.intern("urn:external-only-predicate").unwrap();
    layer
        .transact_to_graph(
            &[Datum {
                entity,
                attribute,
                value: Value::Str("foreign fact".into()),
                valid_from: "2026-01-01T00:00:00Z".into(),
                valid_to: None,
                op: crate::Op::Assert,
            }],
            "2026-01-01T00:00:00Z",
            None,
            None,
            graph,
        )
        .unwrap();
    drop(layer);
    let pack = temp.path().join("layer.db");
    crate::store::respace::respace_file(&foreign, &pack, 1).unwrap();
    let attached = Store::open_with_attachments(
        local.to_str().unwrap(),
        &[crate::store::attach::Attachment::read_only(
            "external_layer",
            pack.to_str().unwrap(),
        )],
    )
    .unwrap();
    let crate::sparql::QueryResult::Select { rows, .. } = crate::sparql::query(
        &attached, "SELECT ?value WHERE { GRAPH <urn:external-only-graph> { <urn:external-only-subject> <urn:external-only-predicate> ?value } }",
    ).unwrap() else { panic!("expected SELECT") };
    assert_eq!(rows.len(), 1, "actual attached layer not served");
    let in_main = attached
        .conn
        .prepare("SELECT 1 FROM main.terms WHERE iri=?1")
        .unwrap()
        .exists(["urn:external-only-graph"])
        .unwrap();
    assert!(
        !in_main,
        "fixture accidentally gave main a graph dictionary entry"
    );
    drop(attached);
    let inspect = Store::open(local.to_str().unwrap()).unwrap();
    assert!(inspect.lookup("urn:external-only-graph").unwrap().is_none());
    let before: i64 = inspect
        .conn
        .query_row("SELECT COUNT(*) FROM terms", [], |r| r.get(0))
        .unwrap();
    let result = import_graph(&local, &foreign, "urn:external-only-graph");
    assert!(
        result.is_err(),
        "unresolved attached graph imported: {result:?}"
    );
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("unmounted attached graph registry")
    );
    let after: i64 = inspect
        .conn
        .query_row("SELECT COUNT(*) FROM terms", [], |r| r.get(0))
        .unwrap();
    assert_eq!(after, before);
}
