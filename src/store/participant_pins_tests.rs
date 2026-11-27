//! Phase 3 S4 (aegis-n0wc.5): inverse invalidation of later writes to a
//! dependency graph. A validated batch pins "E a T in G" in G's file, one pin
//! per dependent member file. Any later write into G that removes that type
//! refuses unless it is a participant batch that enlists every pinning file
//! with a reverse check proving the file no longer references E.
use super::tests::*;
use super::*;
use crate::{
    store::Datum,
    types::{Op, Value as TermValue},
};

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const SHARED: &str = "urn:shared:action";
const OTHER: &str = "urn:shared:other";
const ACTION: &str = "urn:type:Action";
const TASK: &str = "urn:type:Task";

fn data_graph(index: usize) -> &'static str {
    FILES[index].1[0]
}

thread_local! {
    static TERMS: std::cell::RefCell<std::collections::HashMap<(usize, String), i64>> =
        Default::default();
}

/// The fixture files with every IRI these rows use interned in every file,
/// BEFORE any owner opens them.
fn fixture(root: &Path) -> Files {
    let files = files(root);
    for (index, (file, _)) in FILES.iter().enumerate() {
        let store = Store::open(files.root.join(format!("{file}.db")).to_str().unwrap()).unwrap();
        for iri in [RDF_TYPE, SHARED, OTHER, ACTION, TASK, "urn:value"] {
            let id = store.intern(iri).unwrap();
            TERMS.with(|t| t.borrow_mut().insert((index, iri.to_owned()), id));
        }
    }
    files
}

fn term(_files: &Files, index: usize, iri: &str) -> i64 {
    TERMS.with(|t| t.borrow()[&(index, iri.to_owned())])
}

/// A batch for file `index` writing `datums` into its data graph only.
fn data_batch(files: &Files, index: usize, datums: Vec<Datum>) -> GraphSetBatch {
    let mut batch = batch(files, index, "unused");
    let mut datums = Some(datums);
    for (_, slot) in &mut batch.graphs {
        *slot = datums.take().unwrap_or_default();
    }
    batch
}

fn datum(entity: i64, attribute: i64, value: TermValue, op: Op, valid_to: Option<&str>) -> Datum {
    Datum {
        entity,
        attribute,
        value,
        valid_from: "2026-01-01T00:00:00Z".into(),
        valid_to: valid_to.map(str::to_owned),
        op,
    }
}

/// "`entity` a `ty`" in gamma's data graph, as a datum in gamma's term space.
fn gamma_type(files: &Files, entity: &str, ty: &str, op: Op, valid_to: Option<&str>) -> Datum {
    datum(
        term(files, 2, entity),
        term(files, 2, RDF_TYPE),
        TermValue::Ref(term(files, 2, ty)),
        op,
        valid_to,
    )
}

fn gamma_write(files: &Files, gamma: &GraphSetOwner, datums: Vec<Datum>) -> Result<()> {
    gamma
        .transact(&requests(2), &data_batch(files, 2, datums))
        .map(|_| ())
}

/// A member of file `index` whose data graph references SHARED as a value.
fn referencing_member<'a>(files: &Files, owner: &'a GraphSetOwner, index: usize) -> Member<'a> {
    let (item, attribute) = files.terms[index];
    let shared = term(files, index, SHARED);
    Member {
        owner,
        requests: requests(index),
        batch: data_batch(
            files,
            index,
            vec![datum(
                item,
                attribute,
                TermValue::Ref(shared),
                Op::Assert,
                None,
            )],
        ),
    }
}

fn check() -> TypeDependency {
    TypeDependency {
        graph: data_graph(2).into(),
        entity: SHARED.into(),
        type_iri: ACTION.into(),
    }
}

fn pin(file: &str) -> DependencyPin {
    DependencyPin {
        graph: data_graph(2).into(),
        entity: SHARED.into(),
        type_iri: ACTION.into(),
        dependent_file_id: file.into(),
    }
}

