//! Phase 3 S1 (aegis-n0wc.5): participant commit across two graph-set files.
//! A crash is simulated by an injected early return that skips every cleanup,
//! then dropping and reopening each owner from its file. LIMIT: this is not a
//! process kill; a subprocess kill row is a later slice.
use super::*;
use crate::{
    graph_catalog::{Budget, GraphSet, Route},
    store::Datum,
    transaction_auth::{AttestationSlot, Identity, with_identity},
    types::{Op, Value as TermValue},
};
use std::time::{Duration, Instant};

const FILES: [(&str, [&str; 2]); 3] = [
    ("alpha", ["urn:alpha:data", "urn:alpha:provenance"]),
    ("beta", ["urn:beta:data", "urn:beta:provenance"]),
    ("gamma", ["urn:gamma:data", "urn:gamma:provenance"]),
];

struct Files {
    root: std::path::PathBuf,
    catalog: Arc<GraphSetCatalog>,
    /// Per file: (entity, attribute) term ids, stable in that file.
    terms: Vec<(i64, i64)>,
    routes: Vec<Route>,
}

fn files(root: &Path) -> Files {
    let mut routes = Vec::new();
    let mut terms = Vec::new();
    for (file, graphs) in FILES {
        let path = root.join(format!("{file}.db"));
        {
            let store = Store::open(path.to_str().unwrap()).unwrap();
            for graph in graphs {
                store.graph_create(graph).unwrap();
            }
            terms.push((
                store.intern("urn:item").unwrap(),
                store.intern("urn:value").unwrap(),
            ));
        }
        Store::adopt(path.to_str().unwrap()).unwrap();
        let store = Store::open(path.to_str().unwrap()).unwrap();
        routes.push(Route {
            graph: graphs[0].into(),
            file_id: file.into(),
            store_id: store.store_id().unwrap(),
            log_epoch: store.log_epoch().unwrap(),
            generation: 3,
            policy: "policy-v1".into(),
        });
    }
    let catalog = Arc::new(
        GraphSetCatalog::new(
            7,
            "legacy",
            routes
                .iter()
                .zip(FILES)
                .map(|(route, (_, graphs))| GraphSet {
                    owner: route.clone(),
                    graphs: graphs.iter().map(|g| (*g).into()).collect(),
                })
                .collect(),
            Budget {
                max_owners: 3,
                queued_per_owner: 1,
                readers_per_owner: 1,
                max_queued: 3,
                max_connections: 9,
            },
            6,
        )
        .unwrap(),
    );
    Files {
        root: root.to_path_buf(),
        catalog,
        terms,
        routes,
    }
}

/// (Re)open one file's owner, as a restarted server would.
fn open(files: &Files, index: usize) -> GraphSetOwner {
    let path = files.root.join(format!("{}.db", FILES[index].0));
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let allocation = files
        .catalog
        .physical()
        .memory_allocation(960, 1, 1)
        .unwrap();
    GraphSetOwner::prepare(
        files.catalog.clone(),
        &files.root,
        FILES[index].1[0],
        store,
        "policy-v1",
        allocation,
    )
    .unwrap()
}

fn requests(index: usize) -> Vec<WriteRequest<'static>> {
    FILES[index]
        .1
        .into_iter()
        .map(|graph| WriteRequest {
            graph: Some(graph),
            catalog_revision: 7,
            owner_generation: Some(3),
            policy: "policy-v1",
            signed: false,
            cross_graph_context: false,
        })
        .collect()
}

fn batch(files: &Files, index: usize, value: &str) -> GraphSetBatch {
    let (entity, attribute) = files.terms[index];
    let datum = Datum {
        entity,
        attribute,
        value: TermValue::Str(value.into()),
        valid_from: "2026-01-01T00:00:00Z".into(),
        valid_to: None,
        op: Op::Assert,
    };
    GraphSetBatch {
        store_id: files.routes[index].store_id.clone(),
        log_epoch: files.routes[index].log_epoch.clone(),
        graphs: FILES[index]
            .1
            .iter()
            .map(|g| ((*g).to_string(), vec![datum.clone()]))
            .collect(),
        timestamp: "2026-01-01T00:00:00Z".into(),
        actor: Some("fixture".into()),
        source: Some("fixture".into()),
    }
}

