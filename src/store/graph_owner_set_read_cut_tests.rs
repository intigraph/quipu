//! Phase 3 S3 (aegis-n0wc.5): the composed reader's publication cut. A
//! participant batch across two set files is visible to a composed read in
//! ALL its files or NONE: an applied but unpublished participant reads at its
//! pre-batch head (the old cut) until the home journal says PUBLISHED.
//! Fixture: a legacy file plus two routed set files, each in its own term
//! space, read black-box through the authorized composed ATTACH view.
use super::*;
use crate::store::{
    Datum,
    graph_owner::participant::{
        CrashPoint, Member, ParticipantBatch, Recovery, commit, recover, with_crash,
    },
};
use crate::transaction_auth::{AttestationSlot, Identity, with_identity};
use crate::types::{Op, Value as TermValue};

const A_DATA: &str = "urn:alpha:data";
const B_DATA: &str = "urn:beta:data";
const SETS: [(&str, &str, i64); 2] = [("alpha", A_DATA, 3), ("beta", B_DATA, 4)];

struct Cut {
    root: std::path::PathBuf,
    _dir: Option<tempfile::TempDir>,
    dataset: GraphSetDataset,
    admission: ComposedReadAdmission,
    /// Per set file: (urn:item, urn:value) term ids in that file's space.
    terms: Vec<(i64, i64)>,
}

/// Create the three files once: alpha and beta each hold `urn:item
/// urn:value "old"`, then are respaced into disjoint term spaces.
fn create(root: &Path) {
    let mut legacy = file(root, "legacy", &[REFS]);
    write(&mut legacy, REFS, "<urn:ref> <urn:p> <urn:o> .");
    for i in 0..96 {
        legacy.intern(&format!("urn:legacy-pad:{i}")).unwrap();
    }
    drop(legacy);
    for (name, graph, space) in SETS {
        let mut store = file(root, name, &[graph]);
        write(&mut store, graph, "<urn:item> <urn:value> \"old\" .");
        let path = root.join(format!("{name}.db"));
        let tmp = root.join(format!("{name}.respaced.db"));
        drop(store);
        crate::store::respace::respace_file(&path, &tmp, space).unwrap();
        std::fs::rename(&tmp, &path).unwrap();
        Store::adopt(path.to_str().unwrap()).unwrap();
    }
}

/// (Re)open the dataset from existing files, as a restarted server would.
fn open_cut(root: &Path, dir: Option<tempfile::TempDir>) -> Cut {
    let legacy = Store::open(root.join("legacy.db").to_str().unwrap()).unwrap();
    let mut stores = Vec::new();
    let mut terms = Vec::new();
    for (name, _, _) in SETS {
        let store = Store::open(root.join(format!("{name}.db")).to_str().unwrap()).unwrap();
        terms.push((
            store.lookup("urn:item").unwrap().unwrap(),
            store.lookup("urn:value").unwrap().unwrap(),
        ));
        stores.push(store);
    }
    let catalog = Arc::new(
        GraphSetCatalog::new(
            7,
            &legacy.store_id().unwrap(),
            stores
                .iter()
                .zip(SETS)
                .map(|(store, (name, graph, _))| GraphSet {
                    owner: pin(store, name, graph),
                    graphs: vec![graph.into()],
                })
                .collect(),
            Budget {
                max_owners: 2,
                queued_per_owner: 1,
                readers_per_owner: 1,
                max_queued: 2,
                max_connections: 6,
            },
            4,
        )
        .unwrap()
        .with_legacy_graphs(
            pin(&legacy, "legacy", ROOT_GRAPH_IRI),
            vec![ROOT_GRAPH_IRI.into(), REFS.into()],
        )
        .unwrap(),
    );
    let admission =
        ComposedReadAdmission::for_graph_sets(catalog.clone(), 768, 3, 1, reservation()).unwrap();
    let owners = stores
        .into_iter()
        .zip(SETS)
        .map(|(store, (_, graph, _))| {
            GraphSetOwner::prepare(
                catalog.clone(),
                root,
                graph,
                store,
                "policy-v1",
                admission.allocation(),
            )
            .unwrap()
        })
        .collect();
    let dataset = GraphSetDataset::prepare(
        catalog,
        root,
        legacy,
        owners,
        "policy-v1",
        admission.allocation(),
    )
    .unwrap();
    Cut {
        root: root.to_path_buf(),
        _dir: dir,
        dataset,
        admission,
        terms,
    }
}

