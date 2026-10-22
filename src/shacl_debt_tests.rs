//! Pre-existing debt admission (aegis-g51c C12).
//!
//! The production case: a session-capture `/episode` reused an old codex
//! Session as `{name, type: Session}`. The stored Session predates the shape
//! and lacks `actor` and `sourceKind`, so the write was refused for debt it
//! did not introduce (tx 722359). Each test below is one of the ruling's
//! positive controls; together they pin that ONLY that exact debt is admitted.

use super::*;

const SHAPES: &str = r#"
@prefix sh:    <http://www.w3.org/ns/shacl#> .
@prefix xsd:   <http://www.w3.org/2001/XMLSchema#> .
@prefix aegis: <http://aegis.gastown.local/ontology/> .

aegis:SessionShape a sh:NodeShape ;
    sh:targetClass aegis:Session ;
    sh:property [ sh:path aegis:actor ; sh:minCount 1 ;
                  sh:message "a Session names the principal that ran it" ] ;
    sh:property [ sh:path aegis:sourceKind ; sh:minCount 1 ; sh:maxCount 1 ;
                  sh:in ( "observed" ) ;
                  sh:message "a Session enters as observed" ] ;
    sh:property [ sh:path aegis:startedAt ; sh:datatype xsd:dateTime ;
                  sh:message "a Session start is a dateTime" ] .
"#;

const A: &str = "http://aegis.gastown.local/ontology/";

/// The codex-session debt node: typed, but without `actor` or `sourceKind`.
fn debt_session() -> String {
    format!("<{A}old-session> a <{A}Session> .\n")
}

/// A conforming Session.
fn good_session() -> String {
    format!(
        "<{A}good-session> a <{A}Session> ; <{A}actor> <{A}alice> ; <{A}sourceKind> \"observed\" .\n"
    )
}

fn store_with(turtle: &str) -> Store {
    let mut store = Store::open_in_memory().expect("store");
    crate::rdf::ingest_rdf(
        &mut store,
        turtle.as_bytes(),
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-10-22T00:00:00Z",
        None,
        Some("test"),
    )
    .expect("ingested");
    store
}

fn debt_events(store: &Store) -> Vec<String> {
    store
        .pending_write_events
        .borrow()
        .iter()
        .filter(|e| e.event_type == "shacl.debt_admitted")
        .map(|e| e.payload["path"].as_str().unwrap_or("").to_string())
        .collect()
}

/// (a) `{name, type}` reuse of an existing debt Session is admitted, with each
/// debt result kept as a Warning and counted as a `shacl.debt_admitted` event.
#[test]
fn reuse_of_a_debt_node_is_admitted_as_warnings() {
    let store = store_with(&debt_session());
    let reuse = debt_session();

    // CONTROL: without the store this payload is refused; else nothing is tested.
    let ctl = crate::shacl::validate_shapes(SHAPES, &reuse).expect("validated");
    assert_eq!(
        ctl.violations, 2,
        "control must reproduce tx 722359: {:?}",
        ctl.results
    );

    let v = validate_with_store_context(&store, SHAPES, &reuse).expect("validated");
    assert!(v.conforms, "debt reuse refused: {:?}", v.results);
    assert_eq!((v.violations, v.warnings), (0, 2), "{:?}", v.results);
    assert!(
        v.results.iter().all(|r| r
            .message
            .as_deref()
            .unwrap_or("")
            .starts_with(DEBT_MESSAGE_PREFIX)),
        "{:?}",
        v.results
    );
    let mut paths = debt_events(&store);
    paths.sort();
    assert_eq!(paths, [format!("{A}actor"), format!("{A}sourceKind")]);
}

/// (b) The same write on a NEW node is refused exactly as before.
#[test]
fn the_same_write_on_a_new_node_is_refused() {
    let store = store_with(&debt_session());
    let fresh = format!("<{A}new-session> a <{A}Session> .\n");
    let v = validate_with_store_context(&store, SHAPES, &fresh).expect("validated");
    assert!(!v.conforms);
    assert_eq!(v.violations, 2, "{:?}", v.results);
    assert!(debt_events(&store).is_empty());
}

/// (c) Removing a required field from a conforming node is refused; the
/// controls show a delete that keeps the node conforming still passes.
#[test]
fn removing_a_required_field_from_a_conforming_node_is_refused() {
    let store = store_with(&good_session());
    let removed = format!("<{A}good-session> <{A}actor> <{A}alice> .\n");

    let v = retraction_violations_in_graph(&store, SHAPES, &removed, "", crate::schema::ROOT_GRAPH)
        .expect("validated");
    assert!(!v.conforms, "a delete made the node violate and passed");
    assert_eq!(v.violations, 1, "{:?}", v.results);
    assert_eq!(
        v.results[0].path.as_deref(),
        Some(format!("{A}actor").as_str())
    );

    // CONTROL: the same delete with a replacement value in the same update.
    let replaced = format!("<{A}good-session> <{A}actor> <{A}bob> .\n");
    let ok = retraction_violations_in_graph(
        &store,
        SHAPES,
        &removed,
        &replaced,
        crate::schema::ROOT_GRAPH,
    )
    .expect("validated");
    assert!(ok.conforms, "a replace was refused: {:?}", ok.results);

    // CONTROL: deleting a value on a path no shape requires is not judged.
    let optional = format!("<{A}good-session> <{A}note> \"x\" .\n");
    let ok =
        retraction_violations_in_graph(&store, SHAPES, &optional, "", crate::schema::ROOT_GRAPH)
            .expect("validated");
    assert!(ok.conforms, "{:?}", ok.results);
}

