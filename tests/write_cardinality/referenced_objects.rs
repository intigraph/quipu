//! A write owns value-set verdicts only for focuses it can change (aegis-2i6).
//!
//! Every hourly code/commit/doc ingest was refused for seven hours because a
//! commit's `modifies` edge put the module it pointed at on trial, and that
//! module already carried two labels. Edges that can change an object's
//! verdict (object targets, inverse counts) must still own the object.
use quipu::{Datum, Op, Store, Value};

const TS: &str = "2026-10-21T00:00:00Z";
const EX: &str = "https://example.org/2i6/";
const TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const LABELS: &str = r#"
@prefix ex: <https://example.org/2i6/> .
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
ex:ModuleShape a sh:NodeShape; sh:targetClass ex:Module;
 sh:property [sh:path rdfs:label; sh:maxCount 1].
"#;

fn iri(store: &Store, local: &str) -> i64 {
    store.intern(&format!("{EX}{local}")).unwrap()
}

fn fact(store: &Store, subject: &str, predicate: &str, value: Value) -> Datum {
    let attribute = if predicate.starts_with("http") {
        store.intern(predicate).unwrap()
    } else {
        iri(store, predicate)
    };
    Datum {
        entity: iri(store, subject),
        attribute,
        value,
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    }
}

fn reference(store: &Store, subject: &str, predicate: &str, object: &str) -> Datum {
    let object = Value::Ref(iri(store, object));
    fact(store, subject, predicate, object)
}

/// A module that already violates `rdfs:label sh:maxCount 1`, written before
/// any shape applied, as two producers that disagree on the label did.
fn module_with_two_labels(store: &mut Store) {
    let label = "http://www.w3.org/2000/01/rdf-schema#label";
    let debt = vec![
        reference(store, "module", TYPE, "Module"),
        fact(store, "module", label, Value::Str("shacl.rs".into())),
        fact(store, "module", label, Value::Str("src/shacl.rs".into())),
    ];
    store.transact(&debt, TS, None, None).unwrap();
}

fn write(store: &mut Store, shapes: &str, data: &[Datum]) -> quipu::Result<i64> {
    store.with_write_shapes(Some(shapes), |s| s.transact(data, TS, None, None))
}

fn refused_on(result: &quipu::Result<i64>, focus: &str) -> bool {
    matches!(result, Err(quipu::Error::ValidationFailed { messages, .. })
        if messages.iter().any(|m| m.contains(&format!("{EX}{focus}"))))
}

#[test]
fn an_edge_to_a_node_with_stored_overcount_commits() {
    let mut store = Store::open_in_memory().unwrap();
    module_with_two_labels(&mut store);
    let commit = vec![
        reference(&store, "commit", TYPE, "GitCommit"),
        reference(&store, "commit", "modifies", "module"),
    ];
    write(&mut store, LABELS, &commit).unwrap();
    // Positive control: a write that adds to the module's own labels is the
    // module's write, and its verdict is still owned.
    let label = "http://www.w3.org/2000/01/rdf-schema#label";
    let third = vec![fact(&store, "module", label, Value::Str("third".into()))];
    let result = write(&mut store, LABELS, &third);
    assert!(refused_on(&result, "module"), "{result:?}");
}

#[test]
fn an_object_target_edge_still_owns_its_object() {
    let shapes = r#"
@prefix ex: <https://example.org/2i6/> .
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
ex:Linked a sh:NodeShape; sh:targetObjectsOf ex:links;
 sh:property [sh:path rdfs:label; sh:maxCount 1].
"#;
    let mut store = Store::open_in_memory().unwrap();
    module_with_two_labels(&mut store);
    // Not a target until something links to it; the link makes it one.
    let unrelated = vec![reference(&store, "page", "modifies", "module")];
    write(&mut store, shapes, &unrelated).unwrap();
    let link = vec![reference(&store, "page", "links", "module")];
    let result = write(&mut store, shapes, &link);
    assert!(refused_on(&result, "module"), "{result:?}");
}

#[test]
fn an_inverse_count_edge_still_owns_its_object() {
    let shapes = r#"
@prefix ex: <https://example.org/2i6/> .
@prefix sh: <http://www.w3.org/ns/shacl#> .
ex:Owned a sh:NodeShape; sh:targetClass ex:Module;
 sh:property [sh:path [sh:inversePath ex:owns]; sh:maxCount 1].
"#;
    let mut store = Store::open_in_memory().unwrap();
    let first = vec![
        reference(&store, "module", TYPE, "Module"),
        reference(&store, "first", "owns", "module"),
    ];
    write(&mut store, shapes, &first).unwrap();
    // Another predicate does not change the inverse count.
    let other = vec![reference(&store, "second", "modifies", "module")];
    write(&mut store, shapes, &other).unwrap();
    let second = vec![reference(&store, "second", "owns", "module")];
    let result = write(&mut store, shapes, &second);
    assert!(refused_on(&result, "module"), "{result:?}");
}

#[test]
fn restating_a_value_or_adding_another_property_does_not_own_stored_debt() {
    let mut store = Store::open_in_memory().unwrap();
    module_with_two_labels(&mut store);
    let label = "http://www.w3.org/2000/01/rdf-schema#label";
    // A producer restating its own label, as every hourly snapshot does.
    let restated = vec![
        reference(&store, "module", TYPE, "Module"),
        fact(&store, "module", label, Value::Str("src/shacl.rs".into())),
    ];
    write(&mut store, LABELS, &restated).unwrap();
    // doc-mentions: the over-bound node is the SUBJECT of a new, unbounded edge.
    let mention = vec![reference(&store, "module", "mentions", "quipu")];
    write(&mut store, LABELS, &mention).unwrap();
}

/// grant (#732): an IRI may contain parentheses. A new second value under
/// such a predicate must still be owned, read forward or inversely.
#[test]
fn a_new_value_under_a_parenthesised_predicate_is_still_owned() {
    let shapes = r#"
@prefix ex: <https://example.org/2i6/> .
@prefix sh: <http://www.w3.org/ns/shacl#> .
ex:Paren a sh:NodeShape; sh:targetClass ex:Module;
 sh:property [sh:path <https://example.org/2i6/p(x)>; sh:maxCount 1];
 sh:property [sh:path [sh:inversePath <https://example.org/2i6/q(y)>]; sh:maxCount 1].
"#;
    let mut store = Store::open_in_memory().unwrap();
    let first = vec![
        reference(&store, "module", TYPE, "Module"),
        fact(&store, "module", "p(x)", Value::Str("one".into())),
        reference(&store, "first", "q(y)", "module"),
    ];
    write(&mut store, shapes, &first).unwrap();
    let forward = vec![fact(&store, "module", "p(x)", Value::Str("two".into()))];
    let result = write(&mut store, shapes, &forward);
    assert!(refused_on(&result, "module"), "forward: {result:?}");
    let inverse = vec![reference(&store, "second", "q(y)", "module")];
    let result = write(&mut store, shapes, &inverse);
    assert!(refused_on(&result, "module"), "inverse: {result:?}");
}