fn members<'a>(
    files: &Files,
    owners: &[&'a GraphSetOwner],
    indexes: &[usize],
    value: &str,
) -> Vec<Member<'a>> {
    owners
        .iter()
        .zip(indexes)
        .map(|(owner, &index)| Member {
            owner,
            requests: requests(index),
            batch: batch(files, index, value),
        })
        .collect()
}

fn principal(name: &str) -> Option<Identity> {
    Some(Identity {
        principal: name.into(),
        credential_id: None,
        auth_class: "test".into(),
        attestation: AttestationSlot::default(),
    })
}

fn as_writer<T>(work: impl FnOnce() -> T) -> T {
    with_identity(principal("urn:principal:writer"), work)
}

/// Asserted facts per member graph of `index`, read through the owner pool.
fn counts(owner: &GraphSetOwner, index: usize) -> Result<Vec<i64>> {
    owner.owner.read(|store| {
        FILES[index]
            .1
            .iter()
            .map(|g| {
                let graph = store.committed_graph_id(g)?.unwrap();
                Ok(store.conn.query_row(
                    "SELECT count(*) FROM facts WHERE g=?1 AND op=1",
                    [graph],
                    |r| r.get(0),
                )?)
            })
            .collect()
    })
}

fn local_write(files: &Files, owner: &GraphSetOwner, index: usize, value: &str) -> Result<()> {
    owner
        .transact(&requests(index), &batch(files, index, value))
        .map(|_| ())
}

fn raw_head(files: &Files, index: usize) -> i64 {
    // Read the file directly (read-only), bypassing every owner fence.
    let path = files.root.join(format!("{}.db", FILES[index].0));
    let conn =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    conn.query_row("SELECT coalesce(max(id),0) FROM transactions", [], |r| {
        r.get(0)
    })
    .unwrap()
}

fn raw_exec(files: &Files, index: usize, sql: &str) {
    let path = files.root.join(format!("{}.db", FILES[index].0));
    rusqlite::Connection::open(path)
        .unwrap()
        .execute_batch(sql)
        .unwrap();
}

fn injected(error: &crate::Error) -> bool {
    error.to_string().contains("injected participant crash")
}

#[test]
fn a_published_batch_returns_one_receipt_vector_and_both_files_show_the_new_cut() {
    let temp = tempfile::tempdir().unwrap();
    let files = files(temp.path());
    let (alpha, beta) = (open(&files, 0), open(&files, 1));
    let before = (raw_head(&files, 0), raw_head(&files, 1));
    let batch = ParticipantBatch {
        batch_id: "batch-1".into(),
        members: members(&files, &[&alpha, &beta], &[0, 1], "v1"),
    };
    let vector = as_writer(|| commit(&batch)).unwrap();
    assert_eq!(vector.batch_id, "batch-1");
    assert_eq!(
        vector
            .members
            .iter()
            .map(|m| m.file_id.as_str())
            .collect::<Vec<_>>(),
        ["alpha", "beta"]
    );
    for (receipt, (index, head)) in vector.members.iter().zip([(0, before.0), (1, before.1)]) {
        assert_eq!(receipt.store_id, files.routes[index].store_id);
        assert_eq!(receipt.log_epoch, files.routes[index].log_epoch);
        assert_eq!(receipt.heads.len(), 2);
        assert!(receipt.heads.iter().all(|(_, tx)| *tx > head));
        assert_eq!(receipt.heads.last().unwrap().1, raw_head(&files, index));
    }
    assert_eq!(counts(&alpha, 0).unwrap(), [1, 1]);
    assert_eq!(counts(&beta, 1).unwrap(), [1, 1]);
    // Published participants are ordinary owners again.
    local_write(&files, &alpha, 0, "after").unwrap();
    assert_eq!(counts(&alpha, 0).unwrap(), [2, 2]);
}

