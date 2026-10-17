use super::*;
use crate::namespace::{META_GRAPH_IRI, QUIPU_INCLUDES_GRAPH};
use crate::share::{ShareDestination, ShareOptions};
use crate::types::Op;

const T1: &str = "2026-10-15T01:00:00Z";
const T2: &str = "2026-10-15T02:00:00Z";
const T3: &str = "2026-10-15T03:00:00Z";
const T4: &str = "2026-10-15T04:00:00Z";
const T5: &str = "2026-10-15T05:00:00Z";
const REPO: &str = "scbrown/demo";
const SHAPES: &str = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
<urn:PersonShape> a sh:NodeShape; sh:targetClass <urn:Person>;
 sh:property [ sh:path <urn:name>; sh:minCount 1; sh:maxCount 1 ] .
"#;

fn pack(data: &str, shapes: &str) -> ShareImportRequest {
    let mut source = Store::open_in_memory().unwrap();
    crate::rdf::ingest_rdf(
        &mut source,
        data.as_bytes(),
        RdfFormat::Turtle,
        None,
        T1,
        None,
        None,
    )
    .unwrap();
    source.load_shapes("test", shapes, T1).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pack");
    crate::share::share(
        &source,
        path.to_str().unwrap(),
        &ShareOptions {
            destination: ShareDestination::Internal,
            ..Default::default()
        },
    )
    .unwrap();
    let mut request = crate::share_transport::read_local(path.to_str().unwrap()).unwrap();
    request.destination = ShareDestination::Internal;
    request
}

fn keys(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| (*n).to_string()).collect()
}

fn commit(store: &mut Store, repo: &str, data: &str, ts: &str) -> RepoComposition {
    compose_current(
        store,
        repo,
        &keys(&["packs/main"]),
        &[pack(data, "")],
        None,
        ts,
        None,
    )
    .unwrap()
}

/// Values of `<urn:a> <p> ?o` visible through a dataset, sorted.
fn read(store: &Store, dataset: &str, p: &str) -> Vec<String> {
    let query = format!("SELECT ?o FROM <{dataset}> WHERE {{ <urn:a> <{p}> ?o }}");
    let rows = crate::sparql_query(store, &query).unwrap();
    let mut out: Vec<String> = rows
        .rows()
        .iter()
        .map(|row| format!("{:?}", row.values().next().unwrap()))
        .collect();
    out.sort();
    out
}

fn has(store: &Store, dataset: &str, p: &str, needle: &str) -> bool {
    read(store, dataset, p).iter().any(|v| v.contains(needle))
}

/// A local retraction, made the way any client makes one: in the graph itself.
fn retract(store: &mut Store, graph: &str, subject: &str, predicate: &str, object: &str) {
    let g = store.lookup(graph).unwrap().unwrap();
    let datum = crate::store::Datum {
        entity: store.intern(subject).unwrap(),
        attribute: store.intern(predicate).unwrap(),
        value: Value::Str(object.into()),
        valid_from: T1.into(),
        valid_to: None,
        op: Op::Retract,
    };
    store
        .transact_to_graph(&[datum], T1, Some("local"), None, g)
        .unwrap();
}

use crate::types::Value;

#[test]
fn two_commits_move_current_and_the_previous_stays_queryable() {
    let mut store = Store::open_in_memory().unwrap();
    let first = commit(&mut store, REPO, "<urn:a> <urn:p> \"v1\" .", T1);
    assert!(first.moved && first.previous.is_none());
    let second = commit(&mut store, REPO, "<urn:a> <urn:p> \"v2\" .", T2);
    assert!(second.moved);
    let (current, previous) = (second.current.unwrap(), second.previous.unwrap());
    assert_eq!(previous.snapshot, first.current.unwrap().snapshot);
    assert_ne!(current.snapshot, previous.snapshot);
    assert!(has(&store, &current_dataset(REPO), "urn:p", "v2"));
    assert!(!has(&store, &current_dataset(REPO), "urn:p", "v1"));
    assert!(
        has(&store, &previous_dataset(REPO), "urn:p", "v1"),
        "rollback target stays queryable"
    );
    assert!(
        store.current_facts().unwrap().is_empty(),
        "never promotes ROOT"
    );
}

