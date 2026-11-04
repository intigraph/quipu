use super::*;
use crate::share_import::{PromoteImportRequest, promote_import};
use crate::types::Value;

const TS: &str = "2026-09-24T00:00:00Z";
const SHARE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn setup() -> (Store, i64, i64, i64, PromoteImportRequest) {
    let mut store = Store::open_in_memory().unwrap();
    let graph = store
        .graph_create(&format!("urn:quipu:import:staging:{}", &SHARE[7..]))
        .unwrap();
    let e = store.intern("https://example.org/entity").unwrap();
    let a = store.intern("https://example.org/value").unwrap();
    let datums = ["old", "different"].map(|v| Datum {
        entity: e,
        attribute: a,
        value: Value::Str(v.into()),
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    });
    store
        .transact_to_graph(&datums, TS, None, Some("pack"), graph)
        .unwrap();
    (
        store,
        graph,
        e,
        a,
        PromoteImportRequest {
            share_id: SHARE.into(),
            actor: None,
            target_graph: None,
            retire_from: None,
            dry_run: false,
        },
    )
}

#[test]
fn stale_promotion_preserves_retraction_and_source_membership() {
    let (mut store, graph, e, a, request) = setup();
    promote_import(&mut store, &request, TS, None).unwrap();
    store
        .retract_triples(
            e,
            Some(a),
            Some(&Value::Str("old".into())),
            TS,
            None,
            true,
            None,
        )
        .unwrap();
    for _ in 0..2 {
        let result = promote_import(&mut store, &request, TS, None).unwrap();
        assert_eq!(result.suppressed_retractions, 1);
        let root = store.current_facts().unwrap();
        assert_eq!(root.len(), 1);
        assert_eq!(root[0].value, Value::Str("different".into()));
        assert_eq!(store.current_facts_in_graph(graph).unwrap().len(), 2);
    }
    // A local, explicit correction can restore a fact; pack reload cannot.
    store
        .transact(
            &[Datum {
                entity: e,
                attribute: a,
                value: Value::Str("old".into()),
                valid_from: TS.into(),
                valid_to: None,
                op: Op::Assert,
            }],
            TS,
            None,
            Some("local correction"),
        )
        .unwrap();
    let result = promote_import(&mut store, &request, TS, None).unwrap();
    assert_eq!(result.suppressed_retractions, 0);
    assert_eq!(store.current_facts().unwrap().len(), 2);
}

#[test]
fn another_graphs_retraction_does_not_hide_root_content() {
    let (mut store, _, e, a, request) = setup();
    let other = store.graph_create("urn:other").unwrap();
    let mut datum = Datum {
        entity: e,
        attribute: a,
        value: Value::Str("old".into()),
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    };
    store
        .transact_to_graph(&[datum.clone()], TS, None, None, other)
        .unwrap();
    datum.op = Op::Retract;
    store
        .transact_to_graph(&[datum], TS, None, None, other)
        .unwrap();
    let result = promote_import(&mut store, &request, TS, None).unwrap();
    assert_eq!(result.suppressed_retractions, 0);
    assert_eq!(store.current_facts().unwrap().len(), 2);
}

// aegis-27l / tg5 F5 counterexample: board B = {A, W}, inbox I = {U}.
// Moving the share holding A must give B = {W} and I = {A, U}.
const BOARD: &str = "https://tracker.example/board";
const INBOX: &str = "https://tracker.example/board/inbox";

fn subject(store: &mut Store, id: &str) -> i64 {
    store
        .intern(&format!("https://seeds.local/item/{id}"))
        .unwrap()
}

fn fact(store: &mut Store, entity: i64, v: &str) -> Datum {
    let a = store.intern("https://example.org/title").unwrap();
    Datum {
        entity,
        attribute: a,
        value: Value::Str(v.into()),
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    }
}