fn fixture() -> Cut {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    create(&root);
    open_cut(&root, Some(dir))
}

/// Simulated restart: drop every owner and reopen from the same files.
fn restart(cut: Cut) -> Cut {
    let Cut {
        root,
        _dir,
        dataset,
        admission,
        ..
    } = cut;
    // Release every lease before reopening the files.
    drop(dataset);
    drop(admission);
    open_cut(&root, _dir)
}

fn owner<'a>(cut: &'a Cut, graph: &str) -> &'a GraphSetOwner {
    &cut.dataset.routed[graph]
}

/// One batch: in each file assert `urn:item urn:value "new"` and retract "old".
fn batch<'a>(cut: &'a Cut, id: &str) -> ParticipantBatch<'a> {
    let members = SETS
        .iter()
        .enumerate()
        .map(|(index, (_, graph, _))| {
            let owner = owner(cut, graph);
            let route = owner.owner.catalog_route();
            let (entity, attribute) = cut.terms[index];
            let datum = |value: &str, op| Datum {
                entity,
                attribute,
                value: TermValue::Str(value.into()),
                valid_from: "2026-01-01T00:00:00Z".into(),
                valid_to: None,
                op,
            };
            Member {
                owner,
                requests: vec![crate::graph_catalog::WriteRequest {
                    graph: Some(graph),
                    catalog_revision: 7,
                    owner_generation: Some(3),
                    policy: "policy-v1",
                    signed: false,
                    cross_graph_context: false,
                }],
                batch: GraphSetBatch {
                    store_id: route.store_id.clone(),
                    log_epoch: route.log_epoch.clone(),
                    graphs: vec![(
                        (*graph).into(),
                        vec![datum("new", Op::Assert), datum("old", Op::Retract)],
                    )],
                    timestamp: "2026-01-01T00:00:00Z".into(),
                    actor: Some("fixture".into()),
                    source: Some("fixture".into()),
                },
            }
        })
        .collect();
    ParticipantBatch {
        batch_id: id.into(),
        members,
    }
}

fn as_writer<T>(work: impl FnOnce() -> T) -> T {
    with_identity(
        Some(Identity {
            principal: "urn:principal:writer".into(),
            credential_id: None,
            auth_class: "test".into(),
            attestation: AttestationSlot::default(),
        }),
        work,
    )
}

/// Run one composed SPARQL query over exactly `graphs`.
fn query(cut: &Cut, graphs: &[&str], body: &str) -> Result<serde_json::Value> {
    let (policy, identity) = authority(graphs);
    let policy = policy.snapshot().unwrap();
    let permit = cut.admission.try_reserve()?;
    let from: String = graphs
        .iter()
        .map(|g| format!("FROM NAMED <{g}> "))
        .collect();
    let output = cut
        .dataset
        .read_attached(graphs, deadline(), |reads, checkout| {
            let view = policy
                .authorize(&identity, ReadOperation::Query, None, 2)?
                .open_attached_cut(reads, checkout, permit, 2, deadline())?;
            view.query(&body.replace("{from}", &from), false, 2)
        })?;
    Ok(serde_json::from_slice(&output).unwrap())
}