#[test]
fn a_local_retraction_survives_commits_absence_reappearance_and_pruning() {
    let mut store = Store::open_in_memory().unwrap();
    let v1 = commit(
        &mut store,
        REPO,
        "<urn:a> <urn:p> \"keep\" ; <urn:q> \"secret\" .",
        T1,
    );
    let g1 = v1.current.unwrap().members[0].graph.clone();
    retract(&mut store, &g1, "urn:a", "urn:q", "secret");

    let v2 = commit(
        &mut store,
        REPO,
        "<urn:a> <urn:p> \"keep\" ; <urn:q> \"secret\" ; <urn:r> \"new\" .",
        T2,
    );
    assert_eq!(v2.tombstones_harvested, 1);
    assert!(has(&store, &current_dataset(REPO), "urn:r", "new"));
    assert!(
        !has(&store, &current_dataset(REPO), "urn:q", "secret"),
        "carried forward"
    );

    // A version without the statement: v1's graph leaves both pointers and is pruned.
    let v3 = commit(&mut store, REPO, "<urn:a> <urn:p> \"keep\" .", T3);
    assert!(
        v3.dropped.contains(&g1),
        "retention pruned the graph the retraction was made in: {:?}",
        v3.dropped
    );
    // ...then one where the statement comes back.
    commit(
        &mut store,
        REPO,
        "<urn:a> <urn:p> \"keep\" ; <urn:q> \"secret\" .",
        T4,
    );
    assert!(
        !has(&store, &current_dataset(REPO), "urn:q", "secret"),
        "reappearance"
    );
    assert_eq!(
        tombstone_counts(&store, REPO).unwrap()["packs/main"],
        1,
        "survives pruning"
    );
    assert!(
        store
            .lookup(&g1)
            .unwrap()
            .is_none_or(|g| store.graph_class(g).unwrap().is_none()),
        "pruned graph is unregistered"
    );
}

