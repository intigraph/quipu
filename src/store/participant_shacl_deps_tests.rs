//! Phase 3 slice B (aegis-n0wc.5): SHACL-derived type dependencies. A member
//! whose pinned shapes need `sh:class` on a value typed only in ANOTHER set
//! file may enlist that file's owner as a type source. The type triples (and
//! every rdfs:subClassOf axiom the satisfaction relies on) are read ONLY from
//! enlisted sources, under their fences, at PREPARE; each becomes a derived
//! dependency checked and pinned exactly like a declared one (S2b/S4).
//! Unenlisted files and ROOT are never implicit sources. Repair is subset-only:
//! the derived context can remove a member's violation, never add one.
use super::tests::*;
use super::*;
use crate::{
    store::Datum,
    types::{Op, Value as TermValue},
};

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const SUBCLASS_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
const ITEM: &str = "urn:item";
const BLOCKED_ON: &str = "urn:p:blockedOn";
const NAME: &str = "urn:p:name";
const SHARED: &str = "urn:shared:action";
const ACTION: &str = "urn:type:Action";
const SUB_ACTION: &str = "urn:type:SubAction";
const OTHER: &str = "urn:type:Other";
const OTHER_SHARED: &str = "urn:shared:other-action";

/// Alpha's pinned shapes: an Action's blockedOn values must be Actions, and
/// any Other needs a name (the subset-only discriminator).
const SHAPES: &str = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
<urn:shape:Action> a sh:NodeShape ; sh:targetClass <urn:type:Action> ;
  sh:property [ sh:path <urn:p:blockedOn> ; sh:class <urn:type:Action> ] .
<urn:shape:Other> a sh:NodeShape ; sh:targetClass <urn:type:Other> ;
  sh:property [ sh:path <urn:p:name> ; sh:minCount 1 ] .
"#;

fn path(files: &Files, index: usize) -> std::path::PathBuf {
    files.root.join(format!("{}.db", FILES[index].0))
}

thread_local! {
    /// Per file, every fixture IRI's local term id. Interned when the fixture
    /// is built, because an active owner's writer lease refuses a second open.
    /// Each test runs on its own thread.
    static TERMS: std::cell::RefCell<Vec<std::collections::BTreeMap<String, i64>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn fixture_iris() -> Vec<String> {
    let mut iris: Vec<String> = [
        RDF_TYPE,
        SUBCLASS_OF,
        ITEM,
        BLOCKED_ON,
        NAME,
        SHARED,
        ACTION,
        SUB_ACTION,
        OTHER,
        OTHER_SHARED,
    ]
    .iter()
    .map(|iri| (*iri).to_owned())
    .collect();
    iris.extend(many());
    iris
}

fn many() -> Vec<String> {
    (0..=MAX_DEPENDENCY_CHECKS)
        .map(|i| format!("urn:shared:action-{i}"))
        .collect()
}

/// The fixture files, with SHAPES loaded into alpha only.
fn shacl_files(root: &Path) -> Files {
    let files = files(root);
    let mut terms = Vec::new();
    for index in 0..3 {
        let mut store = Store::open(path(&files, index).to_str().unwrap()).unwrap();
        if index == 0 {
            store
                .load_shapes("slice-b", SHAPES, "2026-01-01T00:00:00Z")
                .unwrap();
        }
        terms.push(
            fixture_iris()
                .into_iter()
                .map(|iri| {
                    let id = store.intern(&iri).unwrap();
                    (iri, id)
                })
                .collect(),
        );
    }
    TERMS.with(|t| *t.borrow_mut() = terms);
    files
}

/// (Re)open one owner with write SHACL on, as a configured server would.
fn open_shacl(files: &Files, index: usize) -> GraphSetOwner {
    let mut store = Store::open(path(files, index).to_str().unwrap()).unwrap();
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

/// `subject predicate object` as a native datum in file `index`'s terms.
fn triple(_files: &Files, index: usize, s: &str, p: &str, o: &str, op: Op) -> Datum {
    let id = |iri: &str| TERMS.with(|t| t.borrow()[index][iri]);
    Datum {
        entity: id(s),
        attribute: id(p),
        value: TermValue::Ref(id(o)),
        valid_from: "2026-01-01T00:00:00Z".into(),
        valid_to: None,
        op,
    }
}

/// A batch for file `index` writing `datums` into graph `graph` (0 = data,
/// 1 = provenance) and nothing into the other.
fn graph_batch(files: &Files, index: usize, graph: usize, datums: Vec<Datum>) -> GraphSetBatch {
    let mut batch = batch(files, index, "unused");
    for (at, (_, rows)) in batch.graphs.iter_mut().enumerate() {
        *rows = if at == graph {
            datums.clone()
        } else {
            Vec::new()
        };
    }
    batch
}

/// Seed facts into one file through its plain ordinary owner (no shapes).
fn seed(files: &Files, index: usize, graph: usize, rows: &[(&str, &str, &str)]) {
    let datums: Vec<_> = rows
        .iter()
        .map(|(s, p, o)| triple(files, index, s, p, o, Op::Assert))
        .collect();
    let owner = open(files, index);
    owner
        .transact(&requests(index), &graph_batch(files, index, graph, datums))
        .unwrap();
}

/// Alpha's member payload: `urn:item a Action ; blockedOn <each value>`, plus
/// any extra local rows.
fn alpha_member<'a>(
    files: &Files,
    owner: &'a GraphSetOwner,
    values: &[&str],
    extra: &[(&str, &str, &str)],
) -> Member<'a> {
    let mut datums = vec![triple(files, 0, ITEM, RDF_TYPE, ACTION, Op::Assert)];
    datums.extend(
        values
            .iter()
            .map(|v| triple(files, 0, ITEM, BLOCKED_ON, v, Op::Assert)),
    );
    datums.extend(
        extra
            .iter()
            .map(|(s, p, o)| triple(files, 0, s, p, o, Op::Assert)),
    );
    Member {
        owner,
        requests: requests(0),
        batch: graph_batch(files, 0, 0, datums),
    }
}