/// Seed "SHARED a Action" and "OTHER a Action" in gamma, then commit the
/// validated alpha+beta batch where both reference SHARED.
fn pinned(files: &Files) -> (GraphSetOwner, GraphSetOwner, GraphSetOwner) {
    let (alpha, beta, gamma) = (open(files, 0), open(files, 1), open(files, 2));
    gamma_write(
        files,
        &gamma,
        vec![
            gamma_type(files, SHARED, ACTION, Op::Assert, None),
            gamma_type(files, OTHER, ACTION, Op::Assert, None),
        ],
    )
    .unwrap();
    let batch = ParticipantBatch {
        batch_id: "batch-dep".into(),
        members: vec![
            referencing_member(files, &alpha, 0),
            referencing_member(files, &beta, 1),
        ],
    };
    let validations = [Validation {
        owner: &gamma,
        checks: vec![check()],
        absences: Vec::new(),
    }];
    as_writer(|| commit_validated(&batch, &validations)).unwrap();
    (alpha, beta, gamma)
}

#[test]
fn a_validated_batch_pins_the_type_once_per_dependent_file_and_a_refused_one_pins_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let files = fixture(temp.path());
    let (alpha, beta, gamma) = pinned(&files);
    assert_eq!(
        dependency_pins(&gamma).unwrap(),
        [pin("alpha"), pin("beta")]
    );
    // Pins are metadata: members hold none, and gamma's head did not move.
    assert!(dependency_pins(&alpha).unwrap().is_empty());
    assert!(dependency_pins(&beta).unwrap().is_empty());
    // A second batch relying on the same type does not duplicate the pins.
    let again = ParticipantBatch {
        batch_id: "batch-dep-2".into(),
        members: vec![
            referencing_member(&files, &alpha, 0),
            referencing_member(&files, &beta, 1),
        ],
    };
    let validations = [Validation {
        owner: &gamma,
        checks: vec![check()],
        absences: Vec::new(),
    }];
    as_writer(|| commit_validated(&again, &validations)).unwrap();
    assert_eq!(
        dependency_pins(&gamma).unwrap(),
        [pin("alpha"), pin("beta")]
    );
    // A dependency that does not hold refuses and pins nothing.
    let absent = [Validation {
        owner: &gamma,
        checks: vec![TypeDependency {
            type_iri: TASK.into(),
            ..check()
        }],
        absences: Vec::new(),
    }];
    let refused = ParticipantBatch {
        batch_id: "batch-dep-3".into(),
        members: vec![
            referencing_member(&files, &alpha, 0),
            referencing_member(&files, &beta, 1),
        ],
    };
    let error = as_writer(|| commit_validated(&refused, &absent)).unwrap_err();
    assert!(error.to_string().contains("dependency"), "{error}");
    assert_eq!(
        dependency_pins(&gamma).unwrap(),
        [pin("alpha"), pin("beta")]
    );
}

#[test]
fn an_ordinary_write_that_removes_a_pinned_type_refuses_and_writes_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let files = fixture(temp.path());
    let (_alpha, _beta, gamma) = pinned(&files);
    let head = raw_head(&files, 2);
    let cases = [
        // Retract the pinned fact.
        gamma_type(&files, SHARED, ACTION, Op::Retract, None),
        // Close its valid time.
        gamma_type(
            &files,
            SHARED,
            ACTION,
            Op::Assert,
            Some("2026-06-01T00:00:00Z"),
        ),
    ];
    for removal in cases {
        let error = gamma_write(&files, &gamma, vec![removal]).unwrap_err();
        assert!(error.to_string().contains("pinned"), "{error}");
        assert_eq!(raw_head(&files, 2), head);
    }
    assert_eq!(
        dependency_pins(&gamma).unwrap(),
        [pin("alpha"), pin("beta")]
    );
}

