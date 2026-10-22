use oxrdfio::RdfFormat;

use super::*;
use crate::share_import::{
    ImportResolution, ShareImportRequest, import_share, import_share_prepared,
};

const TS: &str = "2026-10-22T00:00:00Z";
const LABEL: &str = "http://www.w3.org/2000/01/rdf-schema#label";

fn store_with(labels: &[(&str, &str)]) -> Store {
    let mut store = Store::open_in_memory().unwrap();
    crate::share_scrub::seed_test_catalogue(&mut store);
    add_labels(&mut store, labels);
    store
}

fn add_labels(store: &mut Store, labels: &[(&str, &str)]) {
    let nt: String = labels
        .iter()
        .map(|(iri, label)| format!("<{iri}> <{LABEL}> \"{label}\" .\n"))
        .collect();
    crate::rdf::ingest_rdf(
        store,
        nt.as_bytes(),
        RdfFormat::NTriples,
        None,
        TS,
        None,
        Some("test"),
    )
    .unwrap();
}

fn request(source: &Store, accept_exact: bool) -> (tempfile::TempDir, ShareImportRequest) {
    let dir = tempfile::tempdir().unwrap();
    let share_dir = dir.path().join("share");
    crate::share::share(
        source,
        share_dir.to_str().unwrap(),
        &crate::share::ShareOptions {
            no_shapes: true,
            ..Default::default()
        },
    )
    .unwrap();
    let read = |name: &str| std::fs::read_to_string(share_dir.join(name)).unwrap();
    let request = ShareImportRequest {
        manifest: serde_json::from_str(&read("manifest.json")).unwrap(),
        export_ntriples: read("export.nt"),
        shapes_turtle: read("shapes.ttl"),
        alignments_turtle: None,
        queries_turtle: None,
        query_namespace: None,
        replace_queries: false,
        source: "https://example.org/sender/share".into(),
        actor: None,
        accept_exact,
        destination: Default::default(),
        #[cfg(not(target_arch = "wasm32"))]
        attestation: None,
    };
    (dir, request)
}

/// The report `resolve_and_rewrite` built before aegis-oa5: one
/// `resolve_entity` call per label, in label order. The oracle for equivalence.
fn per_label_report(store: &Store, triples: &[oxrdf::Triple]) -> ImportResolution {
    let mut report = ImportResolution::default();
    for (foreign, label) in crate::share_import::labels(triples) {
        let result = crate::resolve_entity(store, &label, &[], 0.85, 5).unwrap();
        let Some(top) = result.candidates.first() else {
            report.unmatched.push(foreign);
            continue;
        };
        if top.score == 1.0 && top.matched_on == "canonical_name:exact" {
            report.exact_merges.push(ImportMatch {
                foreign,
                local: top.iri.clone(),
                score: Some(top.score),
                matched_on: Some(top.matched_on.clone()),
            });
        } else {
            for c in result.candidates {
                report.candidates.push(ImportMatch {
                    foreign: foreign.clone(),
                    local: c.iri,
                    score: Some(c.score),
                    matched_on: Some(c.matched_on),
                });
            }
        }
    }
    report
}

fn json(report: &ImportResolution) -> serde_json::Value {
    serde_json::to_value(report).unwrap()
}

const LOCAL: &[(&str, &str)] = &[
    ("urn:local:alice", "Alice"),
    ("urn:local:alicia", "Alicia"),
    ("urn:local:alicea", "Alicea"),
    ("urn:local:bob", "Bob Builder"),
    ("urn:local:bobby", "Bob Builders"),
    ("urn:local:robert", "Robert"),
    ("urn:local:carol-1", "Carol"),
    ("urn:local:carol-2", "carol"),
];

const FOREIGN: &[(&str, &str)] = &[
    ("urn:foreign:alice", "Alice"),
    ("urn:foreign:alyce", "Alyce"),
    ("urn:foreign:bob", "Bob Builder"),
    ("urn:foreign:carol", "CAROL"),
    ("urn:foreign:opaque", "change-event-00196ae7426f4483"),
    ("urn:foreign:zed", "Zed"),
];

/// One pass over the label set reports exactly what one resolver call per label did:
/// exact, case-folded exact with a tie, fuzzy, and no match.
#[test]
fn one_pass_resolution_reports_what_the_per_label_resolver_did() {
    let store = store_with(LOCAL);
    let (_dir, request) = request(&store_with(FOREIGN), false);
    let triples = crate::share_import::parse_triples(&request.export_ntriples).unwrap();
    let expected = per_label_report(&store, &triples);
    let (actual, replacements) = finish(
        resolve(&store, &crate::share_import::labels(&triples)).unwrap(),
        false,
    );
    assert_eq!(json(&actual), json(&expected));
    assert!(replacements.is_empty(), "no rewrite without accept_exact");
    // Non-vacuous: every branch of the decision is exercised.
    assert_eq!(expected.exact_merges.len(), 3, "{expected:?}");
    assert!(!expected.candidates.is_empty(), "{expected:?}");
    assert!(!expected.unmatched.is_empty(), "{expected:?}");
}