/// A plain conforming member for file `index` (no shapes there).
fn plain_member<'a>(files: &Files, owner: &'a GraphSetOwner, index: usize) -> Member<'a> {
    Member {
        owner,
        requests: requests(index),
        batch: batch(files, index, "v1"),
    }
}

fn type_pin(graph: &str, entity: &str, ty: &str, dependent: &str) -> DependencyPin {
    DependencyPin {
        graph: graph.into(),
        entity: entity.into(),
        type_iri: ty.into(),
        dependent_file_id: dependent.into(),
    }
}

fn data(index: usize) -> &'static str {
    FILES[index].1[0]
}

fn provenance(index: usize) -> &'static str {
    FILES[index].1[1]
}

fn heads(files: &Files) -> Vec<i64> {
    (0..3).map(|i| raw_head(files, i)).collect()
}

/// Nothing prepared or consumed: every file still takes an ordinary write.
fn assert_nothing_prepared(files: &Files, owners: [&GraphSetOwner; 3], before: &[i64]) {
    assert_eq!(heads(files), before, "a refused batch moved a head");
    for (index, owner) in owners.into_iter().enumerate() {
        local_write(files, owner, index, "after-refusal").unwrap();
    }
}

/// An ordinary write into one graph of `index` that retracts one triple.
fn retract(
    files: &Files,
    owner: &GraphSetOwner,
    index: usize,
    graph: usize,
    (s, p, o): (&str, &str, &str),
) -> Result<()> {
    owner
        .transact(
            &requests(index),
            &graph_batch(
                files,
                index,
                graph,
                vec![triple(files, index, s, p, o, Op::Retract)],
            ),
        )
        .map(|_| ())
}

#[test]
fn b0_a_cross_file_class_value_without_a_source_refuses_before_any_intent() {
    // Today's premise (c63/c65), made explicit: alpha's local gate cannot see
    // gamma's type, and the refusal must come at PREPARE, not after COMMIT.
    let temp = tempfile::tempdir().unwrap();
    let files = shacl_files(temp.path());
    seed(&files, 2, 0, &[(SHARED, RDF_TYPE, ACTION)]);
    let before = heads(&files);
    let (alpha, beta, gamma) = (open_shacl(&files, 0), open(&files, 1), open(&files, 2));
    let batch = ParticipantBatch {
        batch_id: "batch-b0".into(),
        members: vec![
            alpha_member(&files, &alpha, &[SHARED], &[]),
            plain_member(&files, &beta, 1),
        ],
    };
    let error = as_writer(|| commit(&batch)).unwrap_err();
    assert!(error.to_string().contains(SHARED), "{error}");
    assert_nothing_prepared(&files, [&alpha, &beta, &gamma], &before);
}