/// Store with B={A,W}, I={U} and a committed staging share holding A.
fn move_setup() -> (Store, PromoteImportRequest, [i64; 3]) {
    let mut store = Store::open_in_memory().unwrap();
    let (a, w, u) = (
        subject(&mut store, "aegis-a"),
        subject(&mut store, "aegis-work"),
        subject(&mut store, "messages-b"),
    );
    let board = store.graph_create(BOARD).unwrap();
    let inbox = store.graph_create(INBOX).unwrap();
    let staging = store
        .graph_create(&format!("urn:quipu:import:staging:{}", &SHARE[7..]))
        .unwrap();
    let (fa, fw, fu) = (
        fact(&mut store, a, "A"),
        fact(&mut store, w, "W"),
        fact(&mut store, u, "U"),
    );
    store
        .transact_to_graph(&[fa.clone(), fw], TS, None, Some("board"), board)
        .unwrap();
    store
        .transact_to_graph(&[fu], TS, None, Some("inbox"), inbox)
        .unwrap();
    store
        .transact_to_graph(&[fa], TS, None, Some("pack"), staging)
        .unwrap();
    let request = PromoteImportRequest {
        share_id: SHARE.into(),
        actor: None,
        target_graph: Some(INBOX.into()),
        retire_from: Some(BOARD.into()),
        dry_run: false,
    };
    (store, request, [a, w, u])
}

fn subjects_in(store: &Store, iri: &str) -> Vec<i64> {
    let g = store.lookup(iri).unwrap().unwrap();
    let mut s: Vec<i64> = store
        .current_facts_in_graph(g)
        .unwrap()
        .iter()
        .map(|f| f.entity)
        .collect();
    s.sort_unstable();
    s.dedup();
    s
}

fn sorted(mut v: Vec<i64>) -> Vec<i64> {
    v.sort_unstable();
    v
}

#[test]
fn subject_move_retires_only_the_moved_subjects() {
    let (mut store, request, [a, w, u]) = move_setup();
    let result = promote_import(&mut store, &request, TS, None).unwrap();
    assert_eq!(result.outcome, "promoted");
    assert_eq!(result.target_graph.as_deref(), Some(INBOX));
    assert_eq!(result.retired, Some(1));
    assert_eq!(subjects_in(&store, BOARD), vec![w]);
    assert_eq!(subjects_in(&store, INBOX), sorted(vec![a, u]));
    // ROOT is untouched by a target promotion.
    assert!(store.current_facts().unwrap().is_empty());
}

#[test]
fn control_whole_graph_promote_drops_the_unrelated_subject() {
    // The pre-27l named-graph promote reproduces the reported wrong result:
    // I loses U, and B still holds A.
    let (mut store, _request, [a, w, u]) = move_setup();
    let staging = format!("urn:quipu:import:staging:{}", &SHARE[7..]);
    crate::project_graph::promote_into_graph(&mut store, SHARE, &staging, INBOX, TS, None).unwrap();
    assert_eq!(subjects_in(&store, INBOX), vec![a]);
    assert_eq!(subjects_in(&store, BOARD), sorted(vec![a, w]));
    let _ = u;
}

#[test]
fn subject_move_repeats_as_unchanged() {
    let (mut store, request, [_, w, _]) = move_setup();
    promote_import(&mut store, &request, TS, None).unwrap();
    let again = crate::share_import::promote_inner(&mut store, &request, TS, None).unwrap();
    assert_eq!(again.outcome, "unchanged");
    assert_eq!(again.tx_id, 0);
    assert_eq!(subjects_in(&store, BOARD), vec![w]);
}

#[test]
fn subject_move_refuses_moving_a_graph_into_itself_and_retire_without_target() {
    let (mut store, mut request, _) = move_setup();
    request.retire_from = Some(INBOX.into());
    assert!(promote_import(&mut store, &request, TS, None).is_err());
    request.target_graph = None;
    request.retire_from = Some(BOARD.into());
    assert!(promote_import(&mut store, &request, TS, None).is_err());
}

fn promoted_reviews(store: &Store) -> i64 {
    store
        .conn
        .query_row(
            "SELECT count(*) FROM import_reviews WHERE share_id=?1 AND state='promoted'",
            [SHARE],
            |r| r.get(0),
        )
        .unwrap()
}

fn expire_now() {
    // Leaked on purpose; the test's open-deadline guard restores on drop.
    std::mem::forget(crate::time::set_request_deadline(Some(
        crate::time::Deadline::after_millis(0),
    )));
    std::thread::sleep(std::time::Duration::from_millis(2));
}