/// The prepared path stages the same facts and reports the same resolution as the
/// single-phase path, with and without `accept_exact`.
#[test]
fn a_prepared_import_stages_what_the_single_phase_import_staged() {
    for accept_exact in [false, true] {
        let (_dir, request) = request(&store_with(FOREIGN), accept_exact);
        let mut single = store_with(LOCAL);
        let a = import_share(&mut single, &request, TS, None).unwrap();
        let mut two = store_with(LOCAL);
        let prepared = prepare(&two, &request).unwrap();
        let b = import_share_prepared(&mut two, &request, TS, None, Some(prepared)).unwrap();
        assert_eq!(json(&a.resolution), json(&b.resolution));
        assert_eq!(a.staging_graph, b.staging_graph);
        assert_eq!(
            staged(&single, &a.staging_graph),
            staged(&two, &b.staging_graph)
        );
        assert_eq!(a.resolution.exact_merges.len(), 3);
    }
}

fn staged(store: &Store, graph: &str) -> Vec<String> {
    let g = store.lookup(graph).unwrap().expect("staging graph exists");
    let mut stmt = store
        .conn
        .prepare("SELECT e, a, v FROM facts WHERE g = ?1 AND op = 1 AND valid_to IS NULL")
        .unwrap();
    let rows: Vec<(i64, i64, Vec<u8>)> = stmt
        .query_map([g], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<std::result::Result<_, _>>()
        .unwrap();
    let mut out: Vec<String> = rows
        .into_iter()
        .map(|(e, a, v)| {
            let v = match crate::types::Value::from_bytes(&v).unwrap() {
                crate::types::Value::Ref(id) => store.resolve(id).unwrap(),
                other => format!("{other:?}"),
            };
            format!(
                "{} {} {v}",
                store.resolve(e).unwrap(),
                store.resolve(a).unwrap()
            )
        })
        .collect();
    out.sort();
    assert!(!out.is_empty(), "nothing staged in {graph}");
    out
}

/// A local label RETRACTED between the phases must not drive an `accept_exact`
/// rewrite. The rewrite is destructive: the foreign IRI would be gone.
#[test]
fn an_exact_match_retracted_between_the_phases_is_not_merged() {
    let mut target = store_with(&[("urn:local:alice", "Alice")]);
    let (_dir, request) = request(&store_with(&[("urn:foreign:alice", "Alice")]), true);
    let prepared = prepare(&target, &request).unwrap();
    assert_eq!(prepared.labels[0].candidates[0].iri, "urn:local:alice");

    let entity = target.lookup("urn:local:alice").unwrap().unwrap();
    let label = target.lookup(LABEL).unwrap().unwrap();
    let (_, retracted) = target
        .retract_triples(entity, Some(label), None, TS, None, true, None)
        .unwrap();
    assert_eq!(retracted, 1);

    let result = import_share_prepared(&mut target, &request, TS, None, Some(prepared)).unwrap();
    // The dropped candidate is REPORTED, not silently lost (wu's condition).
    let dropped = &result.resolution.revalidation_dropped;
    assert_eq!(dropped.len(), 1, "{:?}", result.resolution);
    assert_eq!(dropped[0].foreign, "urn:foreign:alice");
    assert_eq!(dropped[0].local, "urn:local:alice");
    assert_eq!(
        dropped[0].matched_on.as_deref(),
        Some("canonical_name:exact")
    );
    let wire = serde_json::to_value(&result).unwrap();
    assert_eq!(
        wire["resolution"]["revalidation_dropped"][0]["local"],
        "urn:local:alice"
    );
    assert!(
        result.resolution.exact_merges.is_empty(),
        "{:?}",
        result.resolution
    );
    assert_eq!(result.resolution.unmatched, ["urn:foreign:alice"]);
    let staged = staged(&target, &result.staging_graph).join("\n");
    assert!(staged.contains("urn:foreign:alice"), "{staged}");
}

/// A local label ASSERTED between the phases is matched, and with `accept_exact`
/// it is the rewrite target, exactly as if it had existed at the read.
#[test]
fn an_exact_match_asserted_between_the_phases_is_merged() {
    let mut target = store_with(&[("urn:local:other", "Somebody Else")]);
    let (_dir, request) = request(&store_with(&[("urn:foreign:alice", "Alice")]), true);
    let prepared = prepare(&target, &request).unwrap();
    assert!(prepared.labels[0].candidates.is_empty());

    add_labels(&mut target, &[("urn:local:alice", "Alice")]);

    let result = import_share_prepared(&mut target, &request, TS, None, Some(prepared)).unwrap();
    assert_eq!(
        result.resolution.exact_merges.len(),
        1,
        "{:?}",
        result.resolution
    );
    assert_eq!(result.resolution.exact_merges[0].local, "urn:local:alice");
    let staged = staged(&target, &result.staging_graph).join("\n");
    assert!(!staged.contains("urn:foreign:alice"), "{staged}");
}

/// A resolution prepared for one share is never applied to another.
#[test]
fn a_resolution_prepared_for_another_share_is_not_used() {
    let mut target = store_with(&[("urn:local:alice", "Alice")]);
    let (_d1, other) = request(&store_with(&[("urn:foreign:x", "Alice")]), true);
    let (_d2, request) = request(&store_with(&[("urn:foreign:alice", "Zed")]), true);
    let wrong = prepare(&target, &other).unwrap();
    let result = import_share_prepared(&mut target, &request, TS, None, Some(wrong)).unwrap();
    assert!(
        result.resolution.exact_merges.is_empty(),
        "{:?}",
        result.resolution
    );
    assert_eq!(result.resolution.unmatched, ["urn:foreign:alice"]);
}

/// Past the request deadline the import refuses, and nothing is staged or
/// recorded: the writer is not held to finish work nobody is waiting for.
#[test]
fn an_import_past_its_deadline_rolls_back() {
    let mut target = store_with(LOCAL);
    let (_dir, request) = request(&store_with(FOREIGN), false);
    let before: i64 = target
        .conn
        .query_row("SELECT COUNT(*) FROM facts", [], |r| r.get(0))
        .unwrap();
    let error = {
        let _deadline =
            crate::time::set_request_deadline(Some(crate::time::Deadline::after_millis(0)));
        std::thread::sleep(std::time::Duration::from_millis(2));
        import_share(&mut target, &request, TS, None).unwrap_err()
    };
    assert!(
        matches!(error, crate::Error::RequestTimeout { .. }),
        "{error:?}"
    );
    let reviews: i64 = target
        .conn
        .query_row("SELECT COUNT(*) FROM import_reviews", [], |r| r.get(0))
        .unwrap();
    assert_eq!(reviews, 0, "a refused import must leave no review record");
    let facts: i64 = target
        .conn
        .query_row("SELECT COUNT(*) FROM facts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(facts, before, "a refused import must write no facts");
    // Positive control: the same import with no deadline stages, so the refusal
    // above was the deadline and not an import that could never succeed.
    let ok = import_share(&mut target, &request, TS, None).unwrap();
    assert!(!staged(&target, &ok.staging_graph).is_empty());
    let after: i64 = target
        .conn
        .query_row("SELECT COUNT(*) FROM facts", [], |r| r.get(0))
        .unwrap();
    assert!(
        after > facts,
        "the control must write what the refusal did not"
    );
}

fn plan(store: &Store, sql: &str) -> String {
    let mut stmt = store
        .conn
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .unwrap();
    stmt.query_map([], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap()
        .join(" | ")
}

/// Both delta reads under the writer are bounded by the delta, not the label set.
/// With no planner statistics, `a = ?` drew SQLite onto the label index: on a
/// production-size copy that was 12.5 s and 49.8 s, under the writer, for a
/// delta of zero rows (aegis-oa5).
#[test]
fn the_delta_reads_under_the_writer_use_the_transaction_indexes() {
    let store = store_with(LOCAL);
    let label = store.lookup(LABEL).unwrap().unwrap();
    let asserted = plan(&store, &crate::resolution::delta_sql(1, label));
    assert!(asserted.contains("USING INDEX idx_tx"), "{asserted}");
    let retracted = RETRACTED_LABELS_SQL
        .replace("?1", "1")
        .replace("?2", &label.to_string());
    let retracted = plan(&store, &retracted);
    assert!(
        retracted.contains("USING INDEX idx_retracted_tx"),
        "{retracted}"
    );
}

fn expire_now() {
    // Leaked on purpose: the guard would restore the open deadline on drop. The
    // test's own guard restores "no deadline" when it drops.
    std::mem::forget(crate::time::set_request_deadline(Some(
        crate::time::Deadline::after_millis(0),
    )));
    std::thread::sleep(std::time::Duration::from_millis(2));
}

fn count(store: &Store, table: &str) -> i64 {
    store
        .conn
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

/// Run `work` under an open deadline that expires when it reaches `boundary`.
fn expiring_at<T>(
    boundary: crate::share_import::test_hooks::Boundary,
    work: impl FnOnce() -> crate::Result<T>,
) -> crate::Result<T> {
    crate::share_import::test_hooks::set(Some((boundary, expire_now)));
    let outcome = {
        let _open =
            crate::time::set_request_deadline(Some(crate::time::Deadline::after_millis(60_000)));
        work()
    };
    crate::share_import::test_hooks::set(None);
    outcome
}

/// wu's ruling on #736: past the bound, an explicit request timeout, a WHOLE
/// rollback and the writer released. A deadline that expires at any phase
/// boundary leaves zero new facts and zero new review rows. Before this, one
/// that expired while staging still committed: facts 4 -> 5, reviews 0 -> 1
/// (grant's review).
#[test]
fn a_deadline_expiring_at_any_import_boundary_rolls_back_whole() {
    use crate::share_import::test_hooks::Boundary;
    let (_dir, request) = request(&store_with(FOREIGN), false);
    for boundary in [Boundary::Resolved, Boundary::Staged] {
        let mut target = store_with(LOCAL);
        let (facts, reviews) = (count(&target, "facts"), count(&target, "import_reviews"));
        assert_eq!(reviews, 0);
        let error =
            expiring_at(boundary, || import_share(&mut target, &request, TS, None)).unwrap_err();
        assert!(
            matches!(error, crate::Error::RequestTimeout { .. }),
            "{boundary:?}: {error:?}"
        );
        assert_eq!(
            count(&target, "facts"),
            facts,
            "{boundary:?}: zero new facts"
        );
        assert_eq!(
            count(&target, "import_reviews"),
            0,
            "{boundary:?}: zero review rows"
        );
        // Positive control on the same store: the writer was released, nothing
        // lingers, and the same import then stages.
        let ok = import_share(&mut target, &request, TS, None).unwrap();
        assert!(
            !staged(&target, &ok.staging_graph).is_empty(),
            "{boundary:?}"
        );
        assert_eq!(count(&target, "import_reviews"), 1, "{boundary:?}");
    }
}

/// Phase 0 runs on a reader before any writer: an expired deadline there is a
/// request timeout with nothing written anywhere.
#[test]
fn a_deadline_expiring_before_the_read_phase_refuses_it() {
    let store = store_with(LOCAL);
    let (_dir, request) = request(&store_with(FOREIGN), false);
    let facts = count(&store, "facts");
    let error = {
        let _expired =
            crate::time::set_request_deadline(Some(crate::time::Deadline::after_millis(0)));
        std::thread::sleep(std::time::Duration::from_millis(2));
        prepare(&store, &request).unwrap_err()
    };
    assert!(
        matches!(error, crate::Error::RequestTimeout { .. }),
        "{error:?}"
    );
    assert_eq!(count(&store, "facts"), facts);
}

/// A promotion that expires before its savepoint is released leaves ROOT and
/// the review state exactly as staging left them.
#[test]
fn a_deadline_expiring_at_promotion_rolls_back_whole() {
    use crate::share_import::test_hooks::Boundary;
    let (_dir, request) = request(&store_with(FOREIGN), false);
    let mut target = store_with(LOCAL);
    let staged_result = import_share(&mut target, &request, TS, None).unwrap();
    assert!(
        staged_result.promotion.eligible,
        "{:?}",
        staged_result.promotion
    );
    let state = |store: &Store| -> String {
        store
            .conn
            .query_row(
                "SELECT state FROM import_reviews WHERE share_id = ?1",
                [&request.manifest.share_id],
                |r| r.get(0),
            )
            .unwrap()
    };
    let (facts, reviews) = (count(&target, "facts"), count(&target, "import_reviews"));
    assert_eq!(state(&target), "staged");
    let promote = crate::share_import::PromoteImportRequest {
        share_id: request.manifest.share_id.clone(),
        actor: None,
    };
    let error = expiring_at(Boundary::Promoted, || {
        crate::share_import::promote_import(&mut target, &promote, TS, None)
    })
    .unwrap_err();
    assert!(
        matches!(error, crate::Error::RequestTimeout { .. }),
        "{error:?}"
    );
    assert_eq!(count(&target, "facts"), facts, "zero new facts in ROOT");
    assert_eq!(count(&target, "import_reviews"), reviews);
    assert_eq!(state(&target), "staged", "the promotion did not land");
    // Positive control: the same promotion with no expiry lands.
    let promoted = crate::share_import::promote_import(&mut target, &promote, TS, None).unwrap();
    assert!(promoted.triples > 0);
    assert!(count(&target, "facts") > facts);
    assert_eq!(state(&target), "promoted");
}