#[test]
fn a_crash_after_any_prepare_recovers_the_old_cut_and_keeps_the_id_consumed() {
    for point in [
        CrashPoint::AfterJournal,
        CrashPoint::AfterPrepare(0),
        CrashPoint::AfterPrepare(1),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let files = files(temp.path());
        let heads = (raw_head(&files, 0), raw_head(&files, 1));
        {
            let (alpha, beta) = (open(&files, 0), open(&files, 1));
            let batch = ParticipantBatch {
                batch_id: "batch-1".into(),
                members: members(&files, &[&alpha, &beta], &[0, 1], "v1"),
            };
            let error = as_writer(|| with_crash(point, || commit(&batch))).unwrap_err();
            assert!(injected(&error), "{point:?}: {error}");
            // A prepared participant refuses ordinary writes; nothing is visible.
            if point != CrashPoint::AfterJournal {
                let fenced = local_write(&files, &alpha, 0, "other").unwrap_err();
                assert!(
                    fenced.to_string().contains("participant fenced"),
                    "{point:?}: {fenced}"
                );
            }
            assert_eq!(counts(&alpha, 0).unwrap(), [0, 0]);
            assert_eq!(counts(&beta, 1).unwrap(), [0, 0]);
        }
        let (alpha, beta) = (open(&files, 0), open(&files, 1));
        let recovered = recover(&[&alpha, &beta], "batch-1").unwrap();
        assert_eq!(recovered, Recovery::Aborted, "{point:?}");
        assert_eq!((raw_head(&files, 0), raw_head(&files, 1)), heads);
        assert_eq!(counts(&alpha, 0).unwrap(), [0, 0]);
        assert_eq!(counts(&beta, 1).unwrap(), [0, 0]);
        // The aborted ID stays consumed: a retry never executes it again.
        let retry = ParticipantBatch {
            batch_id: "batch-1".into(),
            members: members(&files, &[&alpha, &beta], &[0, 1], "v1"),
        };
        let error = as_writer(|| commit(&retry)).unwrap_err();
        assert!(error.to_string().contains("aborted"), "{point:?}: {error}");
        // Both participants are released for unrelated work and new batches.
        local_write(&files, &alpha, 0, "other").unwrap();
        let fresh = ParticipantBatch {
            batch_id: "batch-2".into(),
            members: members(&files, &[&alpha, &beta], &[0, 1], "v2"),
        };
        as_writer(|| commit(&fresh)).unwrap();
        assert_eq!(counts(&beta, 1).unwrap(), [1, 1]);
    }
}

#[test]
fn a_crash_after_the_decision_finishes_the_same_batch_with_its_original_identity() {
    let temp = tempfile::tempdir().unwrap();
    let files = files(temp.path());
    {
        let (alpha, beta) = (open(&files, 0), open(&files, 1));
        let batch = ParticipantBatch {
            batch_id: "batch-1".into(),
            members: members(&files, &[&alpha, &beta], &[0, 1], "v1"),
        };
        let error =
            as_writer(|| with_crash(CrashPoint::AfterDecision, || commit(&batch))).unwrap_err();
        assert!(injected(&error), "{error}");
    }
    let (alpha, beta) = (open(&files, 0), open(&files, 1));
    // The decision is durable: recovery must finish, never abort.
    let error = local_write(&files, &beta, 1, "other").unwrap_err();
    assert!(error.to_string().contains("participant fenced"), "{error}");
    let Recovery::Published(vector) = recover(&[&alpha, &beta], "batch-1").unwrap() else {
        panic!("a committed decision recovered as an abort");
    };
    assert_eq!(counts(&alpha, 0).unwrap(), [1, 1]);
    assert_eq!(counts(&beta, 1).unwrap(), [1, 1]);
    // A retry with identical input returns the same vector and writes nothing.
    let head = (raw_head(&files, 0), raw_head(&files, 1));
    let retry = ParticipantBatch {
        batch_id: "batch-1".into(),
        members: members(&files, &[&alpha, &beta], &[0, 1], "v1"),
    };
    assert_eq!(as_writer(|| commit(&retry)).unwrap(), vector);
    assert_eq!((raw_head(&files, 0), raw_head(&files, 1)), head);
}