#[test]
fn explicit_revocation_survives_recomposition_and_rollback_then_allows_new_retraction() {
    let mut store = Store::open_in_memory().unwrap();
    let first = commit(
        &mut store,
        REPO,
        "<urn:a> <urn:p> \"v1\" ; <urn:q> \"secret\" .",
        T1,
    );
    retract(
        &mut store,
        &first.current.unwrap().members[0].graph,
        "urn:a",
        "urn:q",
        "secret",
    );
    let second_pack = pack("<urn:a> <urn:p> \"v2\" ; <urn:q> \"secret\" .", "");
    let second = compose_current(
        &mut store,
        REPO,
        &keys(&["packs/main"]),
        std::slice::from_ref(&second_pack),
        None,
        T2,
        None,
    )
    .unwrap();
    let before = second.current.unwrap().snapshot;
    // Negative control: the old manual repair does not revoke the policy.
    store
        .conn
        .execute(
            "DELETE FROM pendant_repo_tombstones WHERE repo = ?1",
            params![REPO],
        )
        .unwrap();
    compose_current(
        &mut store,
        REPO,
        &keys(&["packs/main"]),
        std::slice::from_ref(&second_pack),
        None,
        T3,
        None,
    )
    .unwrap();
    assert_eq!(
        tombstone_counts(&store, REPO).unwrap()["packs/main"],
        1,
        "retained retractions recreate a manually deleted row"
    );
    assert!(!has(&store, &current_dataset(REPO), "urn:q", "secret"));
    let receipt = revoke_tombstone(
        &mut store,
        REPO,
        "packs/main",
        "<urn:a> <urn:q> \"secret\" .",
        "undo mistaken local suppression",
        T3,
        Some("test"),
    )
    .unwrap();
    assert!(receipt.revoked);
    assert_eq!(receipt.graphs_restored, 1);
    assert_eq!(
        read_pointer(&store, REPO, "current")
            .unwrap()
            .unwrap()
            .snapshot,
        before
    );
    assert!(
        !has(&store, &current_dataset(REPO), "urn:q", "secret"),
        "the newer graph never stored this suppressed triple"
    );
    assert!(has(&store, &previous_dataset(REPO), "urn:q", "secret"));
    assert!(tombstone_counts(&store, REPO).unwrap().is_empty());
    let again = revoke_tombstone(
        &mut store,
        REPO,
        "packs/main",
        "<urn:a> <urn:q> \"secret\" .",
        "repeat",
        T3,
        None,
    )
    .unwrap();
    assert!(!again.revoked);
    assert_eq!(again.graphs_restored, 0);
    let replay = compose_current(
        &mut store,
        REPO,
        &keys(&["packs/main"]),
        &[second_pack],
        None,
        T3,
        None,
    )
    .unwrap();
    assert!(!replay.moved);
    assert!(
        !has(&store, &current_dataset(REPO), "urn:q", "secret"),
        "same stripped snapshot never refills"
    );
    assert!(tombstone_counts(&store, REPO).unwrap().is_empty());
    commit(
        &mut store,
        REPO,
        "<urn:a> <urn:p> \"v3\" ; <urn:q> \"secret\" .",
        T4,
    );
    assert!(has(&store, &current_dataset(REPO), "urn:q", "secret"));
    rollback_current(&mut store, REPO, T5, None).unwrap();
    assert!(
        !has(&store, &current_dataset(REPO), "urn:q", "secret"),
        "rollback selects the stripped snapshot without inventing content"
    );
    assert!(
        tombstone_counts(&store, REPO).unwrap().is_empty(),
        "not harvested again"
    );
    let back = rollback_current(&mut store, REPO, T5, None).unwrap();
    assert!(has(&store, &current_dataset(REPO), "urn:q", "secret"));
    retract(
        &mut store,
        &back.current.members[0].graph,
        "urn:a",
        "urn:q",
        "secret",
    );
    commit(
        &mut store,
        REPO,
        "<urn:a> <urn:p> \"v4\" ; <urn:q> \"secret\" .",
        T5,
    );
    assert!(!has(&store, &current_dataset(REPO), "urn:q", "secret"));
    assert_eq!(tombstone_counts(&store, REPO).unwrap()["packs/main"], 1);
}

#[test]
fn revoke_is_scoped_and_refuses_ambiguous_inputs_without_changes() {
    let mut store = Store::open_in_memory().unwrap();
    for repo in [REPO, "org/other"] {
        let first = commit(&mut store, repo, "<urn:a> <urn:q> \"secret\" .", T1);
        retract(
            &mut store,
            &first.current.unwrap().members[0].graph,
            "urn:a",
            "urn:q",
            "secret",
        );
        commit(
            &mut store,
            repo,
            "<urn:a> <urn:q> \"secret\" ; <urn:r> \"new\" .",
            T2,
        );
    }
    for bad in [
        "",
        "_:b <urn:q> \"secret\" .",
        "<urn:a> <urn:q> \"secret\" .\n<urn:a> <urn:r> \"new\" .",
    ] {
        assert!(
            revoke_tombstone(&mut store, REPO, "packs/main", bad, "invalid", T3, None).is_err()
        );
        assert!(!has(&store, &current_dataset(REPO), "urn:q", "secret"));
        assert_eq!(tombstone_counts(&store, REPO).unwrap()["packs/main"], 1);
    }
    let unknown = revoke_tombstone(
        &mut store,
        REPO,
        "packs/other",
        "<urn:a> <urn:q> \"secret\" .",
        "wrong pack",
        T3,
        None,
    )
    .unwrap();
    assert!(!unknown.revoked);
    revoke_tombstone(
        &mut store,
        REPO,
        "packs/main",
        "<urn:a> <urn:q> \"secret\" .",
        "undo",
        T3,
        None,
    )
    .unwrap();
    assert!(!has(&store, &current_dataset(REPO), "urn:q", "secret"));
    commit(
        &mut store,
        REPO,
        "<urn:a> <urn:q> \"secret\" ; <urn:r> \"fresh\" .",
        T4,
    );
    assert!(has(&store, &current_dataset(REPO), "urn:q", "secret"));
    assert!(!has(
        &store,
        &current_dataset("org/other"),
        "urn:q",
        "secret"
    ));
    assert_eq!(
        tombstone_counts(&store, "org/other").unwrap()["packs/main"],
        1
    );
    assert!(store.current_facts().unwrap().is_empty());
}

