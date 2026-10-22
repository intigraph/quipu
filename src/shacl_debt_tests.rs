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

/// Two different anonymous shapes on the same path must not share a debt key
/// (grant, review of #751): a node already lacking `p` under one type's shape,
/// written with a NEW type whose own anonymous shape also requires `p`, is a
/// new violation and is refused.
#[test]
fn a_different_anonymous_shape_on_the_same_path_is_not_debt() {
    const TWO: &str = r#"
@prefix sh:    <http://www.w3.org/ns/shacl#> .
@prefix aegis: <http://aegis.gastown.local/ontology/> .
aegis:OneShape a sh:NodeShape ; sh:targetClass aegis:One ;
    sh:property [ sh:path aegis:p ; sh:minCount 1 ] .
aegis:TwoShape a sh:NodeShape ; sh:targetClass aegis:Two ;
    sh:property [ sh:path aegis:p ; sh:minCount 1 ] .
"#;
    let store = store_with(&format!("<{A}n> a <{A}One> .\n"));
    // CONTROL: reusing the existing type is admitted debt.
    let same = validate_with_store_context(&store, TWO, &format!("<{A}n> a <{A}One> .\n"))
        .expect("validated");
    assert!(same.conforms, "{:?}", same.results);
    // The new type's shape is a different shape: refused.
    let v = validate_with_store_context(&store, TWO, &format!("<{A}n> a <{A}Two> .\n"))
        .expect("validated");
    assert!(
        !v.conforms,
        "a different shape's violation was admitted: {:?}",
        v.results
    );
}

/// Existence is judged per focus (grant, review of #751): a write that touches
/// an existing debt node AND a new node named by `sh:targetNode` gets no
/// allowance for the new node, even though its pre-state is also targeted.
#[test]
fn a_new_target_node_in_the_same_write_is_not_debt() {
    const TARGETED: &str = r#"
@prefix sh:    <http://www.w3.org/ns/shacl#> .
@prefix aegis: <http://aegis.gastown.local/ontology/> .
aegis:FreshShape a sh:NodeShape ; sh:targetNode aegis:fresh ;
    sh:property [ sh:path aegis:p ; sh:minCount 1 ] .
"#;
    let store = store_with(&debt_session());
    let both = format!("{}<{A}fresh> <{A}note> \"new\" .\n", debt_session());
    let v = validate_with_store_context(&store, &format!("{SHAPES}\n{TARGETED}"), &both)
        .expect("validated");
    assert!(
        !v.conforms,
        "a new targetNode focus was admitted: {:?}",
        v.results
    );
    let refused: Vec<_> = v
        .results
        .iter()
        .filter(|r| r.severity != "Warning")
        .collect();
    assert_eq!(refused.len(), 1, "{:?}", v.results);
    assert_eq!(refused[0].focus_node, format!("{A}fresh"));
}

/// A delete can break a required VALUE too (grant, review of #751): removing a
/// `sh:hasValue` value from a conforming node is refused.
#[test]
fn removing_a_required_value_is_refused() {
    const VALUED: &str = r#"
@prefix sh:    <http://www.w3.org/ns/shacl#> .
@prefix aegis: <http://aegis.gastown.local/ontology/> .
aegis:RunShape a sh:NodeShape ; sh:targetClass aegis:Run ;
    sh:property [ sh:path aegis:sourceKind ; sh:hasValue "observed" ] .
"#;
    let store = store_with(&format!(
        "<{A}run> a <{A}Run> ; <{A}sourceKind> \"observed\" .\n"
    ));
    let removed = format!("<{A}run> <{A}sourceKind> \"observed\" .\n");
    let v = retraction_violations_in_graph(&store, VALUED, &removed, "", crate::schema::ROOT_GRAPH)
        .expect("validated");
    assert!(!v.conforms, "a delete broke a required value and passed");
    assert!(
        v.results[0]
            .component
            .ends_with("HasValueConstraintComponent"),
        "{:?}",
        v.results
    );
}

/// A value held in BOTH ROOT and graph `g` survives deleting `g`'s copy (grant,
/// review of #751): the post-state is built per graph, so the delete is not
/// refused. Paired control: when `g` holds the only copy, the delete is refused.
#[test]
fn a_delete_in_a_graph_keeps_roots_copy_of_the_value() {
    let session =
        format!("<{A}s> a <{A}Session> ; <{A}actor> <{A}alice> ; <{A}sourceKind> \"observed\" .\n");
    let removed = format!("<{A}s> <{A}actor> <{A}alice> .\n");

    let mut store = store_with(&session);
    let g = store.graph_create("http://x/graph/plane").expect("graph");
    crate::rdf::ingest_rdf_to_graph(
        &mut store,
        session.as_bytes(),
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-10-22T00:00:00Z",
        None,
        Some("test"),
        g,
    )
    .expect("same facts in the plane");
    let v = retraction_violations_in_graph(&store, SHAPES, &removed, "", g).expect("validated");
    assert!(
        v.conforms,
        "ROOT's copy was lost from the post-state: {:?}",
        v.results
    );

    // CONTROL: the plane alone holds the value, so deleting it empties the path.
    let mut store = Store::open_in_memory().expect("store");
    let g = store.graph_create("http://x/graph/plane").expect("graph");
    crate::rdf::ingest_rdf_to_graph(
        &mut store,
        session.as_bytes(),
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-10-22T00:00:00Z",
        None,
        Some("test"),
        g,
    )
    .expect("facts only in the plane");
    let v = retraction_violations_in_graph(&store, SHAPES, &removed, "", g).expect("validated");
    assert!(
        !v.conforms,
        "the only copy was deleted and the delete passed"
    );
}