#[test]
fn writes_that_leave_the_pinned_type_in_place_commit() {
    let temp = tempfile::tempdir().unwrap();
    let files = fixture(temp.path());
    let (_alpha, _beta, gamma) = pinned(&files);
    // Another entity's identical type is not pinned.
    gamma_write(
        &files,
        &gamma,
        vec![gamma_type(&files, OTHER, ACTION, Op::Retract, None)],
    )
    .unwrap();
    // Another type of the pinned entity.
    gamma_write(
        &files,
        &gamma,
        vec![gamma_type(&files, SHARED, TASK, Op::Assert, None)],
    )
    .unwrap();
    gamma_write(
        &files,
        &gamma,
        vec![gamma_type(&files, SHARED, TASK, Op::Retract, None)],
    )
    .unwrap();
    // Another attribute of the pinned entity.
    let label = datum(
        term(&files, 2, SHARED),
        term(&files, 2, "urn:value"),
        TermValue::Str("label".into()),
        Op::Assert,
        None,
    );
    gamma_write(&files, &gamma, vec![label]).unwrap();
    // The ordinary fixture write (another entity in both graphs).
    local_write(&files, &gamma, 2, "after").unwrap();
    assert_eq!(
        dependency_pins(&gamma).unwrap(),
        [pin("alpha"), pin("beta")]
    );
}

fn removal_member<'a>(files: &Files, gamma: &'a GraphSetOwner) -> Member<'a> {
    Member {
        owner: gamma,
        requests: requests(2),
        batch: data_batch(
            files,
            2,
            vec![gamma_type(files, SHARED, ACTION, Op::Retract, None)],
        ),
    }
}

fn retract_reference(files: &Files, owner: &GraphSetOwner, index: usize) {
    let (item, attribute) = files.terms[index];
    let shared = term(files, index, SHARED);
    owner
        .transact(
            &requests(index),
            &data_batch(
                files,
                index,
                vec![datum(
                    item,
                    attribute,
                    TermValue::Ref(shared),
                    Op::Retract,
                    None,
                )],
            ),
        )
        .unwrap();
}

#[test]
fn a_member_batch_removing_a_pinned_type_must_enlist_every_pinning_file() {
    let temp = tempfile::tempdir().unwrap();
    let files = fixture(temp.path());
    let (alpha, beta, gamma) = pinned(&files);
    retract_reference(&files, &alpha, 0);
    retract_reference(&files, &beta, 1);
    let heads: Vec<_> = (0..3).map(|i| raw_head(&files, i)).collect();
    // gamma + alpha as members; beta pins too but is not enlisted.
    let (item, attribute) = files.terms[0];
    let batch = ParticipantBatch {
        batch_id: "batch-release".into(),
        members: vec![
            removal_member(&files, &gamma),
            Member {
                owner: &alpha,
                requests: requests(0),
                batch: data_batch(
                    &files,
                    0,
                    vec![datum(
                        item,
                        attribute,
                        TermValue::Str("x".into()),
                        Op::Assert,
                        None,
                    )],
                ),
            },
        ],
    };
    let error = as_writer(|| commit(&batch)).unwrap_err();
    assert!(error.to_string().contains("pinned"), "{error}");
    assert_eq!(
        (0..3).map(|i| raw_head(&files, i)).collect::<Vec<_>>(),
        heads
    );
    // Nothing prepared: every file still writes.
    for (owner, index) in [(&alpha, 0), (&beta, 1), (&gamma, 2)] {
        local_write(&files, owner, index, "other").unwrap();
    }
}