// Expire at a boundary INSIDE the move, after a write. Nothing of the move
// or its review may survive: B = {A, W}, I = {U}, no promoted review.
fn assert_expiry_at_rolls_back(boundary: crate::share_import::test_hooks::Boundary) {
    let (mut store, request, [a, w, u]) = move_setup();
    crate::share_import::test_hooks::set(Some((boundary, expire_now)));
    let refused = {
        let _open =
            crate::time::set_request_deadline(Some(crate::time::Deadline::after_millis(60_000)));
        promote_import(&mut store, &request, TS, None)
    };
    crate::share_import::test_hooks::set(None);
    assert!(
        matches!(refused, Err(crate::Error::RequestTimeout { .. })),
        "{boundary:?}: {refused:?}"
    );
    assert_eq!(
        subjects_in(&store, BOARD),
        sorted(vec![a, w]),
        "{boundary:?}"
    );
    assert_eq!(subjects_in(&store, INBOX), vec![u], "{boundary:?}");
    assert_eq!(promoted_reviews(&store), 0, "{boundary:?}");
    // Positive control: the same request without expiry commits the move.
    promote_import(&mut store, &request, TS, None).unwrap();
    assert_eq!(subjects_in(&store, BOARD), vec![w]);
    assert_eq!(promoted_reviews(&store), 1);
}

#[test]
fn expiry_after_the_target_write_rolls_back_the_whole_move() {
    assert_expiry_at_rolls_back(crate::share_import::test_hooks::Boundary::TargetWritten);
}

#[test]
fn expiry_after_the_final_retirement_rolls_back_the_whole_move() {
    // Only the final check before the savepoint RELEASE catches this one.
    assert_expiry_at_rolls_back(crate::share_import::test_hooks::Boundary::Retired);
}

#[test]
fn already_expired_deadline_refuses_before_any_write() {
    let (mut store, request, [a, w, u]) = move_setup();
    let expired = crate::time::Deadline::after_millis(0);
    std::thread::sleep(std::time::Duration::from_millis(2));
    let _guard = crate::time::set_request_deadline(Some(expired));
    assert!(promote_import(&mut store, &request, TS, None).is_err());
    drop(_guard);
    assert_eq!(subjects_in(&store, BOARD), sorted(vec![a, w]));
    assert_eq!(subjects_in(&store, INBOX), vec![u]);
}

fn graph_facts(store: &Store, iri: &str) -> Vec<(i64, i64, Vec<u8>)> {
    let g = store.lookup(iri).unwrap().unwrap();
    let mut f: Vec<_> = store
        .current_facts_in_graph(g)
        .unwrap()
        .iter()
        .map(|f| (f.entity, f.attribute, f.value.to_bytes()))
        .collect();
    f.sort();
    f
}

#[test]
fn dry_run_predicts_the_move_and_writes_nothing() {
    let (mut store, mut request, [a, w, u]) = move_setup();
    let (board, inbox) = (graph_facts(&store, BOARD), graph_facts(&store, INBOX));
    request.dry_run = true;
    let preview = promote_import(&mut store, &request, TS, None).unwrap();
    assert_eq!(preview.outcome, "dry_run");
    assert_eq!((preview.replaced, preview.retired), (Some(0), Some(1)));
    assert_eq!(preview.warning, None);
    assert_eq!((preview.tx_id, preview.queries.is_none()), (0, true));
    assert_eq!(graph_facts(&store, BOARD), board);
    assert_eq!(graph_facts(&store, INBOX), inbox);
    assert_eq!(promoted_reviews(&store), 0);
    // The real move then does exactly what the preview said.
    request.dry_run = false;
    let moved = promote_import(&mut store, &request, TS, None).unwrap();
    assert_eq!(
        (moved.replaced, moved.retired, moved.triples),
        (preview.replaced, preview.retired, preview.triples)
    );
    assert_eq!(subjects_in(&store, BOARD), vec![w]);
    assert_eq!(subjects_in(&store, INBOX), sorted(vec![a, u]));
}

#[test]
fn dry_run_warns_when_the_move_would_retract_target_facts() {
    // The partial-attribute hazard: the target holds a fact of a moved subject
    // that the share does not carry, so the move would retract it.
    let (mut store, mut request, [a, _, _]) = move_setup();
    let extra = fact(&mut store, a, "A-only-in-target");
    let inbox = store.lookup(INBOX).unwrap().unwrap();
    store
        .transact_to_graph(&[extra], TS, None, Some("inbox"), inbox)
        .unwrap();
    let before = graph_facts(&store, INBOX);
    request.dry_run = true;
    let preview = promote_import(&mut store, &request, TS, None).unwrap();
    assert_eq!(preview.replaced, Some(1));
    let warning = preview.warning.expect("a retracting move must warn");
    assert!(warning.contains("would retract 1 facts"), "{warning}");
    assert_eq!(
        graph_facts(&store, INBOX),
        before,
        "a dry run writes nothing"
    );
    request.dry_run = false;
    let moved = promote_import(&mut store, &request, TS, None).unwrap();
    assert_eq!(moved.replaced, Some(1));
    assert_eq!(moved.warning, None);
    assert_ne!(graph_facts(&store, INBOX), before);
}