#[test]
fn a_crash_after_one_apply_hides_it_until_recovery_publishes_the_complete_cut() {
    for applied in [0, 1] {
        let temp = tempfile::tempdir().unwrap();
        let files = files(temp.path());
        let before = (raw_head(&files, 0), raw_head(&files, 1));
        {
            let (alpha, beta) = (open(&files, 0), open(&files, 1));
            let batch = ParticipantBatch {
                batch_id: "batch-1".into(),
                members: members(&files, &[&alpha, &beta], &[0, 1], "v1"),
            };
            let error =
                as_writer(|| with_crash(CrashPoint::AfterApply(applied), || commit(&batch)))
                    .unwrap_err();
            assert!(injected(&error), "{error}");
        }
        // The applied facts are physically present (raw head moved) ...
        assert!(raw_head(&files, 0) > before.0);
        let (alpha, beta) = (open(&files, 0), open(&files, 1));
        // ... but every owner read of an applied, unpublished participant
        // refuses, also after restart, until recovery publishes.
        let error = counts(&alpha, 0).unwrap_err();
        assert!(error.to_string().contains("unpublished"), "{error}");
        if applied == 0 {
            assert_eq!(raw_head(&files, 1), before.1);
            assert_eq!(counts(&beta, 1).unwrap(), [0, 0]);
        } else {
            assert!(
                counts(&beta, 1)
                    .unwrap_err()
                    .to_string()
                    .contains("unpublished")
            );
        }
        let Recovery::Published(vector) = recover(&[&alpha, &beta], "batch-1").unwrap() else {
            panic!("applied batch recovered as an abort");
        };
        // Exactly one application per participant, never a second apply.
        assert_eq!(counts(&alpha, 0).unwrap(), [1, 1]);
        assert_eq!(counts(&beta, 1).unwrap(), [1, 1]);
        assert_eq!(
            vector.members[0].heads.last().unwrap().1,
            raw_head(&files, 0)
        );
        assert_eq!(
            vector.members[1].heads.last().unwrap().1,
            raw_head(&files, 1)
        );
        // Recovery is itself idempotent.
        assert_eq!(
            recover(&[&alpha, &beta], "batch-1").unwrap(),
            Recovery::Published(vector)
        );
    }
}

#[test]
fn a_lost_reply_retry_returns_the_identical_vector_without_new_facts() {
    let temp = tempfile::tempdir().unwrap();
    let files = files(temp.path());
    {
        let (alpha, beta) = (open(&files, 0), open(&files, 1));
        let batch = ParticipantBatch {
            batch_id: "batch-1".into(),
            members: members(&files, &[&alpha, &beta], &[0, 1], "v1"),
        };
        let error =
            as_writer(|| with_crash(CrashPoint::AfterPublish, || commit(&batch))).unwrap_err();
        assert!(injected(&error), "{error}");
    }
    let (alpha, beta) = (open(&files, 0), open(&files, 1));
    let head = (raw_head(&files, 0), raw_head(&files, 1));
    let retry = ParticipantBatch {
        batch_id: "batch-1".into(),
        members: members(&files, &[&alpha, &beta], &[0, 1], "v1"),
    };
    let first = as_writer(|| commit(&retry)).unwrap();
    let second = as_writer(|| commit(&retry)).unwrap();
    assert_eq!(first, second);
    assert_eq!((raw_head(&files, 0), raw_head(&files, 1)), head);
    assert_eq!(counts(&alpha, 0).unwrap(), [1, 1]);
    assert_eq!(counts(&beta, 1).unwrap(), [1, 1]);
}

#[test]
fn the_same_id_with_different_bytes_or_principal_refuses_writing_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let files = files(temp.path());
    let (alpha, beta) = (open(&files, 0), open(&files, 1));
    let batch = ParticipantBatch {
        batch_id: "batch-1".into(),
        members: members(&files, &[&alpha, &beta], &[0, 1], "v1"),
    };
    let vector = as_writer(|| commit(&batch)).unwrap();
    let head = (raw_head(&files, 0), raw_head(&files, 1));
    let changed = ParticipantBatch {
        batch_id: "batch-1".into(),
        members: members(&files, &[&alpha, &beta], &[0, 1], "v2"),
    };
    let error = as_writer(|| commit(&changed)).unwrap_err();
    assert!(error.to_string().contains("different batch"), "{error}");
    let error = with_identity(principal("urn:principal:other"), || commit(&batch)).unwrap_err();
    assert!(error.to_string().contains("different batch"), "{error}");
    // An unattributed caller is refused before any fence or intent.
    let error = commit(&batch).unwrap_err();
    assert!(error.to_string().contains("verified principal"), "{error}");
    assert_eq!((raw_head(&files, 0), raw_head(&files, 1)), head);
    assert_eq!(as_writer(|| commit(&batch)).unwrap(), vector);
}