#[test]
fn b1_a_value_typed_only_in_an_enlisted_source_commits_and_pins_without_a_declaration() {
    let temp = tempfile::tempdir().unwrap();
    let files = shacl_files(temp.path());
    seed(&files, 2, 0, &[(SHARED, RDF_TYPE, ACTION)]);
    let gamma_head = raw_head(&files, 2);
    let (alpha, beta, gamma) = (open_shacl(&files, 0), open(&files, 1), open(&files, 2));
    let batch = ParticipantBatch {
        batch_id: "batch-b1".into(),
        members: vec![
            alpha_member(&files, &alpha, &[SHARED], &[]),
            plain_member(&files, &beta, 1),
        ],
    };
    let vector = as_writer(|| commit_derived(&batch, &[], &[&gamma])).unwrap();
    assert_eq!(
        vector
            .members
            .iter()
            .map(|m| m.file_id.as_str())
            .collect::<Vec<_>>(),
        ["alpha", "beta", "gamma"]
    );
    // Only the member whose shapes needed the type depends on it.
    assert_eq!(
        dependency_pins(&gamma).unwrap(),
        [type_pin(data(2), SHARED, ACTION, "alpha")]
    );
    assert_eq!(raw_head(&files, 2), gamma_head, "a source is never written");
    assert_eq!(counts(&alpha, 0).unwrap(), [2, 0]);
    // A retry is the same batch only with the same sources.
    assert_eq!(
        as_writer(|| commit_derived(&batch, &[], &[&gamma])).unwrap(),
        vector
    );
    let error = as_writer(|| commit(&batch)).unwrap_err();
    assert!(error.to_string().contains("different batch"), "{error}");
}

#[test]
fn b2_a_type_held_only_by_an_unenlisted_file_refuses_with_nothing_prepared() {
    let temp = tempfile::tempdir().unwrap();
    let files = shacl_files(temp.path());
    // Beta holds the type but is not enlisted; gamma is a plain member.
    seed(&files, 1, 0, &[(SHARED, RDF_TYPE, ACTION)]);
    let before = heads(&files);
    let (alpha, beta, gamma) = (open_shacl(&files, 0), open(&files, 1), open(&files, 2));
    let batch = ParticipantBatch {
        batch_id: "batch-b2".into(),
        members: vec![
            alpha_member(&files, &alpha, &[SHARED], &[]),
            plain_member(&files, &gamma, 2),
        ],
    };
    let error = as_writer(|| commit_derived(&batch, &[], &[])).unwrap_err();
    assert!(error.to_string().contains(SHARED), "{error}");
    assert_nothing_prepared(&files, [&alpha, &beta, &gamma], &before);
    // The id was not consumed.
    let ok = ParticipantBatch {
        batch_id: "batch-b2".into(),
        members: vec![
            plain_member(&files, &alpha, 0),
            plain_member(&files, &gamma, 2),
        ],
    };
    as_writer(|| commit(&ok)).unwrap();
}

#[test]
fn b3_a_wrong_class_in_the_source_keeps_the_violation_and_prepares_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let files = shacl_files(temp.path());
    seed(&files, 2, 0, &[(SHARED, RDF_TYPE, OTHER)]);
    let before = heads(&files);
    let (alpha, beta, gamma) = (open_shacl(&files, 0), open(&files, 1), open(&files, 2));
    let batch = ParticipantBatch {
        batch_id: "batch-b3".into(),
        members: vec![
            alpha_member(&files, &alpha, &[SHARED], &[]),
            plain_member(&files, &beta, 1),
        ],
    };
    let error = as_writer(|| commit_derived(&batch, &[], &[&gamma])).unwrap_err();
    assert!(error.to_string().contains(SHARED), "{error}");
    assert_nothing_prepared(&files, [&alpha, &beta, &gamma], &before);
    assert!(dependency_pins(&gamma).unwrap().is_empty());
}