#[test]
fn a_release_batch_with_every_reference_gone_changes_the_type_and_drops_the_pins() {
    let temp = tempfile::tempdir().unwrap();
    let files = fixture(temp.path());
    let (alpha, beta, gamma) = pinned(&files);
    assert_eq!(
        dependency_pins(&gamma).unwrap(),
        [pin("alpha"), pin("beta")]
    );
    // beta dropped its reference earlier; alpha drops its own IN the release
    // batch, atomically with gamma removing the type.
    retract_reference(&files, &beta, 1);
    let (item, attribute) = files.terms[0];
    let shared = term(&files, 0, SHARED);
    let batch = ParticipantBatch {
        batch_id: "batch-release".into(),
        members: vec![
            removal_member(&files, &gamma),
            Member {
                owner: &alpha,
                requests: requests(0),
                batch: data_batch(
                    &files,
                    0,
                    vec![datum(
                        item,
                        attribute,
                        TermValue::Ref(shared),
                        Op::Retract,
                        None,
                    )],
                ),
            },
        ],
    };
    let absence = || ReferenceAbsence {
        entity: SHARED.into(),
    };
    let heads: Vec<_> = (0..3).map(|i| raw_head(&files, i)).collect();
    // Proving only beta leaves alpha's pin standing: refuse, nothing prepared.
    let beta_only = [Validation {
        owner: &beta,
        checks: Vec::new(),
        absences: vec![absence()],
    }];
    let error = as_writer(|| commit_validated(&batch, &beta_only)).unwrap_err();
    assert!(
        error.to_string().contains("pinned by dependent file alpha"),
        "{error}"
    );
    assert_eq!(
        (0..3).map(|i| raw_head(&files, i)).collect::<Vec<_>>(),
        heads
    );
    // A member proves its absence on its state AFTER its own batch.
    let both = [
        Validation {
            owner: &alpha,
            checks: Vec::new(),
            absences: vec![absence()],
        },
        Validation {
            owner: &beta,
            checks: Vec::new(),
            absences: vec![absence()],
        },
    ];
    let vector = as_writer(|| commit_validated(&batch, &both)).unwrap();
    assert!(dependency_pins(&gamma).unwrap().is_empty());
    // Only alpha and gamma were written; beta was fenced and checked only.
    assert_eq!(raw_head(&files, 1), heads[1]);
    assert_eq!(
        as_writer(|| commit_validated(&batch, &both)).unwrap(),
        vector
    );
    // The type is gone, and the next ordinary write into gamma is unpinned.
    gamma_write(
        &files,
        &gamma,
        vec![gamma_type(&files, SHARED, ACTION, Op::Assert, None)],
    )
    .unwrap();
    gamma_write(
        &files,
        &gamma,
        vec![gamma_type(&files, SHARED, ACTION, Op::Retract, None)],
    )
    .unwrap();
}

#[test]
fn a_member_cannot_prove_absence_while_its_own_batch_adds_a_reference() {
    let temp = tempfile::tempdir().unwrap();
    let files = fixture(temp.path());
    let (alpha, beta, gamma) = pinned(&files);
    retract_reference(&files, &alpha, 0);
    retract_reference(&files, &beta, 1);
    let heads: Vec<_> = (0..3).map(|i| raw_head(&files, i)).collect();
    // alpha's member batch re-adds `item -> SHARED` while claiming absence.
    let batch = ParticipantBatch {
        batch_id: "batch-release".into(),
        members: vec![
            removal_member(&files, &gamma),
            referencing_member(&files, &alpha, 0),
        ],
    };
    let absence = || ReferenceAbsence {
        entity: SHARED.into(),
    };
    let validations = [
        Validation {
            owner: &alpha,
            checks: Vec::new(),
            absences: vec![absence()],
        },
        Validation {
            owner: &beta,
            checks: Vec::new(),
            absences: vec![absence()],
        },
    ];
    let error = as_writer(|| commit_validated(&batch, &validations)).unwrap_err();
    assert!(error.to_string().contains("references"), "{error}");
    assert_eq!(
        (0..3).map(|i| raw_head(&files, i)).collect::<Vec<_>>(),
        heads
    );
    assert_eq!(
        dependency_pins(&gamma).unwrap(),
        [pin("alpha"), pin("beta")]
    );
}