#[test]
fn a_missing_home_keeps_an_applied_participant_fenced_for_reads_and_writes() {
    let temp = tempfile::tempdir().unwrap();
    let files = files(temp.path());
    {
        let (alpha, beta) = (open(&files, 0), open(&files, 1));
        let batch = ParticipantBatch {
            batch_id: "batch-1".into(),
            members: members(&files, &[&alpha, &beta], &[0, 1], "v1"),
        };
        let error =
            as_writer(|| with_crash(CrashPoint::AfterApply(1), || commit(&batch))).unwrap_err();
        assert!(injected(&error), "{error}");
    }
    // Restart with the home (alpha, first in canonical order) unavailable.
    let beta = open(&files, 1);
    let error = recover(&[&beta], "batch-1").unwrap_err();
    assert!(error.to_string().contains("home participant"), "{error}");
    assert!(
        counts(&beta, 1)
            .unwrap_err()
            .to_string()
            .contains("unpublished")
    );
    let error = local_write(&files, &beta, 1, "other").unwrap_err();
    assert!(error.to_string().contains("participant fenced"), "{error}");
    // With the home back, recovery publishes the same complete cut.
    let alpha = open(&files, 0);
    assert!(matches!(
        recover(&[&alpha, &beta], "batch-1").unwrap(),
        Recovery::Published(_)
    ));
    assert_eq!(counts(&beta, 1).unwrap(), [1, 1]);
}

#[test]
fn a_damaged_intent_or_foreign_home_identity_stays_fenced_and_applies_nothing() {
    let damage = [
        // Same length and still valid JSON: only the digest can catch it.
        "UPDATE participant_intents SET payload = \
         CAST(replace(CAST(payload AS TEXT), '2026-01-01T', '2026-01-02T') AS BLOB)",
        "UPDATE participant_intents SET home_log_epoch = 'foreign-epoch'",
    ];
    for sql in damage {
        let temp = tempfile::tempdir().unwrap();
        let files = files(temp.path());
        let before = (raw_head(&files, 0), raw_head(&files, 1));
        {
            let (alpha, beta) = (open(&files, 0), open(&files, 1));
            let batch = ParticipantBatch {
                batch_id: "batch-1".into(),
                members: members(&files, &[&alpha, &beta], &[0, 1], "v1"),
            };
            let error =
                as_writer(|| with_crash(CrashPoint::AfterDecision, || commit(&batch))).unwrap_err();
            assert!(injected(&error), "{error}");
        }
        raw_exec(&files, 1, sql);
        // Positive control: the damage really landed in beta's intent row.
        let path = files.root.join("beta.db");
        let landed: i64 = rusqlite::Connection::open(path)
            .unwrap()
            .query_row(
                "SELECT count(*) FROM participant_intents WHERE \
                 instr(CAST(payload AS TEXT), '2026-01-02T') > 0 OR home_log_epoch = 'foreign-epoch'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(landed, 1, "{sql}: damage did not apply");
        let (alpha, beta) = (open(&files, 0), open(&files, 1));
        let error = recover(&[&alpha, &beta], "batch-1").unwrap_err();
        assert!(
            error.to_string().contains("intent"),
            "{sql}: wrong refusal: {error}"
        );
        // Every intent is verified BEFORE any apply: the home applied nothing.
        assert_eq!((raw_head(&files, 0), raw_head(&files, 1)), before, "{sql}");
        for (owner, index) in [(&alpha, 0), (&beta, 1)] {
            let error = local_write(&files, owner, index, "other").unwrap_err();
            assert!(error.to_string().contains("participant fenced"), "{sql}");
        }
    }
}

#[test]
fn an_unrelated_file_commits_and_reads_while_participants_are_fenced() {
    let temp = tempfile::tempdir().unwrap();
    let files = files(temp.path());
    {
        let (alpha, beta) = (open(&files, 0), open(&files, 1));
        let batch = ParticipantBatch {
            batch_id: "batch-1".into(),
            members: members(&files, &[&alpha, &beta], &[0, 1], "v1"),
        };
        let error =
            as_writer(|| with_crash(CrashPoint::AfterApply(0), || commit(&batch))).unwrap_err();
        assert!(injected(&error), "{error}");
    }
    let (alpha, gamma) = (open(&files, 0), open(&files, 2));
    assert!(counts(&alpha, 0).is_err());
    // The strict captured read (which the composed ATTACH reader is built on)
    // refuses the fenced file, and so any selection that includes it.
    let deadline = Instant::now() + Duration::from_secs(2);
    let error =
        Owner::read_strict(&[&alpha.owner, &gamma.owner], deadline, |_| Ok(())).unwrap_err();
    assert!(error.to_string().contains("unpublished"), "{error}");
    Owner::read_strict(&[&gamma.owner], deadline, |_| Ok(())).unwrap();
    local_write(&files, &gamma, 2, "unrelated").unwrap();
    assert_eq!(counts(&gamma, 2).unwrap(), [1, 1]);
}

#[test]
fn a_held_participant_writer_refuses_the_batch_with_nothing_prepared() {
    let temp = tempfile::tempdir().unwrap();
    let files = files(temp.path());
    let (alpha, beta) = (open(&files, 0), open(&files, 1));
    let batch = ParticipantBatch {
        batch_id: "batch-1".into(),
        members: members(&files, &[&alpha, &beta], &[0, 1], "v1"),
    };
    let held = beta.owner.writer.lock();
    let started = Instant::now();
    let error = as_writer(|| commit(&batch)).unwrap_err();
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "waited on a held writer"
    );
    drop(held);
    assert!(error.to_string().contains("busy"), "{error}");
    // Try-all admission released alpha and recorded no intent anywhere.
    local_write(&files, &alpha, 0, "other").unwrap();
    local_write(&files, &beta, 1, "other").unwrap();
    // The refused ID was never consumed.
    as_writer(|| commit(&batch)).unwrap();
}

