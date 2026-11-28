//! A member violation is a PREPARE refusal, before any durable decision/fence.
//! The same test can run on the pre-validation participant implementation.
use super::*;
use crate::types::{Op, Value as TermValue};

const CLASS: &str = "urn:b0:Action";
const VALUE: &str = "urn:b0:untyped";
const PREDICATE: &str = "urn:b0:blockedOn";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const SHAPES: &str = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
<urn:b0:shape> a sh:NodeShape ; sh:targetClass <urn:b0:Action> ;
  sh:property [ sh:path <urn:b0:blockedOn> ; sh:class <urn:b0:Action> ] .
"#;

/// Read-only inspection, intentionally independent of owner read fences.
fn durable_states(files: &Files, index: usize, batch_id: &str) -> Vec<(String, String)> {
    let path = files.root.join(format!("{}.db", FILES[index].0));
    let conn =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let mut states = Vec::new();
    for table in ["participant_journal", "participant_intents"] {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        if exists {
            let mut statement = conn
                .prepare(&format!("SELECT state FROM {table} WHERE batch_id=?1"))
                .unwrap();
            for state in statement
                .query_map([batch_id], |r| r.get::<_, String>(0))
                .unwrap()
            {
                states.push((table.into(), state.unwrap()));
            }
        }
    }
    states
}

fn shaped_owner(files: &Files, index: usize) -> GraphSetOwner {
    let path = files.root.join(format!("{}.db", FILES[index].0));
    let mut store = Store::open(path.to_str().unwrap()).unwrap();
    store.shacl_config_mut().validate_on_write = true;
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

fn check_bad_member(bad: usize) {
    let temp = tempfile::tempdir().unwrap();
    let files = files(temp.path());
    let mut terms = Vec::new();
    for (file, _) in FILES {
        let path = files.root.join(format!("{file}.db"));
        let mut store = Store::open(path.to_str().unwrap()).unwrap();
        store
            .load_shapes("b0", SHAPES, "2026-01-01T00:00:00Z")
            .unwrap();
        terms.push([RDF_TYPE, CLASS, PREDICATE, VALUE].map(|iri| store.intern(iri).unwrap()));
    }
    let owners = [
        shaped_owner(&files, 0),
        shaped_owner(&files, 1),
        shaped_owner(&files, 2),
    ];
    let before: Vec<_> = (0..3).map(|i| raw_head(&files, i)).collect();
    let [rdf_type, class, predicate, value] = terms[bad];
    let datum = |attribute, value| Datum {
        entity: files.terms[bad].0,
        attribute,
        value: TermValue::Ref(value),
        valid_from: "2026-01-01T00:00:00Z".into(),
        valid_to: None,
        op: Op::Assert,
    };
    let mut invalid = batch(&files, bad, "v1");
    invalid.graphs[0].1 = vec![datum(rdf_type, class), datum(predicate, value)];
    invalid.graphs[1].1.clear();
    let id = format!("b0-member-{bad}");
    let mut invalid = Some(invalid);
    let members = (0..2)
        .map(|index| Member {
            owner: &owners[index],
            requests: requests(index),
            batch: if index == bad {
                invalid.take().unwrap()
            } else {
                batch(&files, index, "v1")
            },
        })
        .collect();
    let rejected = as_writer(|| {
        commit(&ParticipantBatch {
            batch_id: id.clone(),
            members,
        })
    })
    .unwrap_err();
    let after: Vec<_> = (0..3).map(|i| raw_head(&files, i)).collect();
    let states: Vec<_> = (0..3).map(|i| durable_states(&files, i, &id)).collect();
    let recovery = as_writer(|| recover(&[&owners[0], &owners[1]], &id));
    let writes: Vec<_> = (0..3)
        .map(|i| local_write(&files, &owners[i], i, "after-refusal"))
        .collect();
    let retry = as_writer(|| {
        commit(&ParticipantBatch {
            batch_id: id.clone(),
            members: (0..2)
                .map(|index| Member {
                    owner: &owners[index],
                    requests: requests(index),
                    batch: batch(&files, index, "corrected"),
                })
                .collect(),
        })
    });
    eprintln!(
        "B0 bad={bad}: refusal={rejected}; before={before:?}; after={after:?}; states={states:?}; recovery={recovery:?}; ordinary_writes={writes:?}; same_id_corrected={retry:?}"
    );
    assert!(
        rejected.to_string().contains("prepare"),
        "violation surfaced after PREPARE: {rejected}"
    );
    assert_eq!(after, before, "refused validation committed member facts");
    assert!(
        states.iter().all(Vec::is_empty),
        "refusal consumed an id or left a durable fence: {states:?}"
    );
    assert!(
        recovery
            .unwrap_err()
            .to_string()
            .contains("no participant intent or journal")
    );
    assert!(
        writes.iter().all(Result::is_ok),
        "ordinary writer still fenced: {writes:?}"
    );
    assert_eq!(
        retry.unwrap().batch_id,
        id,
        "pre-prepare refusal consumed the batch id"
    );
}

#[test]
fn b0_member_violation_first_refuses_before_commit_and_releases_fences() {
    check_bad_member(0);
}
#[test]
fn b0_member_violation_last_refuses_before_any_earlier_member_commits() {
    check_bad_member(1);
}