#[test]
fn a_release_batch_refuses_while_any_pinning_file_still_references_the_entity() {
    let temp = tempfile::tempdir().unwrap();
    let files = fixture(temp.path());
    let (alpha, beta, gamma) = pinned(&files);
    retract_reference(&files, &alpha, 0);
    // beta still holds `item -> SHARED` (an object reference). Separately,
    // alpha gains SHARED as a SUBJECT.
    let shared_alpha = term(&files, 0, SHARED);
    let (_, attribute) = files.terms[0];
    alpha
        .transact(
            &requests(0),
            &data_batch(
                &files,
                0,
                vec![datum(
                    shared_alpha,
                    attribute,
                    TermValue::Str("subject".into()),
                    Op::Assert,
                    None,
                )],
            ),
        )
        .unwrap();
    let heads: Vec<_> = (0..3).map(|i| raw_head(&files, i)).collect();
    let absence = || ReferenceAbsence {
        entity: SHARED.into(),
    };
    for (member_index, validation_owner) in [(0usize, &beta), (1usize, &alpha)] {
        let member_owner = if member_index == 0 { &alpha } else { &beta };
        let (item, attr) = files.terms[member_index];
        let batch = ParticipantBatch {
            batch_id: format!("batch-release-{member_index}"),
            members: vec![
                removal_member(&files, &gamma),
                Member {
                    owner: member_owner,
                    requests: requests(member_index),
                    batch: data_batch(
                        &files,
                        member_index,
                        vec![datum(
                            item,
                            attr,
                            TermValue::Str("x".into()),
                            Op::Assert,
                            None,
                        )],
                    ),
                },
            ],
        };
        let validations = [Validation {
            owner: validation_owner,
            checks: Vec::new(),
            absences: vec![absence()],
        }];
        let error = as_writer(|| commit_validated(&batch, &validations)).unwrap_err();
        assert!(error.to_string().contains("references"), "{error}");
    }
    assert_eq!(
        (0..3).map(|i| raw_head(&files, i)).collect::<Vec<_>>(),
        heads
    );
    assert_eq!(
        dependency_pins(&gamma).unwrap(),
        [pin("alpha"), pin("beta")]
    );
}

#[test]
fn pins_follow_the_batch_decision_through_a_crash() {
    let temp = tempfile::tempdir().unwrap();
    let files = fixture(temp.path());
    let seed = |gamma: &GraphSetOwner| {
        gamma_write(
            &files,
            gamma,
            vec![gamma_type(&files, SHARED, ACTION, Op::Assert, None)],
        )
        .unwrap();
    };
    seed(&open(&files, 2));
    let run = |id: &str, point: CrashPoint| {
        let (alpha, beta, gamma) = (open(&files, 0), open(&files, 1), open(&files, 2));
        let batch = ParticipantBatch {
            batch_id: id.into(),
            members: vec![
                referencing_member(&files, &alpha, 0),
                referencing_member(&files, &beta, 1),
            ],
        };
        let validations = [Validation {
            owner: &gamma,
            checks: vec![check()],
            absences: Vec::new(),
        }];
        let error =
            as_writer(|| with_crash(point, || commit_validated(&batch, &validations))).unwrap_err();
        assert!(injected(&error), "{error}");
    };
    // Crash before the decision: recovery aborts and no pin exists.
    run("batch-abort", CrashPoint::AfterPrepare(2));
    let (alpha, beta, gamma) = (open(&files, 0), open(&files, 1), open(&files, 2));
    assert!(matches!(
        recover(&[&alpha, &beta, &gamma], "batch-abort").unwrap(),
        Recovery::Aborted
    ));
    assert!(dependency_pins(&gamma).unwrap().is_empty());
    drop((alpha, beta, gamma));
    // Crash after the decision: recovery publishes and pins exactly once.
    run("batch-publish", CrashPoint::AfterDecision);
    let (alpha, beta, gamma) = (open(&files, 0), open(&files, 1), open(&files, 2));
    let Recovery::Published(vector) = recover(&[&alpha, &beta, &gamma], "batch-publish").unwrap()
    else {
        panic!("a committed validated batch recovered as an abort");
    };
    assert_eq!(
        dependency_pins(&gamma).unwrap(),
        [pin("alpha"), pin("beta")]
    );
    let Recovery::Published(again) = recover(&[&alpha, &beta, &gamma], "batch-publish").unwrap()
    else {
        panic!("a published batch recovered as an abort");
    };
    assert_eq!(again, vector);
    assert_eq!(
        dependency_pins(&gamma).unwrap(),
        [pin("alpha"), pin("beta")]
    );
}