/// KNOWN GAP, pinned (sattler's ruling on aegis-g51c C12): a delete is judged
/// only for minCount and hasValue. One that breaks a NON-LOCAL constraint, here
/// an `sh:or` of two floors, is not refused today. Widening that is a separate
/// follow-up; when it lands, this test is expected to flip.
#[test]
fn a_delete_that_breaks_an_or_constraint_is_not_judged_yet() {
    const EITHER: &str = r#"
@prefix sh:    <http://www.w3.org/ns/shacl#> .
@prefix aegis: <http://aegis.gastown.local/ontology/> .
aegis:EitherShape a sh:NodeShape ; sh:targetClass aegis:Either ;
    sh:or ( [ sh:path aegis:a ; sh:minCount 1 ] [ sh:path aegis:b ; sh:minCount 1 ] ) .
"#;
    let store = store_with(&format!("<{A}e> a <{A}Either> ; <{A}a> \"x\" .\n"));
    let removed = format!("<{A}e> <{A}a> \"x\" .\n");
    // CONTROL: the post-state really does violate the sh:or.
    let post = crate::shacl::validate_shapes(EITHER, &format!("<{A}e> a <{A}Either> .\n"))
        .expect("validated");
    assert!(!post.conforms, "control: the fixture must break the sh:or");
    let v = retraction_violations_in_graph(&store, EITHER, &removed, "", crate::schema::ROOT_GRAPH)
        .expect("validated");
    assert!(
        v.conforms,
        "the known gap changed; update the docs and this pin: {:?}",
        v.results
    );
}

const CLASS_SHAPES: &str = r#"
@prefix sh:    <http://www.w3.org/ns/shacl#> .
@prefix aegis: <http://aegis.gastown.local/ontology/> .
aegis:ArtifactShape a sh:NodeShape ; sh:targetClass aegis:Artifact ;
    sh:property [ sh:path aegis:contains ; sh:class aegis:WorkItem ] .
"#;

/// A store whose Artifact already `contains` a non-WorkItem (stored before the
/// shape existed), with write validation on and the shape loaded after.
fn store_with_value_debt() -> Store {
    let mut store = store_with(&format!(
        "<{A}art> a <{A}Artifact> ; <{A}contains> <{A}thing> .\n<{A}thing> a <{A}Host> .\n"
    ));
    store.shacl_config_mut().validate_on_write = true;
    store
        .load_shapes("class", CLASS_SHAPES, "2026-10-22T00:30:00Z")
        .expect("shapes");
    store
}

fn write(store: &mut Store, turtle: &str) -> crate::Result<(i64, usize)> {
    crate::rdf::ingest_rdf(
        store,
        turtle.as_bytes(),
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-10-22T01:00:00Z",
        None,
        Some("test"),
    )
}

/// The savepoint gate's stored-VALUE debt (replay of #751 on the served shapes:
/// Artifact/ConfigSetting/Host `contains`, Policy `targets`): a bare type reuse
/// of the debt node is admitted, and counted.
#[test]
fn reuse_of_a_value_debt_node_passes_the_post_state_gate() {
    let mut store = store_with_value_debt();
    write(&mut store, &format!("<{A}art> a <{A}Artifact> .\n"))
        .expect("a bare reuse of a value-debt node was refused");
}

/// The same bad value on a NEW node is refused by the post-state gate.
#[test]
fn the_same_bad_value_on_a_new_node_is_refused_post_state() {
    let mut store = store_with_value_debt();
    let err = write(
        &mut store,
        &format!("<{A}art2> a <{A}Artifact> ; <{A}contains> <{A}thing> .\n"),
    );
    assert!(err.is_err(), "a new node's class violation was admitted");
}

/// A NEW bad value on the debt node is refused; its old value stays debt.
#[test]
fn a_new_bad_value_on_a_value_debt_node_is_refused_post_state() {
    let mut store = store_with_value_debt();
    let err = write(
        &mut store,
        &format!("<{A}other> a <{A}Host> .\n<{A}art> <{A}contains> <{A}other> .\n"),
    );
    assert!(
        err.is_err(),
        "a new class violation on a debt node was admitted"
    );
}

/// A node already over its `sh:maxCount` is not a licence to add more values:
/// the count result carries no value, so `MaxCount` is never admitted as debt.
#[test]
fn adding_to_an_over_count_path_is_not_debt() {
    const ONE_LABEL: &str = r#"
@prefix sh:    <http://www.w3.org/ns/shacl#> .
@prefix aegis: <http://aegis.gastown.local/ontology/> .
aegis:ModShape a sh:NodeShape ; sh:targetClass aegis:Mod ;
    sh:property [ sh:path aegis:lang ; sh:maxCount 1 ] .
"#;
    let store = store_with(&format!(
        "<{A}m> a <{A}Mod> ; <{A}lang> \"rust\" , \"Rust\" .\n"
    ));
    let more = format!("<{A}m> a <{A}Mod> ; <{A}lang> \"rust\" , \"Rust\" , \"rs\" .\n");
    let v = validate_with_store_context(&store, ONE_LABEL, &more).expect("validated");
    assert!(
        !v.conforms,
        "an over-count path grew and was admitted: {:?}",
        v.results
    );
}