#[test]
fn failed_ledger_update_rolls_back_restoration() {
    let mut store = Store::open_in_memory().unwrap();
    let first = commit(&mut store, REPO, "<urn:a> <urn:q> \"secret\" .", T1);
    retract(
        &mut store,
        &first.current.unwrap().members[0].graph,
        "urn:a",
        "urn:q",
        "secret",
    );
    commit(
        &mut store,
        REPO,
        "<urn:a> <urn:q> \"secret\" ; <urn:r> \"new\" .",
        T2,
    );
    store.conn.execute_batch("CREATE TRIGGER refuse_revoke BEFORE DELETE ON pendant_repo_tombstones BEGIN SELECT RAISE(ABORT, 'injected refusal'); END;").unwrap();
    assert!(
        revoke_tombstone(
            &mut store,
            REPO,
            "packs/main",
            "<urn:a> <urn:q> \"secret\" .",
            "undo",
            T3,
            None
        )
        .is_err()
    );
    assert!(!has(&store, &current_dataset(REPO), "urn:q", "secret"));
    assert!(!has(&store, &previous_dataset(REPO), "urn:q", "secret"));
    assert_eq!(tombstone_counts(&store, REPO).unwrap()["packs/main"], 1);
}

#[test]
fn rollback_moves_the_pointer_back_and_applies_newer_tombstones() {
    let mut store = Store::open_in_memory().unwrap();
    commit(
        &mut store,
        REPO,
        "<urn:a> <urn:p> \"1\" ; <urn:q> \"secret\" .",
        T1,
    );
    let v2 = commit(
        &mut store,
        REPO,
        "<urn:a> <urn:p> \"2\" ; <urn:q> \"secret\" .",
        T2,
    );
    let (v2_current, v1_snapshot) = (v2.current.unwrap(), v2.previous.unwrap().snapshot);
    // Retracted while v2 is current, then roll back to v1, which still holds it.
    retract(
        &mut store,
        &v2_current.members[0].graph,
        "urn:a",
        "urn:q",
        "secret",
    );
    let back = rollback_current(&mut store, REPO, T3, None).unwrap();
    assert_eq!(back.current.snapshot, v1_snapshot);
    assert_eq!(back.previous.snapshot, v2_current.snapshot);
    assert_eq!(
        (back.tombstones_harvested, back.tombstones_reapplied),
        (1, 1)
    );
    assert!(has(&store, &current_dataset(REPO), "urn:p", "1"));
    assert!(
        !has(&store, &current_dataset(REPO), "urn:q", "secret"),
        "rollback must not resurrect a retraction"
    );
    assert!(
        has(&store, &previous_dataset(REPO), "urn:p", "2"),
        "roll-forward target kept"
    );
}

#[test]
fn repositories_are_isolated_even_for_identical_bytes() {
    let mut store = Store::open_in_memory().unwrap();
    // ONE pack (one share id) composed into two repositories: the case where a
    // content-addressed graph would be shared.
    let same = pack("<urn:a> <urn:p> \"keep\" ; <urn:q> \"secret\" .", "");
    let k = keys(&["packs/main"]);
    let a = compose_current(
        &mut store,
        "org/a",
        &k,
        std::slice::from_ref(&same),
        None,
        T1,
        None,
    )
    .unwrap();
    let b = compose_current(&mut store, "org/b", &k, &[same], None, T1, None).unwrap();
    let ga = a.current.unwrap().members[0].graph.clone();
    assert_ne!(
        ga,
        b.current.unwrap().members[0].graph,
        "repo-scoped graphs"
    );
    retract(&mut store, &ga, "urn:a", "urn:q", "secret");
    assert!(
        has(&store, &current_dataset("org/b"), "urn:q", "secret"),
        "a retraction in one repository never shows through another's CURRENT"
    );
    let next = pack("<urn:a> <urn:p> \"keep2\" ; <urn:q> \"secret\" .", "");
    compose_current(
        &mut store,
        "org/a",
        &k,
        std::slice::from_ref(&next),
        None,
        T2,
        None,
    )
    .unwrap();
    compose_current(&mut store, "org/b", &k, &[next], None, T2, None).unwrap();
    assert!(!has(&store, &current_dataset("org/a"), "urn:q", "secret"));
    assert!(
        has(&store, &current_dataset("org/b"), "urn:q", "secret"),
        "nor into its later snapshots"
    );
    assert!(tombstone_counts(&store, "org/b").unwrap().is_empty());
}

