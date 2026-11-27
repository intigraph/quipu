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
const RELATES: &str = "urn:p:relates";
const SHARED: &str = "urn:shared:action";
const ACTION: &str = "urn:type:Action";
const SUB_ACTION: &str = "urn:type:SubAction";
const OTHER: &str = "urn:type:Other";
const OTHER_SHARED: &str = "urn:shared:other-action";
const CYCLE_A: &str = "urn:type:CycleA";
const CYCLE_B: &str = "urn:type:CycleB";

/// Alpha's pinned shapes: an Action's blockedOn values must be Actions and
/// its relates values Others, and an Other must be a blank node. Every Other
/// here is an IRI, so a derived Other that became a focus would add a
/// (judged) nodeKind violation: the subset-only discriminator. (sh:minCount
/// would not do: required-value floors are outside the post-state lane.)
const SHAPES: &str = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
<urn:shape:Action> a sh:NodeShape ; sh:targetClass <urn:type:Action> ;
  sh:property [ sh:path <urn:p:blockedOn> ; sh:class <urn:type:Action> ] ;
  sh:property [ sh:path <urn:p:relates> ; sh:class <urn:type:Other> ] .
<urn:shape:Other> a sh:NodeShape ; sh:targetClass <urn:type:Other> ;
  sh:nodeKind sh:BlankNode .
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
        RELATES,
        SHARED,
        ACTION,
        SUB_ACTION,
        OTHER,
        OTHER_SHARED,
        CYCLE_A,
        CYCLE_B,
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
    open_shacl_with(files, index, None)
}

