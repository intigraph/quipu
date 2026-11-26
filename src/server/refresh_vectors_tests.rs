//! `/embed_refresh` controls (aegis-xc7 spec C1-C5): re-embed only stale
//! existing vectors, dry-run by default, bounded and resumable by cursor.
use super::super::graph_backfill::refresh_vectors::{MAX_EXAMINED, Selection, refresh_embeddings};
use super::*;

const OLD: &str = "2020-01-01T00:00:00Z";
const STALE: &str = "koror";

/// `stale` carries two same-transaction comments but a vector built from its
/// label alone (the pre-#1280 shape); `fresh` has a vector built from its
/// current text; `bare` has no vector at all.
fn fixture() -> (SharedStore, Arc<parking_lot::Mutex<Vec<usize>>>, [i64; 3]) {
    let batches = Arc::new(parking_lot::Mutex::new(vec![]));
    let mut store = Store::open_in_memory().unwrap();
    store.embedding_config_mut().auto_embed = false;
    store.embedding_config_mut().dimension = 8;
    store.set_embedding_provider(Arc::new(RecordingProvider {
        batches: batches.clone(),
    }));
    let label = "<http://www.w3.org/2000/01/rdf-schema#label>";
    let comment = "<http://www.w3.org/2000/01/rdf-schema#comment>";
    let nt = format!(
        "<http://example.org/stale> {label} \"koror\" .\n\
         <http://example.org/stale> {comment} \"hosts CT 205\" .\n\
         <http://example.org/stale> {comment} \"automation container\" .\n\
         <http://example.org/fresh> {label} \"fresh entity\" .\n\
         <http://example.org/bare> {label} \"bare entity\" .\n"
    );
    quipu::ingest_rdf(
        &mut store,
        nt.as_bytes(),
        oxrdfio::RdfFormat::NTriples,
        None,
        "2026-01-01",
        None,
        None,
    )
    .unwrap();
    let id = |s: &Store, n: &str| {
        s.lookup(&format!("http://example.org/{n}"))
            .unwrap()
            .unwrap()
    };
    let ids = [id(&store, "stale"), id(&store, "fresh"), id(&store, "bare")];
    let fresh_text = quipu::build_entity_text(&store, ids[1]).unwrap();
    {
        let vs = store.vector_store();
        vs.embed_entity(ids[0], STALE, &[0.1; 8], OLD).unwrap();
        vs.embed_entity(ids[1], &fresh_text, &[0.1; 8], OLD)
            .unwrap();
    }
    let shared: SharedStore = Arc::new(super::super::StoreHandle::writer_only(store));
    (shared, batches, ids)
}

fn current_text(shared: &SharedStore, eid: i64) -> Option<String> {
    shared
        .lock()
        .vector_store()
        .current_embedding_text(eid)
        .unwrap()
}

#[test]
fn refresh_reembeds_only_stale_vectors_and_closes_the_old_row() {
    let (shared, batches, [stale, fresh, bare]) = fixture();
    let sel = Selection::Entities(vec![stale, fresh, bare]);
    let out = refresh_embeddings(&shared, &sel, 10, false).unwrap();
    // C1 / C5: a mutant without the text-differs check would report 2 here.
    assert_eq!(out.examined, 3);
    assert_eq!(out.changed, 1, "{out:?}");
    assert_eq!(out.unchanged, 1);
    assert_eq!(out.missing_skipped, 1, "backfill owns missing vectors");
    assert_eq!(*batches.lock(), vec![1], "exactly one entity embedded");
    let text = current_text(&shared, stale).unwrap();
    assert!(text.contains("hosts CT 205") && text.contains("automation container"));
    assert_eq!(current_text(&shared, bare), None);

    let s = shared.lock();
    let vs = s.vector_store();
    // The superseded row is retained, valid at its old time ...
    let then = vs
        .vector_search(&[0.1; 8], 10, Some("2021-01-01T00:00:00Z"))
        .unwrap();
    assert!(then.iter().any(|m| m.entity_id == stale && m.text == STALE));
    // ... and the replacement is stamped ISO-8601, comparable to fact cutoffs.
    let now = vs.vector_search(&[0.1; 8], 10, None).unwrap();
    let new = now.iter().find(|m| m.entity_id == stale).unwrap();
    assert_eq!(new.text, text);
    assert!(
        new.valid_from.as_str() > OLD && new.valid_from.contains('T'),
        "{new:?}"
    );
}

#[test]
fn refresh_dry_run_writes_nothing() {
    // C2
    let (shared, batches, [stale, fresh, _]) = fixture();
    let out = refresh_embeddings(&shared, &Selection::After(0), 10, true).unwrap();
    assert_eq!((out.examined, out.changed, out.unchanged), (2, 1, 1));
    assert!(batches.lock().is_empty(), "dry run must not embed");
    assert_eq!(current_text(&shared, stale).as_deref(), Some(STALE));
    assert!(current_text(&shared, fresh).is_some());
}

#[test]
fn refresh_cursor_resumes_without_repeat() {
    // C3: the cursor walks entities holding a current vector.
    let (shared, _, [stale, fresh, _]) = fixture();
    let first = refresh_embeddings(&shared, &Selection::After(0), 1, true).unwrap();
    assert_eq!(first.examined, 1);
    let cursor = first.next_cursor.expect("more entities remain");
    let second = refresh_embeddings(&shared, &Selection::After(cursor), 1, true).unwrap();
    assert_eq!(second.examined, 1);
    assert_eq!(second.next_cursor, None, "selection exhausted");
    assert_eq!(first.changed + second.changed, 1);
    assert_eq!(first.unchanged + second.unchanged, 1);
    assert_eq!(cursor, stale.min(fresh));
}

#[test]
fn refresh_cap_is_honoured() {
    // C4: an oversized max is clamped; an oversized explicit list is refused
    // rather than silently truncated.
    let (shared, _, [stale, ..]) = fixture();
    let out = refresh_embeddings(&shared, &Selection::After(0), 100_000, true).unwrap();
    assert!(out.examined <= MAX_EXAMINED);
    let too_many = Selection::Entities(vec![stale; MAX_EXAMINED + 1]);
    assert!(refresh_embeddings(&shared, &too_many, 100_000, true).is_err());
}
