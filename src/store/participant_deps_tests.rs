//! Phase 3 S2b (aegis-n0wc.5): bounded cross-graph type dependencies. A
//! member's write may require "entity E has rdf:type T in graph G" where G
//! lives in another set file. G's owner joins as a validation-only
//! participant: fenced durably like any participant, checked under that fence
//! before any intent, never written.
use super::tests::*;
use super::*;
use crate::{
    store::Datum,
    types::{Op, Value as TermValue},
};

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const SHARED: &str = "urn:shared:action";
const ACTION: &str = "urn:type:Action";

/// Assert `entity a ty` into one gamma graph through gamma's ordinary owner.
fn seed_gamma(files: &Files, graph: usize, entity: &str, ty: &str) {
    let path = files.root.join("gamma.db");
    let (e, a, t) = {
        let store = Store::open(path.to_str().unwrap()).unwrap();
        (
            store.intern(entity).unwrap(),
            store.intern(RDF_TYPE).unwrap(),
            store.intern(ty).unwrap(),
        )
    };
    let gamma = open(files, 2);
    let mut batch = batch(files, 2, "seed");
    for (index, (_, datums)) in batch.graphs.iter_mut().enumerate() {
        *datums = if index == graph {
            vec![Datum {
                entity: e,
                attribute: a,
                value: TermValue::Ref(t),
                valid_from: "2026-01-01T00:00:00Z".into(),
                valid_to: None,
                op: Op::Assert,
            }]
        } else {
            Vec::new()
        };
    }
    gamma.transact(&requests(2), &batch).unwrap();
}

fn check(graph: &str) -> TypeDependency {
    TypeDependency {
        graph: graph.into(),
        entity: SHARED.into(),
        type_iri: ACTION.into(),
    }
}

fn data_graph(index: usize) -> &'static str {
    FILES[index].1[0]
}

#[test]
fn a_present_permitted_type_admits_the_batch_and_the_reference_file_is_never_written() {
    let temp = tempfile::tempdir().unwrap();
    let files = files(temp.path());
    seed_gamma(&files, 0, SHARED, ACTION);
    let gamma_head = raw_head(&files, 2);
    let (alpha, beta, gamma) = (open(&files, 0), open(&files, 1), open(&files, 2));
    let batch = ParticipantBatch {
        batch_id: "batch-dep".into(),
        members: members(&files, &[&alpha, &beta], &[0, 1], "v1"),
    };
    let validations = [Validation {
        owner: &gamma,
        absences: Vec::new(),
        checks: vec![check(data_graph(2))],
    }];
    let vector = as_writer(|| commit_validated(&batch, &validations)).unwrap();
    assert_eq!(
        vector
            .members
            .iter()
            .map(|m| (m.file_id.as_str(), m.heads.len()))
            .collect::<Vec<_>>(),
        [("alpha", 2), ("beta", 2), ("gamma", 0)]
    );
    assert_eq!(raw_head(&files, 2), gamma_head);
    assert_eq!(counts(&alpha, 0).unwrap(), [1, 1]);
    assert_eq!(counts(&beta, 1).unwrap(), [1, 1]);
    // The validation fence is released on publication.
    local_write(&files, &gamma, 2, "after").unwrap();
    // A retry is the same batch only with the same dependencies.
    assert_eq!(
        as_writer(|| commit_validated(&batch, &validations)).unwrap(),
        vector
    );
    let error = as_writer(|| commit(&batch)).unwrap_err();
    assert!(error.to_string().contains("different batch"), "{error}");
}

#[test]
fn an_absent_or_misplaced_type_refuses_with_nothing_prepared() {
    let temp = tempfile::tempdir().unwrap();
    let files = files(temp.path());
    // The type exists, but only in gamma's OTHER graph.
    seed_gamma(&files, 1, SHARED, ACTION);
    let heads: Vec<_> = (0..3).map(|i| raw_head(&files, i)).collect();
    let (alpha, beta, gamma) = (open(&files, 0), open(&files, 1), open(&files, 2));
    let batch = ParticipantBatch {
        batch_id: "batch-dep".into(),
        members: members(&files, &[&alpha, &beta], &[0, 1], "v1"),
    };
    let cases = [
        // Absent from the declared graph (present in an undeclared one).
        check(data_graph(2)),
        // A graph the validation owner does not own.
        check(data_graph(0)),
        // A graph no catalog owner routes.
        check("urn:unselected:graph"),
    ];
    for dependency in cases {
        let validations = [Validation {
            owner: &gamma,
            absences: Vec::new(),
            checks: vec![dependency],
        }];
        let error = as_writer(|| commit_validated(&batch, &validations)).unwrap_err();
        assert!(error.to_string().contains("dependency"), "{error}");
    }
    assert_eq!(
        (0..3).map(|i| raw_head(&files, i)).collect::<Vec<_>>(),
        heads
    );
    // Nothing prepared or consumed: every file writes, and the id still works.
    for (owner, index) in [(&alpha, 0), (&beta, 1), (&gamma, 2)] {
        local_write(&files, owner, index, "other").unwrap();
    }
    as_writer(|| commit(&batch)).unwrap();
}