/// [`open_shacl`] with extra runtime (explicit) write shapes.
fn open_shacl_with(files: &Files, index: usize, explicit: Option<&str>) -> GraphSetOwner {
    let mut store = Store::open(path(files, index).to_str().unwrap()).unwrap();
    store.shacl_config_mut().validate_on_write = true;
    store.write_shapes = explicit.map(str::to_owned);
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
fn graph_batch(files: &Files, index: usize, graph: usize, datums: &[Datum]) -> GraphSetBatch {
    let mut batch = batch(files, index, "unused");
    for (at, (_, rows)) in batch.graphs.iter_mut().enumerate() {
        *rows = if at == graph {
            datums.to_vec()
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
        .transact(&requests(index), &graph_batch(files, index, graph, &datums))
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
        batch: graph_batch(files, 0, 0, &datums),
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
                &[triple(files, index, s, p, o, Op::Retract)],
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
    // SHARED is an Other, not an Action. OTHER_SHARED (an IRI Other)
    // derives, so the refusal is judged WITH derived context: it must be
    // exactly the one class violation, none added by the context.
    seed(
        &files,
        2,
        0,
        &[(SHARED, RDF_TYPE, OTHER), (OTHER_SHARED, RDF_TYPE, OTHER)],
    );
    let before = heads(&files);
    let (alpha, beta, gamma) = (open_shacl(&files, 0), open(&files, 1), open(&files, 2));
    let batch = ParticipantBatch {
        batch_id: "batch-b3".into(),
        members: vec![
            alpha_member(&files, &alpha, &[SHARED], &[(ITEM, RELATES, OTHER_SHARED)]),
            plain_member(&files, &beta, 1),
        ],
    };
    let error = as_writer(|| commit_derived(&batch, &[], &[&gamma])).unwrap_err();
    let text = error.to_string();
    assert!(text.contains("with derived types"), "{error}");
    assert!(text.contains(" 1 violation"), "{error}");
    assert!(
        text.contains(SHARED) && !text.contains(&format!("{OTHER_SHARED} (")),
        "{error}"
    );
    assert_nothing_prepared(&files, [&alpha, &beta, &gamma], &before);
    assert!(dependency_pins(&gamma).unwrap().is_empty());
}

#[test]
fn b4_more_derived_checks_than_the_bound_refuse_before_any_intent() {
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
    // Gamma also says SHARED is an (IRI) Other. Were that type pulled in as
    // context it would make SHARED an Other focus and add a nodeKind
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
    // The value derives an IRI Other: as context it satisfies sh:class
    // and must add no nodeKind violation, on the incremental path (alpha's
    // graph now has history) and on the new-graph path (a fresh fixture).
    derives_an_iri_other(&files, "batch-b5-incremental");
    let fresh = tempfile::tempdir().unwrap();
    derives_an_iri_other(&shacl_files(fresh.path()), "batch-b5-new-graph");
}

fn derives_an_iri_other(files: &Files, batch_id: &str) {
    seed(files, 2, 1, &[(OTHER_SHARED, RDF_TYPE, OTHER)]);
    let (alpha, beta, gamma) = (open_shacl(files, 0), open(files, 1), open(files, 2));
    let batch = ParticipantBatch {
        batch_id: batch_id.into(),
        members: vec![
            alpha_member(files, &alpha, &[], &[(ITEM, RELATES, OTHER_SHARED)]),
            plain_member(files, &beta, 1),
        ],
    };
    as_writer(|| commit_derived(&batch, &[], &[&gamma])).unwrap();
    assert_eq!(
        dependency_pins(&gamma).unwrap(),
        [type_pin(provenance(2), OTHER_SHARED, OTHER, "alpha")]
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

/// Shapes that refuse every Action (an IRI is never a blank node): what a
/// reload to different bytes looks like to an already judged member.
const REFUSING: &str = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
<urn:shape:Action> a sh:NodeShape ; sh:targetClass <urn:type:Action> ;
  sh:nodeKind sh:BlankNode .
"#;

fn audit_rows(files: &Files) -> Vec<(String, String)> {
    let conn = rusqlite::Connection::open_with_flags(
        path(files, 0),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let mut stmt = conn
        .prepare("SELECT kind, detail FROM participant_audit ORDER BY kind")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn intent_state(files: &Files, index: usize, batch_id: &str) -> String {
    let conn = rusqlite::Connection::open_with_flags(
        path(files, index),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    conn.query_row(
        "SELECT state FROM participant_intents WHERE batch_id=?1",
        [batch_id],
        |r| r.get(0),
    )
    .unwrap()
}

fn b1_batch<'a>(
    files: &Files,
    alpha: &'a GraphSetOwner,
    beta: &'a GraphSetOwner,
    id: &str,
) -> ParticipantBatch<'a> {
    ParticipantBatch {
        batch_id: id.into(),
        members: vec![
            alpha_member(files, alpha, &[SHARED], &[]),
            plain_member(files, beta, 1),
        ],
    }
}

#[test]
fn c1_a_shapes_reload_between_prepare_and_the_decision_aborts_before_commit() {
    let temp = tempfile::tempdir().unwrap();
    let files = shacl_files(temp.path());
    seed(&files, 2, 0, &[(SHARED, RDF_TYPE, ACTION)]);
    let before = heads(&files);
    let (alpha, beta, gamma) = (open_shacl(&files, 0), open(&files, 1), open(&files, 2));
    let batch = b1_batch(&files, &alpha, &beta, "batch-c1-pre");
    let error = as_writer(|| {
        derive::with_before_decision(
            |store| {
                store
                    .load_shapes("slice-b", REFUSING, "2026-01-02T00:00:00Z")
                    .unwrap();
            },
            || commit_derived(&batch, &[], &[&gamma]),
        )
    })
    .unwrap_err();
    assert!(error.to_string().contains("before commit"), "{error}");
    // The reload itself is a registry write in alpha; no participant facts.
    assert_eq!(heads(&files)[1..], before[1..]);
    assert_eq!(
        counts(&alpha, 0).unwrap(),
        [0, 0],
        "an aborted batch wrote no facts"
    );
    assert_eq!(intent_state(&files, 0, "batch-c1-pre"), "aborted");
    // No pending-batch fence anywhere: the plain files take ordinary writes.
    local_write(&files, &beta, 1, "after").unwrap();
    local_write(&files, &gamma, 2, "after").unwrap();
    assert!(dependency_pins(&gamma).unwrap().is_empty());
}

#[test]
fn c1_shapes_loaded_into_an_unjudged_member_before_the_decision_abort_before_commit() {
    // eph-review-d2b162 #1: beta has no write shapes at PREPARE, so it is not
    // judged. Shapes refusing its batch, loaded before the decision, must
    // abort every intent instead of refusing beta's APPLY after COMMIT.
    let temp = tempfile::tempdir().unwrap();
    let files = shacl_files(temp.path());
    seed(&files, 2, 0, &[(SHARED, RDF_TYPE, ACTION)]);
    let before = heads(&files);
    let (alpha, beta, gamma) = (
        open_shacl(&files, 0),
        open_shacl(&files, 1),
        open(&files, 2),
    );
    let typed = triple(&files, 1, ITEM, RDF_TYPE, ACTION, Op::Assert);
    let batch = ParticipantBatch {
        batch_id: "batch-c1-unjudged".into(),
        members: vec![
            alpha_member(&files, &alpha, &[SHARED], &[]),
            Member {
                owner: &beta,
                requests: requests(1),
                batch: graph_batch(&files, 1, 0, &[typed]),
            },
        ],
    };
    let beta_file = files.routes[1].file_id.clone();
    let error = as_writer(|| {
        derive::with_before_decision_on(
            &beta_file,
            |store| {
                store
                    .load_shapes("slice-b", REFUSING, "2026-01-02T00:00:00Z")
                    .unwrap();
            },
            || commit_derived(&batch, &[], &[&gamma]),
        )
    })
    .unwrap_err();
    assert!(error.to_string().contains("before commit"), "{error}");
    assert!(error.to_string().contains(&beta_file), "{error}");
    assert_eq!(heads(&files)[0], before[0], "alpha wrote nothing");
    assert_eq!(heads(&files)[2], before[2], "gamma wrote nothing");
    assert_eq!(counts(&alpha, 0).unwrap(), [0, 0]);
    assert_eq!(intent_state(&files, 0, "batch-c1-unjudged"), "aborted");
    assert_eq!(intent_state(&files, 1, "batch-c1-unjudged"), "aborted");
    local_write(&files, &gamma, 2, "after").unwrap();
    assert!(dependency_pins(&gamma).unwrap().is_empty());
}

/// Commit B1's batch with a crash right after the COMMIT decision.
fn crashed_after_decision(files: &Files, id: &str) {
    seed(files, 2, 0, &[(SHARED, RDF_TYPE, ACTION)]);
    let (alpha, beta, gamma) = (open_shacl(files, 0), open(files, 1), open(files, 2));
    let batch = b1_batch(files, &alpha, &beta, id);
    let error = as_writer(|| {
        with_crash(CrashPoint::AfterDecision, || {
            commit_derived(&batch, &[], &[&gamma])
        })
    })
    .unwrap_err();
    assert!(injected(&error), "{error}");
}

fn recover_all(
    files: &Files,
    id: &str,
    alpha: GraphSetOwner,
) -> (GraphSetOwner, GraphSetOwner, GraphSetOwner) {
    let (beta, gamma) = (open(files, 1), open(files, 2));
    let Recovery::Published(_) = recover(&[&alpha, &beta, &gamma], id).unwrap() else {
        panic!("a committed batch recovered as an abort");
    };
    (alpha, beta, gamma)
}

#[test]
fn c1_post_a_recovery_after_a_reload_replays_releases_and_verifies_under_carried_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let files = shacl_files(temp.path());
    crashed_after_decision(&files, "batch-c1-post");
    // The server restarts judging with different shapes that would refuse
    // this member. (A registry write into the fenced file is NOT this case:
    // it moves the head, which the S1 expected-head guard refuses; see n0wc.5.)
    let alpha = open_shacl_with(&files, 0, Some(REFUSING));
    let (alpha, beta, gamma) = recover_all(&files, "batch-c1-post", alpha);
    assert_eq!(
        counts(&alpha, 0).unwrap(),
        [2, 0],
        "the decision was finished"
    );
    for index in 0..3 {
        assert_eq!(intent_state(&files, index, "batch-c1-post"), "published");
    }
    local_write(&files, &beta, 1, "after").unwrap();
    local_write(&files, &gamma, 2, "after").unwrap();
    let rows = audit_rows(&files);
    assert_eq!(
        rows.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        ["shapes-delta"],
        "{rows:?}"
    );
    assert!(rows[0].1.contains("bytes carried"), "{rows:?}");
    assert_eq!(
        dependency_pins(&gamma).unwrap(),
        [type_pin(data(2), SHARED, ACTION, "alpha")]
    );
}

#[test]
fn c1_post_b_corrupt_carried_shapes_still_apply_release_and_raise_the_loud_record() {
    let temp = tempfile::tempdir().unwrap();
    let files = shacl_files(temp.path());
    crashed_after_decision(&files, "batch-c1-bad");
    // Corrupt alpha's carried bytes, keeping the intent's integrity digest
    // valid: the only defect is the carried shape set itself.
    let conn = rusqlite::Connection::open(path(&files, 0)).unwrap();
    let (digest, member, head, payload): (String, i64, i64, Vec<u8>) = conn
        .query_row(
            "SELECT batch_digest, member, expected_head, payload FROM participant_intents \
             WHERE batch_id='batch-c1-bad'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    let mut value: Value = serde_json::from_slice(&payload).unwrap();
    value["pins"]["shapes"] = serde_json::json!(REFUSING);
    let payload = value.to_string().into_bytes();
    let intent =
        super::records::intent_digest(&digest, usize::try_from(member).unwrap(), head, &payload);
    conn.execute(
        "UPDATE participant_intents SET payload=?1, intent_digest=?2 WHERE batch_id='batch-c1-bad'",
        rusqlite::params![payload, intent],
    )
    .unwrap();
    drop(conn);
    let (alpha, beta, _gamma) = recover_all(&files, "batch-c1-bad", open_shacl(&files, 0));
    assert_eq!(counts(&alpha, 0).unwrap(), [2, 0]);
    assert_eq!(intent_state(&files, 0, "batch-c1-bad"), "published");
    local_write(&files, &beta, 1, "after").unwrap();
    let rows = audit_rows(&files);
    assert!(
        rows.iter()
            .any(|(k, d)| k == "verify-failed" && d.contains("member alpha")),
        "{rows:?}"
    );
}

#[test]
fn c2_a_slow_validator_refuses_with_nothing_prepared_and_releases_the_fences() {
    let temp = tempfile::tempdir().unwrap();
    let files = shacl_files(temp.path());
    seed(&files, 2, 0, &[(SHARED, RDF_TYPE, ACTION)]);
    let before = heads(&files);
    let (alpha, beta, gamma) = (open_shacl(&files, 0), open(&files, 1), open(&files, 2));
    let batch = b1_batch(&files, &alpha, &beta, "batch-c2");
    let error = as_writer(|| {
        derive::with_prepare_budget(std::time::Duration::from_millis(20), || {
            derive::with_slow_dry_run(std::time::Duration::from_millis(60), || {
                commit_derived(&batch, &[], &[&gamma])
            })
        })
    })
    .unwrap_err();
    assert!(error.to_string().contains("exceeded"), "{error}");
    assert_nothing_prepared(&files, [&alpha, &beta, &gamma], &before);
    // A second writer proceeds, and the same id was not consumed.
    as_writer(|| commit_derived(&batch, &[], &[&gamma])).unwrap();
}

#[test]
fn c3a_a_pinned_axiom_refusal_explains_itself_and_the_owner_can_clear_it() {
    let temp = tempfile::tempdir().unwrap();
    let files = shacl_files(temp.path());
    seed(&files, 2, 0, &[(SHARED, RDF_TYPE, SUB_ACTION)]);
    seed(&files, 2, 1, &[(SUB_ACTION, SUBCLASS_OF, ACTION)]);
    let (alpha, beta, gamma) = (open_shacl(&files, 0), open(&files, 1), open(&files, 2));
    as_writer(|| {
        commit_derived(
            &b1_batch(&files, &alpha, &beta, "batch-c3a"),
            &[],
            &[&gamma],
        )
    })
    .unwrap();
    let error = retract(&files, &gamma, 2, 1, (SUB_ACTION, SUBCLASS_OF, ACTION)).unwrap_err();
    let text = error.to_string();
    for part in [
        "dependent file alpha",
        SUB_ACTION,
        "ReferenceAbsence { entity: urn:shared:action }",
        "escalate to an administrator",
        "no bypass",
    ] {
        assert!(text.contains(part), "{part}: {text}");
    }
    // The release batch the refusal names: alpha drops its reference, gamma
    // retracts the axiom, alpha is enlisted with the absence check.
    let release = ParticipantBatch {
        batch_id: "batch-c3a-release".into(),
        members: vec![
            Member {
                owner: &alpha,
                requests: requests(0),
                batch: graph_batch(
                    &files,
                    0,
                    0,
                    &[triple(&files, 0, ITEM, BLOCKED_ON, SHARED, Op::Retract)],
                ),
            },
            Member {
                owner: &gamma,
                requests: requests(2),
                batch: graph_batch(
                    &files,
                    2,
                    1,
                    &[triple(
                        &files,
                        2,
                        SUB_ACTION,
                        SUBCLASS_OF,
                        ACTION,
                        Op::Retract,
                    )],
                ),
            },
        ],
    };
    let validations = [Validation {
        owner: &alpha,
        checks: Vec::new(),
        absences: vec![ReferenceAbsence {
            entity: SHARED.into(),
        }],
    }];
    as_writer(|| commit_validated(&release, &validations)).unwrap();
    assert!(subclass_pins(&gamma).unwrap().is_empty());
    assert!(dependency_pins(&gamma).unwrap().is_empty());
    // eph-review-873d46 #2: alpha is a judged member whose batch retracts;
    // the post-release audit must not record a false verify failure.
    let audits = audit_rows(&files);
    assert!(
        !audits.iter().any(|(kind, _)| kind == "verify-failed"),
        "{audits:?}"
    );
}

#[test]
fn c3b_a_subclass_cycle_terminates_with_a_deterministic_chain_or_refusal() {
    // No exit to Action: the walk terminates and refuses naming the value.
    let temp = tempfile::tempdir().unwrap();
    let files = shacl_files(temp.path());
    seed(&files, 2, 0, &[(SHARED, RDF_TYPE, CYCLE_A)]);
    seed(
        &files,
        2,
        1,
        &[
            (CYCLE_A, SUBCLASS_OF, CYCLE_B),
            (CYCLE_B, SUBCLASS_OF, CYCLE_A),
        ],
    );
    {
        let (alpha, beta, gamma) = (open_shacl(&files, 0), open(&files, 1), open(&files, 2));
        let error = as_writer(|| {
            commit_derived(
                &b1_batch(&files, &alpha, &beta, "batch-c3b"),
                &[],
                &[&gamma],
            )
        })
        .unwrap_err();
        assert!(error.to_string().contains(SHARED), "{error}");
    }
    // With an exit (B sub Action), the chain is A sub B, B sub Action: two
    // axiom checks plus the type, each counted, each pinned once.
    seed(&files, 2, 1, &[(CYCLE_B, SUBCLASS_OF, ACTION)]);
    let (alpha, beta, gamma) = (open_shacl(&files, 0), open(&files, 1), open(&files, 2));
    as_writer(|| {
        commit_derived(
            &b1_batch(&files, &alpha, &beta, "batch-c3b-exit"),
            &[],
            &[&gamma],
        )
    })
    .unwrap();
    assert_eq!(
        subclass_pins(&gamma)
            .unwrap()
            .iter()
            .map(|p| (p.class.as_str(), p.parent.as_str(), p.via.as_str()))
            .collect::<Vec<_>>(),
        [(CYCLE_A, CYCLE_B, SHARED), (CYCLE_B, ACTION, SHARED)]
    );
    assert_eq!(
        dependency_pins(&gamma).unwrap(),
        [type_pin(data(2), SHARED, CYCLE_A, "alpha")]
    );
}

/// eph-review-873d46 #1: the PREPARE dry run interns foreign IRIs (the
/// derived context's class) into the member file inside a rolled-back
/// savepoint. When the batch then aborts, nothing re-interns them, so the
/// writer's term cache must not keep the ids rollback freed: rowids are
/// reused, and a stale entry would resolve a later term to the wrong IRI.
#[test]
#[cfg(feature = "ownership-witness-s3")] // the writer latch needs it
fn the_member_writer_term_cache_matches_its_file_after_an_aborted_derived_batch() {
    let temp = tempfile::tempdir().unwrap();
    let files = shacl_files(temp.path());
    // b8's shape of source, through a subclass IRI only gamma holds (the
    // fixture pre-interns its own vocabulary in every file).
    const FOREIGN: &str = "urn:type:OnlyInGamma";
    {
        let gamma = Store::open(path(&files, 2).to_str().unwrap()).unwrap();
        let id = gamma.intern(FOREIGN).unwrap();
        TERMS.with(|t| t.borrow_mut()[2].insert(FOREIGN.to_owned(), id));
    }
    seed(&files, 2, 0, &[(SHARED, RDF_TYPE, FOREIGN)]);
    seed(&files, 2, 1, &[(FOREIGN, SUBCLASS_OF, ACTION)]);
    let (alpha, beta, gamma) = (open_shacl(&files, 0), open(&files, 1), open(&files, 2));
    let batch = b1_batch(&files, &alpha, &beta, "batch-cache");
    // Alpha's dry run with the derived context succeeds; a shapes change
    // before the decision then aborts every intent, so nothing is applied.
    let error = as_writer(|| {
        derive::with_before_decision(
            |store| {
                store
                    .load_shapes("slice-b", REFUSING, "2026-01-02T00:00:00Z")
                    .unwrap();
            },
            || commit_derived(&batch, &[], &[&gamma]),
        )
    })
    .unwrap_err();
    assert!(error.to_string().contains("before commit"), "{error}");
    let truth = |store: &Store, iri: &str| -> Option<i64> {
        store
            .conn
            .query_row("SELECT id FROM terms WHERE iri = ?1", [iri], |r| r.get(0))
            .ok()
    };
    let w = alpha.hold_writer_for_test();
    assert_eq!(
        truth(&w, FOREIGN),
        None,
        "an aborted batch left the IRI in alpha"
    );
    for iri in [FOREIGN, SUBCLASS_OF] {
        assert_eq!(
            w.lookup(iri).unwrap(),
            truth(&w, iri),
            "stale cache for {iri}"
        );
    }
    // A later new term takes the next rowid; the class IRI must not alias it.
    let fresh = w.intern("urn:fresh:after-the-dry-run").unwrap();
    assert_eq!(w.resolve(fresh).unwrap(), "urn:fresh:after-the-dry-run");
    assert_ne!(
        w.lookup(FOREIGN).unwrap(),
        Some(fresh),
        "a dry-run IRI aliases a new term"
    );
}