#[test]
fn a_quarantined_commit_leaves_current_where_it_was() {
    let mut store = Store::open_in_memory().unwrap();
    let good = pack("<urn:alice> a <urn:Person>; <urn:name> \"Alice\" .", SHAPES);
    let bad = pack("<urn:alice> a <urn:Person> .", SHAPES);
    let first = compose_current(&mut store, REPO, &keys(&["k"]), &[good], None, T1, None).unwrap();
    let second = compose_current(&mut store, REPO, &keys(&["k"]), &[bad], None, T2, None).unwrap();
    assert_eq!(second.composition.outcome, "quarantined");
    assert!(!second.moved);
    assert_eq!(
        second.current.unwrap().snapshot,
        first.current.unwrap().snapshot,
        "an inadmissible snapshot never becomes CURRENT"
    );
    assert!(second.previous.is_none());
}

#[test]
fn a_blank_node_retraction_fails_closed_and_changes_nothing() {
    let mut store = Store::open_in_memory().unwrap();
    let v1 = commit(
        &mut store,
        REPO,
        "_:x <urn:p> \"blank\" . <urn:a> <urn:p> \"y\" .",
        T1,
    );
    let current = v1.current.unwrap();
    let g = store.lookup(&current.members[0].graph).unwrap().unwrap();
    let blank = store
        .current_facts_in_graph(g)
        .unwrap()
        .into_iter()
        .find(|f| matches!(f.value, Value::Str(ref s) if s == "blank"))
        .unwrap();
    store
        .transact_to_graph(
            &[crate::store::Datum {
                entity: blank.entity,
                attribute: blank.attribute,
                value: blank.value,
                valid_from: T1.into(),
                valid_to: None,
                op: Op::Retract,
            }],
            T1,
            None,
            None,
            g,
        )
        .unwrap();
    let err = compose_current(
        &mut store,
        REPO,
        &keys(&["packs/main"]),
        &[pack("<urn:a> <urn:p> \"z\" .", "")],
        None,
        T2,
        None,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("blank-node"), "{err}");
    assert_eq!(
        read_pointer(&store, REPO, "current")
            .unwrap()
            .unwrap()
            .snapshot,
        current.snapshot
    );
    assert!(tombstone_counts(&store, REPO).unwrap().is_empty());
}