/// A delete on a node that was ALREADY in debt is not blamed for that debt.
#[test]
fn a_delete_on_a_debt_node_is_judged_only_for_what_it_changes() {
    let store = store_with(&format!(
        "<{A}old-session> a <{A}Session> ; <{A}sourceKind> \"observed\" .\n"
    ));
    // Removing the one required value it had IS new: refused, actor debt not reported.
    let removed = format!("<{A}old-session> <{A}sourceKind> \"observed\" .\n");
    let v = retraction_violations_in_graph(&store, SHAPES, &removed, "", crate::schema::ROOT_GRAPH)
        .expect("validated");
    assert_eq!(v.violations, 1, "{:?}", v.results);
    assert_eq!(
        v.results[0].path.as_deref(),
        Some(format!("{A}sourceKind").as_str())
    );
}

/// (d) A DIFFERENT, new violation on a debt node is refused; its old debt is
/// still only a Warning.
#[test]
fn a_new_violation_on_a_debt_node_is_refused() {
    let store = store_with(&debt_session());

    let bad_value =
        format!("<{A}old-session> a <{A}Session> ; <{A}sourceKind> \"hand-declared\" .\n");
    let v = validate_with_store_context(&store, SHAPES, &bad_value).expect("validated");
    assert!(
        !v.conforms,
        "a new sh:in violation was admitted: {:?}",
        v.results
    );
    assert_eq!(v.violations, 1, "{:?}", v.results);
    assert_eq!(
        v.results
            .iter()
            .find(|r| r.severity == "Warning")
            .map(|r| r.path.clone()),
        Some(Some(format!("{A}actor")))
    );

    let bad_path = format!("<{A}old-session> a <{A}Session> ; <{A}startedAt> \"yesterday\" .\n");
    let v = validate_with_store_context(&store, SHAPES, &bad_path).expect("validated");
    assert!(
        !v.conforms,
        "a violation on another path was admitted: {:?}",
        v.results
    );
    assert_eq!(v.violations, 1, "{:?}", v.results);
}

/// A new TYPE on an existing node is not debt: its shape had no pre-state.
#[test]
fn a_new_type_on_an_existing_node_is_refused() {
    let store = store_with(&format!("<{A}old-session> <{A}note> \"untyped\" .\n"));
    let v = validate_with_store_context(&store, SHAPES, &debt_session()).expect("validated");
    assert!(!v.conforms);
    assert_eq!(v.violations, 2, "{:?}", v.results);
}

/// (e) A write that repairs the debt clears it: no warnings, no debt events,
/// and a later bare reuse finds nothing to admit.
#[test]
fn a_repairing_write_clears_the_debt() {
    let mut store = store_with(&debt_session());
    let repair = format!(
        "<{A}old-session> a <{A}Session> ; <{A}actor> <{A}alice> ; <{A}sourceKind> \"observed\" .\n"
    );
    let v = validate_with_store_context(&store, SHAPES, &repair).expect("validated");
    assert!(v.conforms, "{:?}", v.results);
    assert_eq!((v.violations, v.warnings), (0, 0), "{:?}", v.results);
    assert!(debt_events(&store).is_empty());

    crate::rdf::ingest_rdf(
        &mut store,
        repair.as_bytes(),
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-10-22T01:00:00Z",
        None,
        Some("test"),
    )
    .expect("repair ingested");
    let v = validate_with_store_context(&store, SHAPES, &debt_session()).expect("validated");
    assert_eq!((v.violations, v.warnings), (0, 0), "{:?}", v.results);
    assert!(debt_events(&store).is_empty());
}

/// Logical and nested components are never admitted, even when the stored node
/// already violates them: their pre-state depends on other nodes, and a bounded
/// context could fabricate it (W3C qualifiedValueShape-001 written in halves).
#[test]
fn logical_component_debt_is_not_admitted() {
    const OR_SHAPES: &str = r#"
@prefix sh:    <http://www.w3.org/ns/shacl#> .
@prefix aegis: <http://aegis.gastown.local/ontology/> .
aegis:RuleShape a sh:NodeShape ;
    sh:targetClass aegis:Rule ;
    sh:or ( [ sh:path aegis:memberOf ; sh:minCount 1 ]
            [ sh:path aegis:governedBy ; sh:minCount 1 ] ) .
"#;
    let rule = format!("<{A}rule> a <{A}Rule> .\n");
    let store = store_with(&rule);
    let v = validate_with_store_context(&store, OR_SHAPES, &rule).expect("validated");
    assert!(!v.conforms, "sh:or debt was admitted: {:?}", v.results);
    assert!(debt_events(&store).is_empty());
}