#[test]
fn members_must_be_distinct_owners_of_one_catalog_spanning_two_files() {
    let temp = tempfile::tempdir().unwrap();
    let files = files(temp.path());
    let alpha = open(&files, 0);
    let one = ParticipantBatch {
        batch_id: "batch-1".into(),
        members: members(&files, &[&alpha], &[0], "v1"),
    };
    let error = as_writer(|| commit(&one)).unwrap_err();
    assert!(error.to_string().contains("two files"), "{error}");
    let twice = ParticipantBatch {
        batch_id: "batch-1".into(),
        members: members(&files, &[&alpha, &alpha], &[0, 0], "v1"),
    };
    let error = as_writer(|| commit(&twice)).unwrap_err();
    assert!(error.to_string().contains("duplicate"), "{error}");
    assert_eq!(counts(&alpha, 0).unwrap(), [0, 0]);
    local_write(&files, &alpha, 0, "other").unwrap();
}

#[test]
fn a_missing_non_home_participant_refuses_recovery_and_the_home_stays_fenced() {
    let temp = tempfile::tempdir().unwrap();
    let files = files(temp.path());
    {
        let (alpha, beta) = (open(&files, 0), open(&files, 1));
        let batch = ParticipantBatch {
            batch_id: "batch-1".into(),
            members: members(&files, &[&alpha, &beta], &[0, 1], "v1"),
        };
        let error =
            as_writer(|| with_crash(CrashPoint::AfterApply(0), || commit(&batch))).unwrap_err();
        assert!(injected(&error), "{error}");
    }
    let alpha = open(&files, 0);
    let error = recover(&[&alpha], "batch-1").unwrap_err();
    assert!(error.to_string().contains("participant set"), "{error}");
    assert!(
        counts(&alpha, 0)
            .unwrap_err()
            .to_string()
            .contains("unpublished")
    );
    // An unrelated owner supplied in place of the missing one does not help.
    let gamma = open(&files, 2);
    let error = recover(&[&alpha, &gamma], "batch-1").unwrap_err();
    assert!(error.to_string().contains("participant set"), "{error}");
    let beta = open(&files, 1);
    assert!(matches!(
        recover(&[&alpha, &beta], "batch-1").unwrap(),
        Recovery::Published(_)
    ));
}