#[test]
fn b4_more_derived_checks_than_the_bound_refuse_before_admission() {
    let temp = tempfile::tempdir().unwrap();
    let files = shacl_files(temp.path());
    let values = many();
    let typed: Vec<_> = values
        .iter()
        .map(|v| (v.as_str(), RDF_TYPE, ACTION))
        .collect();
    seed(&files, 2, 0, &typed);
    let before = heads(&files);
    let (alpha, beta, gamma) = (open_shacl(&files, 0), open(&files, 1), open(&files, 2));
    let refs: Vec<&str> = values.iter().map(String::as_str).collect();
    let batch = ParticipantBatch {
        batch_id: "batch-b4".into(),
        members: vec![
            alpha_member(&files, &alpha, &refs, &[]),
            plain_member(&files, &beta, 1),
        ],
    };
    let error = as_writer(|| commit_derived(&batch, &[], &[&gamma])).unwrap_err();
    assert!(error.to_string().contains("bound"), "{error}");
    assert_nothing_prepared(&files, [&alpha, &beta, &gamma], &before);
}

#[test]
fn b5_a_payload_that_conforms_alone_derives_nothing_and_the_context_adds_no_violation() {
    let temp = tempfile::tempdir().unwrap();
    let files = shacl_files(temp.path());
    // Gamma also says SHARED is an Other with no name. Were that type pulled
    // in as context it would make SHARED an Other focus and add a minCount
    // violation: repair is subset-only, so it must not.
    seed(
        &files,
        2,
        0,
        &[(SHARED, RDF_TYPE, ACTION), (SHARED, RDF_TYPE, OTHER)],
    );
    let (alpha, beta, gamma) = (open_shacl(&files, 0), open(&files, 1), open(&files, 2));
    // SHARED is typed locally in the same payload: no derivation needed.
    let batch = ParticipantBatch {
        batch_id: "batch-b5".into(),
        members: vec![
            alpha_member(&files, &alpha, &[SHARED], &[(SHARED, RDF_TYPE, ACTION)]),
            plain_member(&files, &beta, 1),
        ],
    };
    as_writer(|| commit_derived(&batch, &[], &[&gamma])).unwrap();
    assert!(dependency_pins(&gamma).unwrap().is_empty());
    drop(batch);
    drop((alpha, beta, gamma));
    // Derivation in progress (value typed only remotely) with the same extra
    // Other type in the source: the derived context still adds no violation.
    let other = OTHER_SHARED;
    seed(
        &files,
        2,
        1,
        &[(other, RDF_TYPE, ACTION), (other, RDF_TYPE, OTHER)],
    );
    let (alpha, beta, gamma) = (open_shacl(&files, 0), open(&files, 1), open(&files, 2));
    let batch = ParticipantBatch {
        batch_id: "batch-b5-derived".into(),
        members: vec![
            alpha_member(&files, &alpha, &[other], &[]),
            plain_member(&files, &beta, 1),
        ],
    };
    as_writer(|| commit_derived(&batch, &[], &[&gamma])).unwrap();
    assert_eq!(
        dependency_pins(&gamma).unwrap(),
        [type_pin(provenance(2), other, ACTION, "alpha")]
    );
}

/// B1's committed state, for the inverse rows.
fn committed_b1(files: &Files) -> (GraphSetOwner, GraphSetOwner, GraphSetOwner) {
    seed(files, 2, 0, &[(SHARED, RDF_TYPE, ACTION)]);
    let (alpha, beta, gamma) = (open_shacl(files, 0), open(files, 1), open(files, 2));
    let batch = ParticipantBatch {
        batch_id: "batch-b1".into(),
        members: vec![
            alpha_member(files, &alpha, &[SHARED], &[]),
            plain_member(files, &beta, 1),
        ],
    };
    as_writer(|| commit_derived(&batch, &[], &[&gamma])).unwrap();
    (alpha, beta, gamma)
}

#[test]
fn b6_retracting_a_derived_pinned_type_is_refused_by_the_s4_guard() {
    let temp = tempfile::tempdir().unwrap();
    let files = shacl_files(temp.path());
    let (_alpha, _beta, gamma) = committed_b1(&files);
    let head = raw_head(&files, 2);
    let error = retract(&files, &gamma, 2, 0, (SHARED, RDF_TYPE, ACTION)).unwrap_err();
    assert!(error.to_string().contains("pinned"), "{error}");
    assert_eq!(raw_head(&files, 2), head);
}