#[test]
fn a_crashed_validated_batch_fences_the_reference_file_until_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let files = files(temp.path());
    seed_gamma(&files, 0, SHARED, ACTION);
    {
        let (alpha, beta, gamma) = (open(&files, 0), open(&files, 1), open(&files, 2));
        let batch = ParticipantBatch {
            batch_id: "batch-dep".into(),
            members: members(&files, &[&alpha, &beta], &[0, 1], "v1"),
        };
        let validations = [Validation {
            owner: &gamma,
            absences: Vec::new(),
            checks: vec![check(data_graph(2))],
        }];
        let error = as_writer(|| {
            with_crash(CrashPoint::AfterDecision, || {
                commit_validated(&batch, &validations)
            })
        })
        .unwrap_err();
        assert!(injected(&error), "{error}");
    }
    let (alpha, beta, gamma) = (open(&files, 0), open(&files, 1), open(&files, 2));
    // A write that could invalidate the checked type cannot slip in.
    let error = local_write(&files, &gamma, 2, "invalidate").unwrap_err();
    assert!(error.to_string().contains("participant fenced"), "{error}");
    // Recovery needs the validation participant too.
    let error = recover(&[&alpha, &beta], "batch-dep").unwrap_err();
    assert!(error.to_string().contains("participant set"), "{error}");
    let Recovery::Published(vector) = recover(&[&alpha, &beta, &gamma], "batch-dep").unwrap()
    else {
        panic!("a committed validated batch recovered as an abort");
    };
    assert_eq!(vector.members.len(), 3);
    assert_eq!(counts(&beta, 1).unwrap(), [1, 1]);
    local_write(&files, &gamma, 2, "after").unwrap();
}

#[test]
fn unbounded_or_ambiguous_dependencies_refuse_before_any_fence() {
    let temp = tempfile::tempdir().unwrap();
    let files = files(temp.path());
    seed_gamma(&files, 0, SHARED, ACTION);
    let (alpha, beta, gamma) = (open(&files, 0), open(&files, 1), open(&files, 2));
    let batch = ParticipantBatch {
        batch_id: "batch-dep".into(),
        members: members(&files, &[&alpha, &beta], &[0, 1], "v1"),
    };
    let over = [Validation {
        owner: &gamma,
        absences: Vec::new(),
        checks: (0..=MAX_DEPENDENCY_CHECKS)
            .map(|_| check(data_graph(2)))
            .collect(),
    }];
    let error = as_writer(|| commit_validated(&batch, &over)).unwrap_err();
    assert!(error.to_string().contains("bound"), "{error}");
    let empty = [Validation {
        owner: &gamma,
        absences: Vec::new(),
        checks: Vec::new(),
    }];
    let error = as_writer(|| commit_validated(&batch, &empty)).unwrap_err();
    assert!(error.to_string().contains("dependency"), "{error}");
    // A member cannot also be its own validation participant.
    let member = [Validation {
        owner: &alpha,
        absences: Vec::new(),
        checks: vec![check(data_graph(0))],
    }];
    let error = as_writer(|| commit_validated(&batch, &member)).unwrap_err();
    assert!(error.to_string().contains("duplicate"), "{error}");
    // A relative or blank IRI is not an exact dependency.
    let relative = [Validation {
        owner: &gamma,
        absences: Vec::new(),
        checks: vec![TypeDependency {
            graph: data_graph(2).into(),
            entity: "shared".into(),
            type_iri: ACTION.into(),
        }],
    }];
    let error = as_writer(|| commit_validated(&batch, &relative)).unwrap_err();
    assert!(error.to_string().contains("exact IRI"), "{error}");
    as_writer(|| commit(&batch)).unwrap();
}