#[test]
fn a_late_failure_leaves_pointers_graphs_and_caches_intact() {
    let mut store = Store::open_in_memory().unwrap();
    let v1 = commit(&mut store, REPO, "<urn:a> <urn:p> \"v1\" .", T1);
    store
        .conn
        .execute_batch(
            "CREATE TEMP TRIGGER refuse_current BEFORE INSERT ON pendant_repo_pointers \
             WHEN NEW.slot = 'current' BEGIN SELECT RAISE(ABORT, 'fixture refuses the move'); END;",
        )
        .unwrap();
    let v2 = pack("<urn:a> <urn:p> \"v2\" .", "");
    let v2_graph = Scope::Repository {
        repo: REPO,
        keys: &keys(&["packs/main"]),
        tombstones: &[BTreeSet::new()],
    }
    .pack_graph(0, &v2.manifest.share_id);
    assert!(
        compose_current(
            &mut store,
            REPO,
            &keys(&["packs/main"]),
            &[v2],
            None,
            T2,
            None
        )
        .is_err()
    );
    // Rolled back AFTER previous was written and the new graph ingested.
    assert!(read_pointer(&store, REPO, "previous").unwrap().is_none());
    assert_eq!(
        read_pointer(&store, REPO, "current")
            .unwrap()
            .unwrap()
            .snapshot,
        v1.current.unwrap().snapshot
    );
    assert!(
        store
            .lookup(&v2_graph)
            .unwrap()
            .is_none_or(|g| store.graph_class(g).unwrap().is_none())
    );
    assert!(has(&store, &current_dataset(REPO), "urn:p", "v1"));
    assert!(
        !has(&store, &current_dataset(REPO), "urn:p", "v2"),
        "read model not stale"
    );
    store
        .conn
        .execute_batch("DROP TRIGGER refuse_current")
        .unwrap();
    assert!(
        commit(&mut store, REPO, "<urn:a> <urn:p> \"v2\" .", T3).moved,
        "positive control"
    );
}

#[test]
fn moved_pointers_name_only_their_current_members_in_the_meta_graph() {
    let mut store = Store::open_in_memory().unwrap();
    commit(&mut store, REPO, "<urn:a> <urn:p> \"v1\" .", T1);
    commit(&mut store, REPO, "<urn:a> <urn:p> \"v2\" .", T2);
    let v3 = commit(&mut store, REPO, "<urn:a> <urn:p> \"v3\" .", T3);
    let query = format!(
        "SELECT ?g WHERE {{ GRAPH <{META_GRAPH_IRI}> {{ <{}> <{QUIPU_INCLUDES_GRAPH}> ?g }} }}",
        current_dataset(REPO)
    );
    let rows = crate::sparql_query(&store, &query).unwrap();
    assert_eq!(
        rows.rows().len(),
        1,
        "meta-graph must not keep every graph CURRENT ever held"
    );
    let Some(Value::Ref(id)) = rows.rows()[0].values().next() else {
        panic!("expected a graph term: {:?}", rows.rows()[0]);
    };
    assert_eq!(
        store.resolve(*id).unwrap(),
        v3.current.unwrap().members[0].graph
    );
}

#[test]
fn recomposing_the_current_snapshot_does_not_move_anything() {
    let mut store = Store::open_in_memory().unwrap();
    commit(&mut store, REPO, "<urn:a> <urn:p> \"v1\" .", T1);
    // The SAME pack bytes recomposed (re-sharing would mint a new share id).
    let v2_pack = pack("<urn:a> <urn:p> \"v2\" .", "");
    let k = keys(&["packs/main"]);
    let v2 = compose_current(
        &mut store,
        REPO,
        &k,
        std::slice::from_ref(&v2_pack),
        None,
        T2,
        None,
    )
    .unwrap();
    let again = compose_current(&mut store, REPO, &k, &[v2_pack], None, T5, None).unwrap();
    assert!(!again.moved);
    assert_eq!(
        again.previous.unwrap().snapshot,
        v2.previous.unwrap().snapshot,
        "previous kept"
    );
}

#[test]
fn names_are_validated_and_keys_are_unique() {
    let mut store = Store::open_in_memory().unwrap();
    let p = || pack("<urn:a> <urn:p> \"x\" .", "");
    for repo in ["", "a:b", "a//b", "../x", "a b"] {
        assert!(
            compose_current(&mut store, repo, &keys(&["k"]), &[p()], None, T1, None).is_err(),
            "{repo:?}"
        );
    }
    assert!(
        compose_current(
            &mut store,
            REPO,
            &keys(&["k", "k"]),
            &[p(), p()],
            None,
            T1,
            None
        )
        .unwrap_err()
        .to_string()
        .contains("twice")
    );
    assert!(compose_current(&mut store, REPO, &keys(&["k"]), &[p(), p()], None, T1, None).is_err());
    assert!(
        rollback_current(&mut store, REPO, T1, None).is_err(),
        "nothing to roll back"
    );
}