#[test]
fn b7_a_derived_dependency_equal_to_a_declared_one_is_one_check_and_one_pin() {
    let temp = tempfile::tempdir().unwrap();
    let files = shacl_files(temp.path());
    seed(&files, 2, 0, &[(SHARED, RDF_TYPE, ACTION)]);
    let (alpha, beta, gamma) = (open_shacl(&files, 0), open(&files, 1), open(&files, 2));
    let declared = TypeDependency {
        graph: data(2).into(),
        entity: SHARED.into(),
        type_iri: ACTION.into(),
    };
    // The full declared bound, all equal to the derived one: the derived
    // duplicate must not count a 65th check.
    let validations = [Validation {
        owner: &gamma,
        absences: Vec::new(),
        checks: vec![declared; MAX_DEPENDENCY_CHECKS],
    }];
    let batch = ParticipantBatch {
        batch_id: "batch-b7".into(),
        members: vec![
            alpha_member(&files, &alpha, &[SHARED], &[]),
            plain_member(&files, &beta, 1),
        ],
    };
    as_writer(|| commit_derived(&batch, &validations, &[&gamma])).unwrap();
    // Declared pins cover every member (S4); the derived one adds no copy.
    assert_eq!(
        dependency_pins(&gamma).unwrap(),
        [
            type_pin(data(2), SHARED, ACTION, "alpha"),
            type_pin(data(2), SHARED, ACTION, "beta"),
        ]
    );
}

#[test]
fn b8_a_class_satisfied_through_a_subclass_pins_the_axiom_where_it_lives() {
    let temp = tempfile::tempdir().unwrap();
    let files = shacl_files(temp.path());
    // The type in gamma's data graph, the axiom in gamma's provenance graph.
    seed(&files, 2, 0, &[(SHARED, RDF_TYPE, SUB_ACTION)]);
    seed(&files, 2, 1, &[(SUB_ACTION, SUBCLASS_OF, ACTION)]);
    let (alpha, beta, gamma) = (open_shacl(&files, 0), open(&files, 1), open(&files, 2));
    let batch = ParticipantBatch {
        batch_id: "batch-b8".into(),
        members: vec![
            alpha_member(&files, &alpha, &[SHARED], &[]),
            plain_member(&files, &beta, 1),
        ],
    };
    as_writer(|| commit_derived(&batch, &[], &[&gamma])).unwrap();
    let head = raw_head(&files, 2);
    // Both triples the satisfaction relies on are pinned: neither may go.
    let error = retract(&files, &gamma, 2, 1, (SUB_ACTION, SUBCLASS_OF, ACTION)).unwrap_err();
    assert!(error.to_string().contains("pinned"), "{error}");
    let error = retract(&files, &gamma, 2, 0, (SHARED, RDF_TYPE, SUB_ACTION)).unwrap_err();
    assert!(error.to_string().contains("pinned"), "{error}");
    assert_eq!(raw_head(&files, 2), head);
}

#[test]
fn b8_an_axiom_outside_every_enlisted_source_refuses_with_nothing_prepared() {
    let temp = tempfile::tempdir().unwrap();
    let files = shacl_files(temp.path());
    seed(&files, 2, 0, &[(SHARED, RDF_TYPE, SUB_ACTION)]);
    // The axiom exists only in beta, a member and not a source.
    seed(&files, 1, 1, &[(SUB_ACTION, SUBCLASS_OF, ACTION)]);
    let before = heads(&files);
    let (alpha, beta, gamma) = (open_shacl(&files, 0), open(&files, 1), open(&files, 2));
    let batch = ParticipantBatch {
        batch_id: "batch-b8-out".into(),
        members: vec![
            alpha_member(&files, &alpha, &[SHARED], &[]),
            plain_member(&files, &beta, 1),
        ],
    };
    let error = as_writer(|| commit_derived(&batch, &[], &[&gamma])).unwrap_err();
    assert!(error.to_string().contains(SHARED), "{error}");
    assert_nothing_prepared(&files, [&alpha, &beta, &gamma], &before);
    assert!(dependency_pins(&gamma).unwrap().is_empty());
}

#[test]
fn ordinary_single_file_writes_are_unchanged_by_slice_b() {
    // Control: alpha's ordinary owner still judges sh:class locally only.
    let temp = tempfile::tempdir().unwrap();
    let files = shacl_files(temp.path());
    seed(&files, 2, 0, &[(SHARED, RDF_TYPE, ACTION)]);
    let alpha = open_shacl(&files, 0);
    let member = alpha_member(&files, &alpha, &[SHARED], &[]);
    assert!(alpha.transact(&member.requests, &member.batch).is_err());
    let local = alpha_member(&files, &alpha, &[SHARED], &[(SHARED, RDF_TYPE, ACTION)]);
    alpha.transact(&local.requests, &local.batch).unwrap();
}