/// Facts about urn:item visible to an aggregate (a different evaluator path).
fn counted(cut: &Cut, graphs: &[&str]) -> i64 {
    let json = query(
        cut,
        graphs,
        "SELECT (COUNT(*) AS ?n) {from} WHERE { GRAPH ?g { <urn:item> ?p ?o } }",
    )
    .unwrap();
    json["results"]["bindings"][0]["n"]["value"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}

/// The composed answer: sorted (graph, value) pairs for urn:item urn:value.
fn seen(cut: &Cut, graphs: &[&str]) -> Result<Vec<(String, String)>> {
    let (policy, identity) = authority(graphs);
    let policy = policy.snapshot().unwrap();
    let permit = cut.admission.try_reserve()?;
    let output = cut
        .dataset
        .read_attached(graphs, deadline(), |reads, checkout| {
            let view = policy
                .authorize(&identity, ReadOperation::Query, None, 2)?
                .open_attached_cut(reads, checkout, permit, 2, deadline())?;
            let from: String = graphs
                .iter()
                .map(|g| format!("FROM NAMED <{g}> "))
                .collect();
            view.query(
                &format!(
                    "SELECT ?g ?v {from} WHERE {{ GRAPH ?g {{ <urn:item> <urn:value> ?v }} }}"
                ),
                false,
                2,
            )
        })?;
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let mut rows: Vec<_> = json["results"]["bindings"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| {
            (
                row["g"]["value"].as_str().unwrap().to_owned(),
                row["v"]["value"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    rows.sort();
    Ok(rows)
}

fn both(value: &str) -> Vec<(String, String)> {
    vec![(A_DATA.into(), value.into()), (B_DATA.into(), value.into())]
}

fn crashed_at(point: CrashPoint) -> Cut {
    let cut = fixture();
    assert_eq!(seen(&cut, &[A_DATA, B_DATA]).unwrap(), both("old"));
    let error = as_writer(|| with_crash(point, || commit(&batch(&cut, "batch-cut")))).unwrap_err();
    assert!(
        error.to_string().contains("injected participant crash"),
        "{error}"
    );
    restart(cut)
}

#[test]
fn an_unpublished_apply_in_one_file_reads_as_the_old_cut_in_both() {
    // "A crash between participants publishes neither": alpha applied, beta not.
    let cut = crashed_at(CrashPoint::AfterApply(0));
    assert_eq!(seen(&cut, &[A_DATA, B_DATA]).unwrap(), both("old"));
    // Recovery publishes the complete new cut, exactly once.
    let (alpha, beta) = (owner(&cut, A_DATA), owner(&cut, B_DATA));
    assert!(matches!(
        recover(&[alpha, beta], "batch-cut").unwrap(),
        Recovery::Published(_)
    ));
    assert_eq!(seen(&cut, &[A_DATA, B_DATA]).unwrap(), both("new"));
}

#[test]
fn every_file_applied_but_unpublished_still_reads_as_the_old_cut() {
    let cut = crashed_at(CrashPoint::AfterApply(1));
    // The retraction of "old" is installed in both files, yet masked.
    assert_eq!(seen(&cut, &[A_DATA, B_DATA]).unwrap(), both("old"));
    // An aggregate sees the same old cut: one fact per file, not 0 or 2.
    assert_eq!(counted(&cut, &[A_DATA, B_DATA]), 2);
    let (alpha, beta) = (owner(&cut, A_DATA), owner(&cut, B_DATA));
    recover(&[alpha, beta], "batch-cut").unwrap();
    assert_eq!(counted(&cut, &[A_DATA, B_DATA]), 2);
    assert_eq!(seen(&cut, &[A_DATA, B_DATA]).unwrap(), both("new"));
}

#[test]
fn a_published_home_decision_reads_as_the_new_cut_before_fences_release() {
    let cut = crashed_at(CrashPoint::AfterPublish);
    assert_eq!(seen(&cut, &[A_DATA, B_DATA]).unwrap(), both("new"));
}

#[test]
fn a_single_unhomed_participant_reads_its_cut_from_the_home_journal_or_refuses() {
    // beta selected alone; its batch home is alpha (first in canonical order).
    let cut = crashed_at(CrashPoint::AfterApply(1));
    assert_eq!(
        seen(&cut, &[B_DATA]).unwrap(),
        vec![(B_DATA.into(), "old".into())]
    );
    let published = crashed_at(CrashPoint::AfterPublish);
    assert_eq!(
        seen(&published, &[B_DATA]).unwrap(),
        vec![(B_DATA.into(), "new".into())]
    );
    // With the home file gone the old cut is not provable: refuse. (The
    // dataset keeps alpha's owner, but alpha is not selected, so only the
    // home lookup can notice.)
    std::fs::rename(cut.root.join("alpha.db"), cut.root.join("alpha.moved")).unwrap();
    let error = seen(&cut, &[B_DATA]).unwrap_err();
    assert!(error.to_string().contains("home"), "{error}");
}

#[test]
fn a_live_participant_writer_refuses_the_composed_read_never_a_partial_cut() {
    let cut = fixture();
    let held = owner(&cut, B_DATA).owner.writer.lock();
    let error = seen(&cut, &[A_DATA, B_DATA]).unwrap_err();
    drop(held);
    assert!(error.to_string().contains("fence occupied"), "{error}");
    assert_eq!(seen(&cut, &[A_DATA, B_DATA]).unwrap(), both("old"));
}

#[test]
fn single_file_owner_reads_keep_refusing_an_unpublished_file() {
    let cut = crashed_at(CrashPoint::AfterApply(0));
    let error = owner(&cut, A_DATA)
        .owner
        .read(Store::latest_tx_id)
        .unwrap_err();
    assert!(error.to_string().contains("unpublished"), "{error}");
}

#[test]
fn an_unselected_home_without_a_decision_or_with_another_identity_refuses() {
    // beta selected alone, its batch applied in both files but unpublished;
    // the home (alpha) is read from its file. Each damage makes the old cut
    // unprovable, so the composed read refuses instead of guessing a cut.
    let cases = [
        ("DELETE FROM participant_journal", "no decision"),
        ("DROP TABLE participant_journal", "no decision"),
        (
            "UPDATE store_identity SET store_id = 'urn:uuid:foreign-store' WHERE id = 1",
            "identity differs",
        ),
        (
            "UPDATE log_epoch SET epoch = 'urn:uuid:foreign-epoch' WHERE id = 1",
            "identity differs",
        ),
    ];
    for (damage, expected) in cases {
        let cut = crashed_at(CrashPoint::AfterApply(1));
        assert_eq!(
            seen(&cut, &[B_DATA]).unwrap(),
            vec![(B_DATA.into(), "old".into())],
            "control before: {damage}"
        );
        rusqlite::Connection::open(cut.root.join("alpha.db"))
            .unwrap()
            .execute_batch(damage)
            .unwrap();
        let error = seen(&cut, &[B_DATA]).unwrap_err();
        assert!(error.to_string().contains(expected), "{damage}: {error}");
    }
}

#[test]
fn a_participant_apply_that_changes_graph_rows_refuses_and_applies_nothing() {
    // The composed reader copies graph rows without a cut bound, so a
    // participant apply must never change them. Force one (a trigger that
    // rewrites beta's graph row on any fact insert): the apply refuses inside
    // its own transaction and no file shows the batch.
    let cut = fixture();
    rusqlite::Connection::open(cut.root.join("beta.db"))
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER rewrite_graph_row AFTER INSERT ON facts \
             BEGIN UPDATE graphs SET created_at = created_at || 'x'; END;",
        )
        .unwrap();
    let cut = restart(cut);
    let error = as_writer(|| commit(&batch(&cut, "batch-graphs"))).unwrap_err();
    assert!(error.to_string().contains("graph rows"), "{error}");
    let cut = restart(cut);
    let (alpha, beta) = (owner(&cut, A_DATA), owner(&cut, B_DATA));
    // Committed batch: recovery keeps refusing on the same rows, never a
    // partial publish; until then a composed read refuses or shows the old cut.
    let error = recover(&[alpha, beta], "batch-graphs").unwrap_err();
    assert!(error.to_string().contains("graph rows"), "{error}");
    match seen(&cut, &[A_DATA, B_DATA]) {
        Ok(rows) => assert_eq!(rows, both("old")),
        Err(error) => assert!(error.to_string().contains("home"), "{error}"),
    }
}