#[test]
fn dry_run_refuses_a_closed_review_like_the_real_move() {
    let (mut store, mut request, _) = move_setup();
    store
        .conn
        .execute(
            "INSERT INTO import_reviews(share_id,state,first_seen,updated_at,payload) \
             VALUES(?1,'rejected',?2,?2,'{}')",
            rusqlite::params![SHARE, TS],
        )
        .unwrap();
    request.dry_run = true;
    let preview = promote_import(&mut store, &request, TS, None).unwrap_err();
    request.dry_run = false;
    let real = promote_import(&mut store, &request, TS, None).unwrap_err();
    assert_eq!(preview.to_string(), real.to_string());
    assert!(
        preview.to_string().contains("import review is closed"),
        "{preview}"
    );
}

#[test]
fn dry_run_refuses_an_overlay_target_like_the_real_move() {
    let (mut store, mut request, _) = move_setup();
    let overlay = "https://tracker.example/overlay";
    store.overlay_create(overlay, 0).unwrap();
    request.target_graph = Some(overlay.into());
    request.retire_from = None;
    request.dry_run = true;
    let preview = promote_import(&mut store, &request, TS, None).unwrap_err();
    request.dry_run = false;
    let real = promote_import(&mut store, &request, TS, None).unwrap_err();
    assert_eq!(preview.to_string(), real.to_string());
    assert!(
        preview.to_string().contains("class is fixed at create"),
        "{preview}"
    );
}

#[test]
fn dry_run_never_creates_the_target_graph_and_refuses_root() {
    let (mut store, mut request, _) = move_setup();
    request.dry_run = true;
    request.retire_from = None;
    request.target_graph = Some("https://tracker.example/new".into());
    let preview = promote_import(&mut store, &request, TS, None).unwrap();
    assert_eq!(preview.outcome, "dry_run");
    assert!(
        store
            .lookup("https://tracker.example/new")
            .unwrap()
            .is_none()
    );
    request.target_graph = None;
    assert!(promote_import(&mut store, &request, TS, None).is_err());
}

/// aegis-gxfm: promotion closes target facts the staging copy lacks; that
/// removal passes the deletion gate like any retraction.
#[cfg(feature = "shacl")]
#[test]
fn subject_move_that_drops_a_required_value_is_refused() {
    let (mut store, mut request, [a, _, _]) = move_setup();
    request.retire_from = None;
    let inbox = store.lookup(INBOX).unwrap().unwrap();
    let staging = store
        .lookup(&format!("urn:quipu:import:staging:{}", &SHARE[7..]))
        .unwrap()
        .unwrap();
    let rdf_type = store.intern(crate::namespace::RDF_TYPE).unwrap();
    let item = store.intern("https://example.org/Item").unwrap();
    let owner = store.intern("https://example.org/owner").unwrap();
    let typed = Datum {
        entity: a,
        attribute: rdf_type,
        value: Value::Ref(item),
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    };
    let owned = Datum {
        attribute: owner,
        value: Value::Str("x".into()),
        ..typed.clone()
    };
    store
        .transact_to_graph(&[typed.clone(), owned], TS, None, Some("inbox"), inbox)
        .unwrap();
    store
        .transact_to_graph(&[typed], TS, None, Some("pack"), staging)
        .unwrap();
    store
        .load_shapes(
            "items",
            "@prefix sh: <http://www.w3.org/ns/shacl#> .\n\
             <urn:t:ItemShape> a sh:NodeShape ; sh:targetClass <https://example.org/Item> ;\n    \
             sh:property [ sh:path <https://example.org/owner> ; sh:minCount 1 ] .\n",
            TS,
        )
        .unwrap();
    store.shacl_config_mut().validate_on_write = true;
    let err = promote_import(&mut store, &request, TS, None).unwrap_err();
    assert!(
        err.to_string().contains("introduced by a deletion"),
        "{err}"
    );
    let owners = store
        .current_facts_in_graph(inbox)
        .unwrap()
        .iter()
        .filter(|f| f.attribute == owner)
        .count();
    assert_eq!(owners, 1, "nothing written");
    assert_eq!(promoted_reviews(&store), 0);
}
